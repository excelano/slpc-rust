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
    /// The default bound on the metadata member: 1 MiB.
    ///
    /// **This is a bound on the member, and the member is not what costs.** A
    /// parsed document costs far more than the bytes it came from, because
    /// `toml_edit` keeps a key's decor, span and representation alongside its
    /// value so that a rewrite can put back what it did not touch. Measured on
    /// 2026-08-27 against the densest conformant shape — shortest legal keys,
    /// shortest legal values — 256 KiB of metadata parses to 22 MB resident and
    /// 1 MiB to 85 MB. Around 85 times, and the multiplier is a function of how
    /// many keys fit rather than of the size, so the dense shape is the one to
    /// choose the number against.
    ///
    /// 16 MiB was this constant for one afternoon, on the estimate that a
    /// parsed document costs *several times* its source. That was wrong by
    /// more than an order of magnitude: 16 MiB is about 1.4 GB parsed, and in
    /// a viewer that renders the document it is several times that again.
    ///
    /// 1 MiB against the other end. The format defines two keys and SPEC 2.2's
    /// example is four lines; a document carrying what §2.2 also permits — an
    /// extracted text layer, an embedded thumbnail — reaches tens or hundreds
    /// of kilobytes rather than megabytes, and the largest container in the
    /// conformance corpus holds 64 KiB of metadata. A megabyte is generous
    /// against every legitimate document anyone has produced and costs 85 MB
    /// against the worst one anyone can write.
    ///
    /// It is a default and not a recommendation, which is what [`Limits`] is
    /// for. Anything that also renders the document should set it lower —
    /// `slipcase-desktop` uses 256 KiB, having measured what a tree of that
    /// many rows costs — and a reader invoked automatically over a directory
    /// lower still.
    pub const DEFAULT_METADATA_BYTES: u64 = 1 << 20;
}

impl Limits {
    /// Every bound at its default, as a constant.
    ///
    /// [`Default::default`] is not `const`, and the struct is
    /// `#[non_exhaustive]` so a caller outside this crate cannot write the
    /// literal. Without this there is no way to say *the defaults, with this one
    /// changed* in a `const`, which is where a caller with a considered bound
    /// wants to put it.
    ///
    /// ```
    /// const LIMITS: slpc::Limits = {
    ///     let mut l = slpc::Limits::DEFAULT;
    ///     l.metadata_bytes = 256 << 10;
    ///     l
    /// };
    /// ```
    pub const DEFAULT: Self = Self {
        metadata_bytes: Self::DEFAULT_METADATA_BYTES,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}
