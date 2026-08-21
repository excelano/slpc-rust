//! <!-- This crate's documentation is its README, so the two cannot disagree.
//! The examples below are compiled and run by `cargo test` wherever they
//! render: on docs.rs, on crates.io, and in the repository. -->
#![doc = include_str!("../README.md")]
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)
#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

mod central;
mod container;
#[cfg(feature = "fs")]
mod dest;
mod error;
mod metadata;
mod name;
mod write;

/// The TOML implementation this crate is built on, re-exported.
///
/// [`DocumentMut`](toml_edit::DocumentMut) appears in this crate's signatures,
/// so a caller needs the same version of it. Taking it from here rather than
/// from a dependency of their own is what stops the two from skewing.
pub use toml_edit;

pub use container::Container;
#[cfg(feature = "fs")]
pub use dest::Destination;
pub use error::{EntryKind, Error, Malformed, NameError, Result, Unsupported};
pub use name::check_payload_name;
pub use write::{pack_file, pack_reader, rewrite_metadata, rewrite_metadata_bytes, Repack};

/// The archive member holding the metadata (SPEC 2.1).
pub const METADATA_MEMBER: &str = "slipcase.metadata.toml";

/// The version of the specification this build implements.
pub const VERSION: &str = "1.0";

/// The metadata key naming the specification version (SPEC 2.2).
pub const VERSION_KEY: &str = "slipcase_version";

/// The metadata key naming the payload member (SPEC 2.2).
pub const PAYLOAD_FILE_KEY: &str = "payload.file";

/// What can be said about a container after reading it.
///
/// Four answers rather than two. SPEC 2.2 and SPEC 3 require that a container
/// whose metadata member cannot be read is reported as neither conformant nor
/// non-conformant, and SPEC 2.4 puts a container declaring another version
/// outside this document's conformance question rather than failing it. A
/// yes-or-no return could say neither thing.
#[derive(Debug)]
#[non_exhaustive]
pub enum Verdict {
    /// Conformant to the version this build implements.
    Conformant,
    /// Not conformant, and this is the rule it breaks.
    NonConformant(Malformed),
    /// The metadata member could not be read, so conformance cannot be
    /// established from the file (SPEC 2.2). Not a failure, and not a pass.
    Undetermined(Unsupported),
    /// Declares a `slipcase_version` this build does not implement (SPEC 2.4).
    /// Outside the question rather than failing it.
    OutOfScope(String),
}

impl Verdict {
    /// Whether this is [`Verdict::Conformant`].
    ///
    /// Deliberately not `is_ok`: the other three are not failures in the same
    /// sense, and two of them are not failures at all.
    #[must_use]
    pub fn is_conformant(&self) -> bool {
        matches!(self, Self::Conformant)
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conformant => f.write_str("conformant"),
            Self::NonConformant(m) => write!(f, "not conformant: {m}"),
            Self::Undetermined(u) => write!(f, "conformance cannot be established: {u}"),
            Self::OutOfScope(v) => write!(
                f,
                "declares slipcase_version {v:?}, which this build does not implement, so it says nothing about conformance"
            ),
        }
    }
}

/// Report what can be said about a byte stream as a container.
///
/// Reads the central directory and the metadata member. It confirms that
/// exactly one member matches `payload.file` and that the member is a regular
/// file entry, and it never decompresses the payload, so a container whose
/// payload uses a compression method this build lacks still validates.
///
/// The `Err` this returns is always [`Error::Io`]: not being able to read the
/// bytes at all is a fact about the reader rather than about the container.
/// Everything the container itself can be is a [`Verdict`].
pub fn validate<R: std::io::Read + std::io::Seek>(reader: R) -> Result<Verdict> {
    match Container::read(reader) {
        Ok(c) if c.version() == VERSION => Ok(Verdict::Conformant),
        Ok(c) => Ok(Verdict::OutOfScope(c.version().to_owned())),
        Err(Error::Malformed(m)) => Ok(Verdict::NonConformant(m)),
        Err(Error::Unsupported(u)) => Ok(Verdict::Undetermined(u)),
        Err(e) => Err(e),
    }
}
