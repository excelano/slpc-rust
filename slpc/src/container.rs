// Reading a container.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::io::{Read, Seek};
use std::path::Path;

use toml_edit::DocumentMut;
use zip::ZipArchive;

use crate::error::{EntryKind, Error, Malformed, Result, Unsupported};
use crate::{central, metadata, name, METADATA_MEMBER, VERSION};

/// One member of the central directory, as far as this library cares.
///
/// Collected in a single pass at open time because `ZipArchive` lends out one
/// member at a time and re-reading the directory per lookup would be the same
/// work done repeatedly. The rewrite path walks the same list to copy members
/// through in the order they arrived.
pub(crate) struct Entry {
    raw: Vec<u8>,
    kind: EntryKind,
    /// The member's uncompressed length, kept from the same pass that read the
    /// name and the kind. It is eight bytes per member against a lookup that
    /// would otherwise need the archive, and needing the archive is what would
    /// make asking a member's size require `&mut`.
    size: u64,
    /// Whether general purpose bit 0 is set on the member.
    encrypted: bool,
    /// The compression method's number, when it is one this build carries no
    /// decoder for, and `None` for every method it can decode. Kept for the
    /// same reason as `size`: the answer is in the central directory, and going
    /// back to the archive for it is what would need `&mut`.
    unsupported_method: Option<u16>,
}

/// Collect what the central directory says about every member, in one pass.
///
/// `by_index_raw` rather than `by_index`: the latter refuses a member whose
/// compression method this build was not given, which is exactly the member
/// that has to survive being listed and copied. Nothing here decompresses
/// anything.
fn entries_of<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<Vec<Entry>> {
    let mut entries = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let f = archive.by_index_raw(i)?;
        entries.push(Entry {
            raw: f.name_raw().to_owned(),
            kind: EntryKind::from_mode(f.unix_mode()),
            size: f.size(),
            encrypted: f.encrypted(),
            unsupported_method: unsupported_method(f.compression()),
        });
    }
    Ok(entries)
}

/// The method's number, when it is one this build carries no decoder for.
///
/// The ZIP crate gates each `CompressionMethod` variant behind one of its own
/// features, so a method the build was not given arrives as `Unsupported`
/// carrying the number the archive stated. That is the test the crate itself
/// makes before it builds a decoder, which is what keeps
/// [`Container::check_payload_readable`] from answering differently from
/// extraction.
#[allow(deprecated)]
fn unsupported_method(method: zip::CompressionMethod) -> Option<u16> {
    match method {
        zip::CompressionMethod::Unsupported(id) => Some(id),
        _ => None,
    }
}

/// How many members of the central directory carry a given name.
///
/// A name is either absent, borne by one member, or borne by several, and SPEC
/// 2.1 has a different thing to say about each. Which error a caller raises
/// depends on which name it was looking for, so this reports and does not
/// decide.
pub(crate) enum Located {
    One(usize),
    None,
    Several(usize),
}

/// Find the one archive member whose name decodes to `want`.
///
/// Counting happens over the central directory read in `central.rs`, because
/// `ZipArchive` keys its own directory by name and cannot see a duplicate. The
/// index returned is the archive's, found by the name's raw bytes.
///
/// Matching on raw bytes is exact here, and only because the count ran first.
/// Two members can carry one byte string and decode differently only when their
/// flag bits differ over non-ASCII bytes; over ASCII both branches agree, which
/// would make them duplicates and stop this before it began.
pub(crate) fn locate(entries: &[Entry], names: &[central::RawName], want: &str) -> Located {
    let mut matched = names.iter().filter(|n| n.decodes_to(want));
    let Some(first) = matched.next() else {
        return Located::None;
    };
    let count = 1 + matched.count();
    if count > 1 {
        return Located::Several(count);
    }
    // The archive and the central directory are the same directory, so an entry
    // counted there is an entry here.
    match entries.iter().position(|e| e.raw == first.bytes) {
        Some(i) => Located::One(i),
        None => Located::None,
    }
}

/// A slipcase container, open for reading.
///
/// Reading needs `Read + Seek`, because a ZIP's central directory is at the end
/// of the file and there is no way to find a member without first finding that.
pub struct Container<R> {
    pub(crate) archive: ZipArchive<R>,
    pub(crate) entries: Vec<Entry>,
    /// Every central directory name, duplicates included, for the counting the
    /// ZIP crate cannot do. Kept so a rewrite can check the metadata it is
    /// about to write against the same archive.
    pub(crate) names: Vec<central::RawName>,
    pub(crate) metadata_index: usize,
    doc: DocumentMut,
    bytes: Vec<u8>,
    version: String,
    payload_file: String,
    /// `None` when `slipcase_version` is one this build does not implement, in
    /// which case the payload was never located. See [`Container::payload`].
    pub(crate) payload_index: Option<usize>,
}

impl Container<std::fs::File> {
    /// Open a container from a path.
    ///
    /// Named for [`std::fs::File::open`] rather than for symmetry with the
    /// packing side, because that convention is the one a reader already knows.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::read(std::fs::File::open(path)?)
    }
}

impl<R: Read + Seek> Container<R> {
    /// Read a container from anything seekable.
    ///
    /// Reads the central directory and the metadata member, and nothing else.
    /// The payload is not decompressed and not read, so a container whose
    /// payload uses a compression method this build lacks still opens.
    pub fn read(mut reader: R) -> Result<Self> {
        // Count names before the archive is built, because `ZipArchive` keys
        // its directory by name and two members sharing one arrive as a single
        // entry. SPEC 2.1 requires exactly one of each named member, which is
        // a question the crate cannot be asked.
        let names = central::names(&mut reader)?;
        reader.rewind()?;

        let mut archive = ZipArchive::new(reader)?;

        let entries = entries_of(&mut archive)?;

        let meta_index = match locate(&entries, &names, METADATA_MEMBER) {
            Located::One(i) => i,
            Located::None => return Err(Malformed::NoMetadataMember.into()),
            Located::Several(n) => return Err(Malformed::DuplicateMetadataMember(n).into()),
        };

        // Buffering here is deliberate and is not the rule the payload lives
        // under: the metadata member is a small TOML document, and every
        // caller of this library wants all of it.
        let mut bytes = Vec::new();
        archive.by_index(meta_index)?.read_to_end(&mut bytes)?;

        let (doc, keys) = metadata::parse(&bytes)?;
        let crate::metadata::Keys {
            version,
            payload_file,
        } = keys;

        // Everything past this point is a rule stated by version 1.0 of the
        // specification. A container declaring a version this build does not
        // implement is parsed and reported and no further, because SPEC 3
        // forbids assuming its rules are the ones written here.
        let payload_index = if version == VERSION {
            Some(locate_payload(&entries, &names, &payload_file)?)
        } else {
            None
        };

        Ok(Self {
            archive,
            entries,
            names,
            metadata_index: meta_index,
            doc,
            bytes,
            version,
            payload_file,
            payload_index,
        })
    }

    /// The `slipcase_version` as written.
    ///
    /// This and [`Container::payload_name`] describe the container as it was
    /// read. Editing the document through [`Container::metadata_mut`] does not
    /// change them; the edited document is validated when it is written back.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The value of `payload.file`.
    pub fn payload_name(&self) -> &str {
        &self.payload_file
    }

    /// The whole TOML document, unknown keys intact.
    pub fn metadata(&self) -> &DocumentMut {
        &self.doc
    }

    /// The whole TOML document, to be changed in place.
    pub fn metadata_mut(&mut self) -> &mut DocumentMut {
        &mut self.doc
    }

    /// The metadata member as stored, byte for byte.
    ///
    /// For a caller who wants a different parser, a schema validator, or a
    /// hash. Nothing else here promises to reproduce these bytes: TOML defines
    /// no canonical serialization, so re-serializing the document is not the
    /// same operation.
    pub fn metadata_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether `slipcase_version` is one this build implements.
    pub(crate) fn version_is_recognised(&self) -> bool {
        self.payload_index.is_some()
    }

    /// The payload's length, uncompressed.
    ///
    /// Read from the central directory, which already carries it, so this
    /// decompresses nothing and costs no more than asking. For a caller sizing
    /// a progress bar, deciding whether a payload fits somewhere, or reporting
    /// what is in a container without extracting it.
    ///
    /// Fails the way [`Container::payload`] does for a container declaring a
    /// version this build does not implement, since in that case the payload
    /// was never located.
    ///
    /// Borrows shared rather than mutably, unlike [`Container::payload`], so it
    /// composes with [`Container::payload_name`] in one expression — which is
    /// how anything reporting what is in a container asks the question:
    ///
    /// ```no_run
    /// # fn main() -> slpc::Result<()> {
    /// let c = slpc::Container::open("report.pdf.slpc")?;
    /// println!("{} is {} bytes", c.payload_name(), c.payload_size()?);
    /// # Ok(())
    /// # }
    /// ```
    pub fn payload_size(&self) -> Result<u64> {
        let i = self
            .payload_index
            .ok_or_else(|| Unsupported::Version(self.version.clone()))?;
        Ok(self.entries[i].size)
    }

    /// Whether this build can decode the payload, and what stops it when it
    /// cannot.
    ///
    /// Read off the central directory entry collected when the container was
    /// opened, so this decompresses nothing, reads nothing further, and
    /// borrows shared. It is for a program that has to commit to extraction
    /// before performing it: a button offering to open the payload, a menu
    /// item, a plan stating what it is about to do. The alternative is to
    /// attempt the extraction and read the answer off the failure.
    ///
    /// The three refusals are [`Container::payload`]'s own, in the order it
    /// meets them. A container declaring a version this build does not
    /// implement never had its payload located, so that answer comes first. An
    /// encrypted member is next, because a member can be encrypted and
    /// compressed at once and the archive is asked about encryption first. A
    /// compression method this build carries no decoder for is last.
    ///
    /// None of the three makes the container non-conformant. SPEC 2.5 puts
    /// compression and encryption outside the conformance question, so this is
    /// a capability query and not a verdict: [`validate`](crate::validate)
    /// answers that one, and it reports such a container conformant.
    ///
    /// **`Ok` is not a promise that extraction will succeed.** It says this
    /// build knows how to decode the member. The bytes can still be truncated,
    /// fail their checksum, or fail to read, and [`Container::payload`] and the
    /// stream it returns report that if it happens.
    ///
    /// ```no_run
    /// # fn main() -> slpc::Result<()> {
    /// let c = slpc::Container::open("report.pdf.slpc")?;
    /// if let Err(why) = c.check_payload_readable() {
    ///     println!("{} cannot be opened here: {why}", c.payload_name());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn check_payload_readable(&self) -> std::result::Result<(), Unsupported> {
        let i = self
            .payload_index
            .ok_or_else(|| Unsupported::Version(self.version.clone()))?;
        if self.entries[i].encrypted {
            return Err(Unsupported::Encrypted);
        }
        if let Some(m) = self.entries[i].unsupported_method {
            return Err(Unsupported::Compression(m));
        }
        Ok(())
    }

    /// The payload, as a stream.
    ///
    /// Never buffered whole. A payload is a file of arbitrary size, and a
    /// library that handed back a `Vec<u8>` would be deciding for its caller
    /// that the file fits in memory.
    pub fn payload(&mut self) -> Result<impl Read + '_> {
        let i = self
            .payload_index
            .ok_or_else(|| Unsupported::Version(self.version.clone()))?;
        Ok(self.archive.by_index(i)?)
    }
}

/// The metadata document of a byte stream, asking no conformance question.
///
/// Reads the member SPEC 2.1 names and parses it, requiring of that member what
/// SPEC 2.2 requires: one of it, valid TOML, UTF-8. It looks for neither
/// required key and never locates a payload.
///
/// This exists because a container can be non-conformant somewhere else
/// entirely and still carry a metadata document worth reading: `payload.file`
/// naming no member, naming several, or naming something SPEC 2.3 forbids all
/// leave a document that parsed cleanly, and [`Container::read`] returns an
/// error over the payload before a caller can reach it. A program showing a
/// person what is in a file wants to show them that document.
///
/// It is not a verdict and must not be used as one: a document coming back
/// says nothing about whether the container conforms. Ask
/// [`validate`](crate::validate) for that, which is the only function here that
/// answers the question SPEC 3 constrains.
pub fn metadata_of<R: Read + Seek>(mut reader: R) -> Result<DocumentMut> {
    let names = central::names(&mut reader)?;
    reader.rewind()?;

    let mut archive = ZipArchive::new(reader)?;
    let entries = entries_of(&mut archive)?;

    let i = match locate(&entries, &names, METADATA_MEMBER) {
        Located::One(i) => i,
        Located::None => return Err(Malformed::NoMetadataMember.into()),
        Located::Several(n) => return Err(Malformed::DuplicateMetadataMember(n).into()),
    };

    let mut bytes = Vec::new();
    archive.by_index(i)?.read_to_end(&mut bytes)?;
    metadata::document(&bytes)
}

/// Find the member `payload.file` names, and check it may be a payload.
pub(crate) fn locate_payload(
    entries: &[Entry],
    names: &[central::RawName],
    payload_file: &str,
) -> Result<usize> {
    name::check_payload_name(payload_file)?;

    let i = match locate(entries, names, payload_file) {
        Located::One(i) => i,
        Located::None => return Err(Malformed::NoPayloadMember(payload_file.to_owned()).into()),
        Located::Several(count) => {
            return Err(Malformed::DuplicatePayloadMember {
                name: payload_file.to_owned(),
                count,
            }
            .into())
        }
    };

    // SPEC 2.3 requires a regular file entry, so every other type an archive
    // can record is excluded rather than just symbolic links.
    if entries[i].kind != EntryKind::Regular {
        return Err(Error::Malformed(Malformed::PayloadNotARegularFile {
            name: payload_file.to_owned(),
            kind: entries[i].kind,
        }));
    }
    Ok(i)
}
