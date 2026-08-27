// What can go wrong, in the three families DESIGN.md 4.5 describes.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::fmt;

/// Why `payload.file` is not a name a payload may have.
///
/// Every variant is one bullet of SPEC 2.3. They are separate so a message can
/// say which rule was broken rather than restating the whole list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NameError {
    /// The name is empty.
    Empty,
    /// The name is `.` or `..`.
    Relative,
    /// The name contains a path separator.
    Separator(char),
    /// The name contains a colon, which some platforms read as rooting a path.
    Colon,
    /// The name contains a character in U+0000 to U+001F, or U+007F.
    ControlCharacter(char),
    /// The name is the metadata member's.
    ReservedForMetadata,
    /// The name is not UTF-8, so no TOML string can hold it.
    NotUtf8,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("payload.file is empty (SPEC 2.3)"),
            Self::Relative => f.write_str("payload.file is `.` or `..` (SPEC 2.3)"),
            Self::Separator(c) => write!(f, "payload.file contains {c:?}, so it is a path rather than a filename (SPEC 2.3)"),
            Self::Colon => f.write_str("payload.file contains ':', which is read as rooting a path on some platforms (SPEC 2.3)"),
            Self::ControlCharacter(c) => write!(f, "payload.file contains U+{:04X}, a control character (SPEC 2.3)", *c as u32),
            Self::ReservedForMetadata => write!(f, "payload.file is {:?}, which names the metadata member (SPEC 2.3)", crate::METADATA_MEMBER),
            Self::NotUtf8 => f.write_str("the name is not UTF-8, and payload.file is a TOML string (SPEC 2.2)"),
        }
    }
}

/// Why a byte stream is not a conformant container, or why the container asked
/// for could not be written.
///
/// Each variant names the rule it breaks. A caller that wants to explain itself
/// can print the variant and cite the clause it carries.
///
/// The write-side variants sit here rather than in a family of their own
/// because they say the same thing from the other direction: the container that
/// would come out is not one this library would accept back.
#[derive(Debug)]
#[non_exhaustive]
pub enum Malformed {
    /// Not a ZIP archive, or one whose central directory will not parse.
    NotAnArchive(String),
    /// No member named `slipcase.metadata.toml` (SPEC 2.1).
    NoMetadataMember,
    /// The metadata member is not UTF-8 (SPEC 2.2).
    MetadataNotUtf8,
    /// The metadata member is not a valid TOML document (SPEC 2.2).
    MetadataNotToml(String),
    /// A required key is absent (SPEC 2.2).
    MissingKey(&'static str),
    /// A required key is present but is not a string (SPEC 2.2).
    KeyNotAString(&'static str),
    /// `payload.file` is not a name a payload may have (SPEC 2.3).
    PayloadName(NameError),
    /// `payload.file` names no member of the archive (SPEC 2.1).
    NoPayloadMember(String),
    /// The payload member is not a regular file entry (SPEC 2.3).
    ///
    /// Directory entries, symbolic links, and every other entry type a ZIP
    /// implementation can record are excluded. An entry carrying no type
    /// information at all is taken to be a regular file, since there is nothing
    /// to say otherwise and ordinary archives are full of them.
    PayloadNotARegularFile {
        /// The member named by `payload.file`.
        name: String,
        /// What the archive says the entry is.
        kind: EntryKind,
    },
    /// More than one member is named `slipcase.metadata.toml` (SPEC 2.1).
    DuplicateMetadataMember(usize),
    /// More than one member's name equals `payload.file` (SPEC 2.1).
    ///
    /// Which one is the payload would depend on the order members sit in, and
    /// SPEC 3 forbids depending on that.
    DuplicatePayloadMember {
        /// The name they share.
        name: String,
        /// How many members carry it.
        count: usize,
    },
    /// The archive already holds a member under the name a payload is being
    /// written as (SPEC 2.1).
    ///
    /// Reached only by repacking, where the payload arrives with a name and the
    /// container already has one. Writing it anyway would leave two members
    /// carrying that name, and which of them was the payload would depend on
    /// the order they sit in.
    PayloadNameTaken(String),
    /// A file on disk is called something `payload.file` cannot express (SPEC 2.3).
    ///
    /// The payload is rejected rather than renamed. A container that cannot
    /// name its own payload is worse than a refusal to write one.
    PayloadPathName {
        /// The path that was handed in.
        path: std::path::PathBuf,
        /// Which rule its filename breaks.
        cause: NameError,
    },
    /// Metadata handed in contradicts what is being written (SPEC 2.2).
    ///
    /// The library sets both required keys itself, so a caller that also sets
    /// one has said two things. Which of them was meant is not recoverable, and
    /// overwriting silently would pick one without saying so.
    Disagrees {
        /// The key both sides set.
        key: &'static str,
        /// What the metadata handed in says.
        found: String,
        /// What is actually being written.
        writing: String,
    },
    /// `payload` is present in the metadata and is not a table (SPEC 2.2).
    PayloadNotATable,
}

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnArchive(why) => write!(f, "not a readable ZIP archive: {why}"),
            Self::NoMetadataMember => {
                write!(f, "no member named {:?} (SPEC 2.1)", crate::METADATA_MEMBER)
            }
            Self::MetadataNotUtf8 => {
                write!(f, "{:?} is not UTF-8 (SPEC 2.2)", crate::METADATA_MEMBER)
            }
            Self::MetadataNotToml(why) => write!(
                f,
                "{:?} is not a valid TOML document (SPEC 2.2): {why}",
                crate::METADATA_MEMBER
            ),
            Self::MissingKey(k) => write!(f, "the metadata has no `{k}` key (SPEC 2.2)"),
            Self::KeyNotAString(k) => write!(f, "the metadata's `{k}` is not a string (SPEC 2.2)"),
            Self::PayloadName(e) => e.fmt(f),
            Self::NoPayloadMember(n) => write!(
                f,
                "payload.file names {n:?}, which the archive does not contain (SPEC 2.1)"
            ),
            Self::PayloadNotARegularFile { name, kind } => write!(
                f,
                "the payload member {name:?} is {kind} rather than a regular file entry (SPEC 2.3)"
            ),
            Self::DuplicateMetadataMember(n) => write!(
                f,
                "{n} members are named {:?}; a container has exactly one (SPEC 2.1)",
                crate::METADATA_MEMBER
            ),
            Self::DuplicatePayloadMember { name, count } => write!(
                f,
                "{count} members are named {name:?}; a container has exactly one payload (SPEC 2.1)"
            ),
            Self::PayloadNameTaken(n) => write!(
                f,
                "the container already has a member named {n:?}, so writing the payload under that name would leave it with two (SPEC 2.1)"
            ),
            Self::PayloadPathName { path, cause } => write!(
                f,
                "{} cannot be packed under its own name: {cause}",
                path.display()
            ),
            Self::Disagrees { key, found, writing } => write!(
                f,
                "the metadata sets `{key}` to {found:?}, but this container is being written with {writing:?} (SPEC 2.2)"
            ),
            Self::PayloadNotATable => f.write_str("the metadata's `payload` is not a table (SPEC 2.2)"),
        }
    }
}

/// The container may well be conformant, and this build cannot handle it.
///
/// SPEC 2.5 forbids rejecting a container for any of these, so they are kept
/// apart from [`Malformed`]. Reporting one as the other would have this
/// implementation calling conformant containers broken.
#[derive(Debug)]
#[non_exhaustive]
pub enum Unsupported {
    /// A `slipcase_version` this build does not implement (SPEC 3).
    Version(String),
    /// A compression method this build was not compiled with (SPEC 2.5).
    Compression(u16),
    /// An encrypted member (SPEC 2.5).
    Encrypted,
    /// Something the ZIP crate declined to read and did not name further.
    Archive(String),
    /// The metadata member is larger than this reader's bound (SPEC 6).
    ///
    /// Undetermined and never non-conformant: the bound belongs to the reader,
    /// so two readers holding different ones must not disagree about whether
    /// the same file conforms. Raise it with [`Limits`](crate::Limits).
    MetadataTooLarge {
        /// The bound that was exceeded, in bytes.
        limit: u64,
        /// What the central directory recorded for the member.
        ///
        /// A hint and not a fact. Nothing checks it against what the member
        /// inflates to, so it is under `limit` whenever the directory
        /// understated the member and the bound caught it on the way past.
        declared: u64,
    },
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Version(v) => write!(
                f,
                "slipcase_version {v:?} is not one this build implements (SPEC 3)"
            ),
            Self::Compression(m) => write!(
                f,
                "compression method {m} is not compiled into this build (SPEC 2.5)"
            ),
            Self::Encrypted => f.write_str("the member is encrypted (SPEC 2.5)"),
            Self::Archive(why) => write!(f, "this build cannot read the archive: {why}"),
            // Two sentences for two situations, because a reader deciding
            // whether to raise the bound is helped by knowing which it met.
            Self::MetadataTooLarge { limit, declared } if declared > limit => write!(
                f,
                "the metadata member declares {declared} bytes, over this reader's limit of {limit} (SPEC 6)"
            ),
            Self::MetadataTooLarge { limit, declared } => write!(
                f,
                "the metadata member declares {declared} bytes and read past this reader's limit of {limit} (SPEC 6)"
            ),
        }
    }
}

/// Anything the library can fail with.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The bytes could not be read or written.
    Io(std::io::Error),
    /// This is not a conformant container.
    Malformed(Malformed),
    /// This is or may be a conformant container, and this build cannot handle it.
    Unsupported(Unsupported),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "i/o error: {e}"),
            Self::Malformed(e) => e.fmt(f),
            Self::Unsupported(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Malformed(_) | Self::Unsupported(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<Malformed> for Error {
    fn from(e: Malformed) -> Self {
        Self::Malformed(e)
    }
}

impl From<NameError> for Error {
    fn from(e: NameError) -> Self {
        Self::Malformed(Malformed::PayloadName(e))
    }
}

impl From<Unsupported> for Error {
    fn from(e: Unsupported) -> Self {
        Self::Unsupported(e)
    }
}

impl From<zip::result::ZipError> for Error {
    /// Sort the ZIP crate's one error type into the three families.
    ///
    /// The split that matters is `UnsupportedArchive`, which covers both an
    /// encrypted member and whatever else the crate declines to read. Both are
    /// [`Unsupported`], because SPEC 2.5 forbids calling either non-conformant.
    fn from(e: zip::result::ZipError) -> Self {
        use zip::result::ZipError as Z;
        match e {
            Z::Io(e) => Self::Io(e),
            Z::InvalidArchive(why) => Self::Malformed(Malformed::NotAnArchive(why.to_string())),
            Z::UnsupportedArchive(why) if why == Z::PASSWORD_REQUIRED => {
                Self::Unsupported(Unsupported::Encrypted)
            }
            Z::UnsupportedArchive(why) => Self::Unsupported(Unsupported::Archive(why.to_string())),
            Z::CompressionMethodNotSupported(m) => Self::Unsupported(Unsupported::Compression(m)),
            Z::InvalidPassword => Self::Unsupported(Unsupported::Encrypted),
            // Reached only by asking for a member index the archive does not have,
            // which this crate never does.
            Z::FileNotFound => Self::Malformed(Malformed::NotAnArchive(
                "a member named in the central directory is not there".into(),
            )),
            other => Self::Malformed(Malformed::NotAnArchive(other.to_string())),
        }
    }
}

/// The library's result type.
pub type Result<T> = std::result::Result<T, Error>;

/// What kind of entry an archive says a member is.
///
/// Taken from the file-type bits of the member's external attributes, which is
/// the only place a ZIP archive records this. An archive written on a system
/// with no such notion carries no bits, and [`EntryKind::Regular`] is the
/// answer for want of any other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EntryKind {
    /// An ordinary file, or an entry that does not say.
    Regular,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
    /// Something else the file-type bits name: a FIFO, a socket, a device.
    Other(u32),
}

impl EntryKind {
    /// Read the file-type bits of a unix mode.
    pub(crate) fn from_mode(mode: Option<u32>) -> Self {
        // The mask and values are POSIX's S_IFMT and friends. They are written
        // out rather than taken from libc because this crate has no C
        // dependency and these five numbers have not moved since the 1970s.
        const FMT: u32 = 0o170_000;
        match mode.map(|m| m & FMT) {
            None | Some(0 | 0o100_000) => Self::Regular,
            Some(0o040_000) => Self::Directory,
            Some(0o120_000) => Self::Symlink,
            Some(other) => Self::Other(other >> 12),
        }
    }
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Regular => f.write_str("a regular file"),
            Self::Directory => f.write_str("a directory entry"),
            Self::Symlink => f.write_str("a symbolic link entry"),
            Self::Other(t) => write!(f, "an entry of type {t:#o}"),
        }
    }
}
