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
    /// The name is the metadata member's.
    ReservedForMetadata,
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("payload.file is empty (SPEC 2.3)"),
            Self::Relative => f.write_str("payload.file is `.` or `..` (SPEC 2.3)"),
            Self::Separator(c) => write!(f, "payload.file contains {c:?}, so it is a path rather than a filename (SPEC 2.3)"),
            Self::Colon => f.write_str("payload.file contains ':', which is read as rooting a path on some platforms (SPEC 2.3)"),
            Self::ReservedForMetadata => write!(f, "payload.file is {:?}, which names the metadata member (SPEC 2.3)", crate::METADATA_MEMBER),
        }
    }
}

/// Why a byte stream is not a conformant container.
///
/// Each variant names the rule it breaks. A caller that wants to explain itself
/// can print the variant and cite the clause it carries.
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
    /// The payload member is a symbolic link entry (SPEC 2.3).
    PayloadIsSymlink(String),
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
            Self::PayloadIsSymlink(n) => write!(
                f,
                "the payload member {n:?} is a symbolic link entry (SPEC 2.3)"
            ),
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
