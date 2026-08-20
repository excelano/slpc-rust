//! The Rust implementation of the [slipcase](https://github.com/excelano/slipcase)
//! container format.
//!
//! A container is a ZIP archive binding a single payload file to a TOML
//! document describing it, so that the two travel as one file.
//!
//! ```no_run
//! let mut c = slpc::Container::open("report.pdf.slpc")?;
//! println!("{} holds {}", c.version(), c.payload_name());
//! let mut payload = c.payload()?;
//! std::io::copy(&mut payload, &mut std::io::stdout())?;
//! # Ok::<(), slpc::Error>(())
//! ```
//!
//! The specification lives in `excelano/slipcase` and is the authority on the
//! format. This crate implements it and has no standing to change it.
//!
//! Writing is four free functions, because none of them takes or returns a
//! container:
//!
//! ```no_run
//! use toml_edit::DocumentMut;
//! slpc::pack_file("report.pdf", DocumentMut::new(), std::fs::File::create("report.pdf.slpc")?)?;
//! # Ok::<(), slpc::Error>(())
//! ```
//!
//! The metadata argument is anything convertible into a `DocumentMut`, which is
//! a document, a table, or, with `toml_edit`'s `serde` feature turned on by the
//! caller, whatever `toml_edit::ser::to_document` makes of a struct or a map.
//! Building metadata from nothing has no formatting to preserve, so there is
//! nothing for that conversion to lose.
//!
//! # No vocabulary
//!
//! The two structural keys have typed accessors. Every other key is passed
//! through unexamined, because there is nothing to examine it against.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::missing_errors_doc)]

mod central;
mod container;
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
pub use error::{EntryKind, Error, Malformed, NameError, Result, Unsupported};
pub use name::check_payload_name;
pub use write::{pack_file, pack_reader, rewrite_metadata, rewrite_metadata_bytes};

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
