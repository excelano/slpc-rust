// Reading a container.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::io::{Read, Seek};
use std::path::Path;

use toml_edit::DocumentMut;
use zip::ZipArchive;

use crate::error::{Error, Malformed, Result, Unsupported};
use crate::{name, METADATA_MEMBER, PAYLOAD_FILE_KEY, VERSION, VERSION_KEY};

/// One member of the central directory, as far as this library cares.
///
/// Collected in a single pass at open time because `ZipArchive` lends out one
/// member at a time and re-reading the directory per lookup would be the same
/// work done repeatedly.
struct Entry {
    name: String,
    raw: Vec<u8>,
    symlink: bool,
}

/// A slipcase container, open for reading.
///
/// Reading needs `Read + Seek`, because a ZIP's central directory is at the end
/// of the file and there is no way to find a member without first finding that.
pub struct Container<R> {
    archive: ZipArchive<R>,
    doc: DocumentMut,
    bytes: Vec<u8>,
    version: String,
    payload_file: String,
    /// `None` when `slipcase_version` is one this build does not implement, in
    /// which case the payload was never located. See [`Container::payload`].
    payload_index: Option<usize>,
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
    pub fn read(reader: R) -> Result<Self> {
        let mut archive = ZipArchive::new(reader)?;

        // `by_index_raw` rather than `by_index`: the latter refuses a member
        // whose compression method this build was not given, which is exactly
        // the member that has to survive being listed and copied. Nothing here
        // decompresses anything.
        let mut entries = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            let f = archive.by_index_raw(i)?;
            entries.push(Entry {
                name: f.name().to_owned(),
                raw: f.name_raw().to_owned(),
                symlink: f.is_symlink(),
            });
        }

        let meta_index = entries
            .iter()
            .position(|e| name::matches(&e.name, &e.raw, METADATA_MEMBER))
            .ok_or(Malformed::NoMetadataMember)?;

        // Buffering here is deliberate and is not the rule the payload lives
        // under: the metadata member is a small TOML document, and every
        // caller of this library wants all of it.
        let mut bytes = Vec::new();
        archive.by_index(meta_index)?.read_to_end(&mut bytes)?;

        let text = std::str::from_utf8(&bytes).map_err(|_| Malformed::MetadataNotUtf8)?;
        let doc: DocumentMut = text
            .parse()
            .map_err(|e: toml_edit::TomlError| Malformed::MetadataNotToml(e.to_string()))?;

        let version = required_string(&doc, VERSION_KEY)?.to_owned();
        let payload_file = required_string(&doc, PAYLOAD_FILE_KEY)?.to_owned();

        // Everything past this point is a rule stated by version 1.0 of the
        // specification. A container declaring a version this build does not
        // implement is parsed and reported and no further, because SPEC 3
        // forbids assuming its rules are the ones written here.
        let payload_index = if version == VERSION {
            Some(locate_payload(&entries, &payload_file)?)
        } else {
            None
        };

        Ok(Self {
            archive,
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

/// Read a required key that SPEC 2.2 says is a string.
fn required_string<'d>(doc: &'d DocumentMut, key: &'static str) -> Result<&'d str> {
    let item = key
        .split('.')
        .try_fold(doc.as_item(), |item, part| item.get(part))
        .ok_or(Malformed::MissingKey(key))?;
    item.as_str()
        .ok_or_else(|| Malformed::KeyNotAString(key).into())
}

/// Find the member `payload.file` names, and check it may be a payload.
fn locate_payload(entries: &[Entry], payload_file: &str) -> Result<usize> {
    name::check_payload_name(payload_file)?;

    // Two members may carry one name, and the specification says nothing about
    // which is the payload when they do. Taking the first is a choice this
    // implementation makes and has no standing to turn into a format rule, so
    // it is recorded here and raised upstream rather than settled quietly.
    let i = entries
        .iter()
        .position(|e| name::matches(&e.name, &e.raw, payload_file))
        .ok_or_else(|| Malformed::NoPayloadMember(payload_file.to_owned()))?;

    if entries[i].symlink {
        return Err(Error::Malformed(Malformed::PayloadIsSymlink(
            payload_file.to_owned(),
        )));
    }
    Ok(i)
}
