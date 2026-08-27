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

/// One central directory entry, reduced to what this crate reads off it.
pub(crate) struct Recorded {
    /// General purpose bit 11: the name is UTF-8 when set, CP437 otherwise.
    pub utf8: bool,
    pub bytes: Vec<u8>,
    /// The member's external attributes, exactly as recorded.
    ///
    /// Read here rather than through the ZIP crate because the crate's
    /// `unix_mode` does not distinguish a mode a writer recorded from one it
    /// invented: for an archive made on DOS with no high bits it returns
    /// `S_IFREG | 0o664`, and for a read-only one `0o444`. Both are answers to
    /// a question the container never answered, and
    /// [`Container::payload_mode`](crate::Container::payload_mode) has to be
    /// silent there rather than confident.
    pub external_attributes: u32,
}

impl Recorded {
    /// Does this name decode to `want`, as SPEC 2.1 requires it be decoded?
    ///
    /// Comparison is exact over the decoded code points: case-sensitive, and no
    /// Unicode normalization on either side.
    pub fn decodes_to(&self, want: &str) -> bool {
        if self.utf8 {
            // A name flagged UTF-8 whose bytes are not UTF-8 has no decoding,
            // so it equals nothing. The ZIP crate substitutes U+FFFD instead
            // and reports neither the flag nor the substitution, which is why
            // names are decoded here rather than taken from it.
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

/// Every name in the central directory, duplicates included.
///
/// Reads names and the one flag bit that decodes them, and skips everything
/// else. The reader is left wherever this finished with it; the caller rewinds.
pub(crate) fn names<R: Read + Seek>(reader: &mut R) -> Result<Vec<Recorded>> {
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
        let external_attributes = u32::from_le_bytes(header[38..42].try_into().unwrap());
        let name_len = u16::from_le_bytes(header[28..30].try_into().unwrap()) as usize;
        let extra_len = i64::from(u16::from_le_bytes(header[30..32].try_into().unwrap()));
        let comment_len = i64::from(u16::from_le_bytes(header[32..34].try_into().unwrap()));

        let mut bytes = vec![0u8; name_len];
        reader.read_exact(&mut bytes)?;
        reader.seek(SeekFrom::Current(extra_len + comment_len))?;

        names.push(Recorded {
            utf8: flags & (1 << 11) != 0,
            bytes,
            external_attributes,
        });
    }
    Ok(names)
}

/// How many entries the central directory holds, and where it starts.
///
/// **Everything this function refuses, it refuses because agreeing with the ZIP
/// crate matters more than reading one more file.** This crate resolves the
/// central directory twice — here, to count names the crate cannot see, and in
/// the crate itself, for every byte anything actually reads. Where the two
/// resolve *different* directories, the uniqueness SPEC 2.1 requires is
/// established over one set of members and the payload is served from another,
/// which is the shape of Android's Master Key bug and is what SPEC 3's
/// enumeration rule exists to prevent. Measured on 2026-08-27, three separate
/// fields could be made to split them, so the answer is not to chase the crate's
/// behaviour field by field but to refuse any archive where the question has
/// more than one answer.
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
    let this_disk = u16::from_le_bytes(eocd[4..6].try_into().unwrap());
    let directory_disk = u16::from_le_bytes(eocd[6..8].try_into().unwrap());
    let here = u64::from(u16::from_le_bytes(eocd[8..10].try_into().unwrap()));
    let count = u64::from(u16::from_le_bytes(eocd[10..12].try_into().unwrap()));
    let size = u64::from(u32::from_le_bytes(eocd[12..16].try_into().unwrap()));
    let offset = u64::from(u32::from_le_bytes(eocd[16..20].try_into().unwrap()));
    let comment_len = u64::from(u16::from_le_bytes(eocd[20..22].try_into().unwrap()));

    // The two count fields are *entries on this disk* and *entries in total*.
    // A single-disk archive has them equal, and every writer produces one. Read
    // apart they are a lever: this function used to take the total and the ZIP
    // crate takes the count on this disk, so an archive declaring 3 and 2 was
    // counted here as two members and served by the crate as three — a
    // duplicate payload smuggled past a conformant verdict.
    if here != count {
        return Err(Malformed::NotAnArchive(format!(
            "the end of central directory record says {here} entries on this disk and {count} in total; a container is a single-disk archive"
        ))
        .into());
    }
    if this_disk != 0 || directory_disk != 0 {
        return Err(Malformed::NotAnArchive(
            "the archive is split across disks; a container is a single-disk archive".into(),
        )
        .into());
    }

    // The record has to be the last thing in the file, its comment included.
    // Without this the *last* signature wins here while the ZIP crate, which
    // checks the comment length and keeps looking, falls back to an earlier
    // record — two directories, two payloads, one verdict. Refusing rather than
    // falling back too: a file with a second end-of-central-directory record
    // that does not add up is one whose contents depend on who is reading, and
    // SPEC 2.1 has already taken that decision about duplicate names and about
    // two archives in one file.
    let record_end = from + at as u64 + 22 + comment_len;
    if record_end != end {
        return Err(Malformed::NotAnArchive(format!(
            "the end of central directory record does not end the file: it accounts for {record_end} bytes of {end}"
        ))
        .into());
    }

    // The same gate the ZIP crate applies, `size` included. Reading Zip64 on a
    // different trigger than the crate is the third way to split the two
    // parsers: set only the directory-size sentinel and the crate reads the
    // Zip64 record while this reads the one beside it.
    let sentinel = count == u64::from(u16::MAX)
        || offset == u64::from(u32::MAX)
        || size == u64::from(u32::MAX);
    if sentinel {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn utf8(bytes: &[u8]) -> Recorded {
        Recorded {
            external_attributes: 0,
            utf8: true,
            bytes: bytes.to_vec(),
        }
    }
    fn cp437_named(bytes: &[u8]) -> Recorded {
        Recorded {
            external_attributes: 0,
            utf8: false,
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn a_name_decodes_to_itself() {
        assert!(utf8(b"report.pdf").decodes_to("report.pdf"));
        // Bit 11 clear, so CP437: 0x87 is U+00E7.
        assert!(cp437_named(b"caf\x87.txt").decodes_to("caf\u{e7}.txt"));
        // The same bytes read as UTF-8 are not that name, and are not any name.
        assert!(!utf8(b"caf\x87.txt").decodes_to("caf\u{e7}.txt"));
    }

    #[test]
    fn a_name_flagged_utf8_that_is_not_utf8_equals_nothing() {
        // The ZIP crate would hand back U+FFFD here. A name with no decoding
        // must match no `payload.file`, which is always a real string, or two
        // members could collapse to one and the answer would depend on the
        // order they sit in.
        let bad = utf8(b"caf\xff.txt");
        assert!(!bad.decodes_to("caf\u{fffd}.txt"));
        assert!(!bad.decodes_to("caf\u{e7}.txt"));
    }

    #[test]
    fn a_replacement_character_someone_meant_is_a_name_like_any_other() {
        let honest = utf8("caf\u{fffd}.txt".as_bytes());
        assert!(honest.decodes_to("caf\u{fffd}.txt"));
    }

    #[test]
    fn cp437_covers_every_byte_and_repeats_none() {
        // What makes the two branches distinguishable: no byte decodes to
        // U+FFFD, so a replacement character can only have come from a lossy
        // UTF-8 read.
        let all: Vec<char> = (0u8..=255).map(cp437).collect();
        let mut seen: Vec<char> = all.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 256, "every byte maps to a distinct character");
        assert!(!all.contains(&'\u{fffd}'));
    }

    #[test]
    fn comparison_is_exact_over_code_points() {
        // SPEC 2.1: case-sensitive, and no normalization on either side.
        assert!(!utf8(b"Report.pdf").decodes_to("report.pdf"));
        let nfc = "caf\u{e9}.txt";
        let nfd = "cafe\u{301}.txt";
        assert!(!utf8(nfc.as_bytes()).decodes_to(nfd));
    }
}
