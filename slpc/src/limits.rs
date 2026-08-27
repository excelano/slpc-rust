// What a reader is willing to spend before it knows what it is holding.
//
// SPEC 6 requires a bound on the metadata member and names no number, because
// the number belongs to whoever is doing the reading. This is where a caller
// says theirs, and where the default this crate picks when nobody says is
// written down.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

/// Bounds a reader applies to a container before it trusts anything in it.
///
/// SPEC 6 exists because identifying a container is a parse of untrusted input:
/// a reader must decompress the metadata member and parse it as TOML before it
/// knows whether the file was a container at all, which is not the position of
/// a general ZIP consumer, who chooses what to extract.
///
/// Pass one to [`Container::read_with`](crate::Container::read_with),
/// [`Container::open_with`](crate::Container::open_with),
/// [`validate_with`](crate::validate_with) or
/// [`metadata_of_with`](crate::metadata_of_with). The unsuffixed forms of all
/// four use [`Limits::default`].
///
/// Construct with [`Limits::default`] and adjust the fields; the struct is
/// `#[non_exhaustive]` so that a later bound can be added without breaking a
/// caller.
///
/// ```
/// let mut limits = slpc::Limits::default();
/// limits.metadata_bytes = 1 << 20;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// The most the metadata member may decompress to, in bytes.
    ///
    /// A container whose metadata member exceeds this is reported
    /// [`Undetermined`](crate::Verdict::Undetermined) and never
    /// non-conformant, because the bound is the reader's and not a property of
    /// the file. Two readers with different bounds must not disagree about
    /// conformance.
    pub metadata_bytes: u64,
}

impl Limits {
    /// The default bound on the metadata member: 16 MiB.
    ///
    /// Chosen against both ends of what SPEC 2.2 permits. The two keys the
    /// format defines occupy a few dozen bytes, and a document holding what
    /// §2.2 also permits — an extracted text layer, an embedded thumbnail, a
    /// provenance chain — can plausibly reach a megabyte or two, so a bound
    /// near that would refuse legitimate containers. At the other end this is
    /// what a hostile container can make a reader allocate, and 16 MiB of TOML
    /// costs a parsed document several times that, which is survivable on
    /// anything this crate is likely to run on.
    ///
    /// It is a default and not a recommendation. A reader invoked
    /// automatically over a directory should set it far lower; one being
    /// handed a single file by somebody who chose it can afford more.
    pub const DEFAULT_METADATA_BYTES: u64 = 16 << 20;
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            metadata_bytes: Self::DEFAULT_METADATA_BYTES,
        }
    }
}
