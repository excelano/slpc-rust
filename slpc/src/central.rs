// Reading the central directory directly, for the one question the ZIP crate
// cannot answer.
//
// `ZipArchive` keys its directory by name, so two members sharing a name arrive
// as one and `len()` counts them once. SPEC 2.1 requires exactly one member
// named `slipcase.metadata.toml` and exactly one matching `payload.file`, which
// means counting them, which means reading the directory ourselves. Nothing
// else here duplicates the crate: members are still located and read through
// it, and this only ever counts.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::io::{Read, Seek, SeekFrom};

use crate::error::{Malformed, Result};

const EOCD: u32 = 0x0605_4B50;
const EOCD64_LOCATOR: u32 = 0x0706_4B50;
const EOCD64: u32 = 0x0606_4B50;
const CENTRAL_HEADER: u32 = 0x0201_4B50;

/// The ZIP comment is a 16-bit length, so the record starts no further back.
const MAX_EOCD_SEARCH: u64 = 22 + u16::MAX as u64;

/// One central directory entry, reduced to what naming needs.
pub(crate) struct RawName {
    /// General purpose bit 11: the name is UTF-8 when set, CP437 otherwise.
    pub utf8: bool,
    pub bytes: Vec<u8>,
}

impl RawName {
    /// Does this name decode to `want`, as SPEC 2.1 requires it be decoded?
    ///
    /// Comparison is exact over the decoded code points: case-sensitive, and no
    /// Unicode normalization on either side.
    pub fn decodes_to(&self, want: &str) -> bool {
        if self.utf8 {
            // A name flagged UTF-8 whose bytes are not UTF-8 has no decoding,
            // so it equals nothing. The ZIP crate substitutes U+FFFD instead,
            // which is what `name::matches` has to guard against; here the flag
            // is in hand and the answer is direct.
            std::str::from_utf8(&self.bytes).is_ok_and(|s| s == want)
        } else {
            self.bytes.len() == want.chars().count()
                && self.bytes.iter().copied().map(cp437).eq(want.chars())
        }
    }
}

/// One byte of CP437 as a character.
///
/// The table is IBM code page 437's upper half, transcribed from the `zip`
/// crate's own so that this and the crate cannot disagree about a name. All 128
/// entries are distinct and none is U+FFFD, which is what lets the guard in
/// `name.rs` tell a CP437 name from a lossily decoded one.
fn cp437(b: u8) -> char {
    const HIGH: [char; 128] = [
        '\u{00c7}', '\u{00fc}', '\u{00e9}', '\u{00e2}', '\u{00e4}', '\u{00e0}', '\u{00e5}',
        '\u{00e7}', '\u{00ea}', '\u{00eb}', '\u{00e8}', '\u{00ef}', '\u{00ee}', '\u{00ec}',
        '\u{00c4}', '\u{00c5}', '\u{00c9}', '\u{00e6}', '\u{00c6}', '\u{00f4}', '\u{00f6}',
        '\u{00f2}', '\u{00fb}', '\u{00f9}', '\u{00ff}', '\u{00d6}', '\u{00dc}', '\u{00a2}',
        '\u{00a3}', '\u{00a5}', '\u{20a7}', '\u{0192}', '\u{00e1}', '\u{00ed}', '\u{00f3}',
        '\u{00fa}', '\u{00f1}', '\u{00d1}', '\u{00aa}', '\u{00ba}', '\u{00bf}', '\u{2310}',
        '\u{00ac}', '\u{00bd}', '\u{00bc}', '\u{00a1}', '\u{00ab}', '\u{00bb}', '\u{2591}',
        '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
        '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255d}', '\u{255c}', '\u{255b}',
        '\u{2510}', '\u{2514}', '\u{2534}', '\u{252c}', '\u{251c}', '\u{2500}', '\u{253c}',
        '\u{255e}', '\u{255f}', '\u{255a}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}',
        '\u{2550}', '\u{256c}', '\u{2567}', '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}',
        '\u{2558}', '\u{2552}', '\u{2553}', '\u{256b}', '\u{256a}', '\u{2518}', '\u{250c}',
        '\u{2588}', '\u{2584}', '\u{258c}', '\u{2590}', '\u{2580}', '\u{03b1}', '\u{00df}',
        '\u{0393}', '\u{03c0}', '\u{03a3}', '\u{03c3}', '\u{00b5}', '\u{03c4}', '\u{03a6}',
        '\u{0398}', '\u{03a9}', '\u{03b4}', '\u{221e}', '\u{03c6}', '\u{03b5}', '\u{2229}',
        '\u{2261}', '\u{00b1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00f7}',
        '\u{2248}', '\u{00b0}', '\u{2219}', '\u{00b7}', '\u{221a}', '\u{207f}', '\u{00b2}',
        '\u{25a0}', '\u{00a0}',
    ];
    if b < 0x80 {
        b as char
    } else {
        HIGH[b as usize - 0x80]
    }
}

/// How many names decode to `want`.
pub(crate) fn count(names: &[RawName], want: &str) -> usize {
    names.iter().filter(|n| n.decodes_to(want)).count()
}

/// Every name in the central directory, duplicates included.
///
/// Reads names and the one flag bit that decodes them, and skips everything
/// else. The reader is left wherever this finished with it; the caller rewinds.
pub(crate) fn names<R: Read + Seek>(reader: &mut R) -> Result<Vec<RawName>> {
    let (count, offset) = directory_location(reader)?;
    reader.seek(SeekFrom::Start(offset))?;

    let mut names = Vec::with_capacity(usize::try_from(count).unwrap_or_default().min(4096));
    for _ in 0..count {
        let mut header = [0u8; 46];
        reader.read_exact(&mut header)?;
        if u32::from_le_bytes(header[0..4].try_into().unwrap()) != CENTRAL_HEADER {
            return Err(Malformed::NotAnArchive(
                "the central directory ends before it says it does".into(),
            )
            .into());
        }
        let flags = u16::from_le_bytes(header[8..10].try_into().unwrap());
        let name_len = u16::from_le_bytes(header[28..30].try_into().unwrap()) as usize;
        let extra_len = i64::from(u16::from_le_bytes(header[30..32].try_into().unwrap()));
        let comment_len = i64::from(u16::from_le_bytes(header[32..34].try_into().unwrap()));

        let mut bytes = vec![0u8; name_len];
        reader.read_exact(&mut bytes)?;
        reader.seek(SeekFrom::Current(extra_len + comment_len))?;

        names.push(RawName {
            utf8: flags & (1 << 11) != 0,
            bytes,
        });
    }
    Ok(names)
}

/// How many entries the central directory holds, and where it starts.
fn directory_location<R: Read + Seek>(reader: &mut R) -> Result<(u64, u64)> {
    let end = reader.seek(SeekFrom::End(0))?;
    let window = MAX_EOCD_SEARCH.min(end);
    let from = end - window;
    reader.seek(SeekFrom::Start(from))?;
    let mut tail = vec![0u8; usize::try_from(window).unwrap_or(usize::MAX)];
    reader.read_exact(&mut tail)?;

    // Scan backwards: the last signature is the real record, since an archive
    // comment may contain bytes that look like one.
    let at = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| u32::from_le_bytes(tail[i..i + 4].try_into().unwrap()) == EOCD)
        .ok_or_else(|| Malformed::NotAnArchive("no end of central directory record".into()))?;

    let eocd = &tail[at..];
    let count = u64::from(u16::from_le_bytes(eocd[10..12].try_into().unwrap()));
    let offset = u64::from(u32::from_le_bytes(eocd[16..20].try_into().unwrap()));

    // Zip64 puts 0xFFFF or 0xFFFFFFFF in the field it has outgrown and the real
    // value in its own record, found through a locator just before this one.
    if count == u64::from(u16::MAX) || offset == u64::from(u32::MAX) {
        if let Some(z) = zip64(&tail, at, reader)? {
            return Ok(z);
        }
    }
    Ok((count, offset))
}

/// The Zip64 end of central directory record, if the locator is there.
fn zip64<R: Read + Seek>(
    tail: &[u8],
    eocd_at: usize,
    reader: &mut R,
) -> Result<Option<(u64, u64)>> {
    if eocd_at < 20 {
        return Ok(None);
    }
    let loc = &tail[eocd_at - 20..eocd_at];
    if u32::from_le_bytes(loc[0..4].try_into().unwrap()) != EOCD64_LOCATOR {
        return Ok(None);
    }
    let at = u64::from_le_bytes(loc[8..16].try_into().unwrap());

    reader.seek(SeekFrom::Start(at))?;
    let mut rec = [0u8; 56];
    reader.read_exact(&mut rec)?;
    if u32::from_le_bytes(rec[0..4].try_into().unwrap()) != EOCD64 {
        return Ok(None);
    }
    Ok(Some((
        u64::from_le_bytes(rec[32..40].try_into().unwrap()),
        u64::from_le_bytes(rec[48..56].try_into().unwrap()),
    )))
}
