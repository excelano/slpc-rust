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
pub use error::{Error, Malformed, NameError, Result, Unsupported};
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

/// Report whether a byte stream is a conformant container.
///
/// Reads the central directory and the metadata member. It confirms that a
/// member matching `payload.file` is present and is not a symbolic link entry,
/// and it never decompresses the payload, so a container whose payload uses a
/// compression method this build lacks still validates.
pub fn validate<R: std::io::Read + std::io::Seek>(reader: R) -> Result<()> {
    Container::read(reader).map(|_| ())
}
