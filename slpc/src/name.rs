// Member names: what `payload.file` may be, and when a decoded name can be
// trusted to be the member's name.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use crate::error::NameError;
use crate::METADATA_MEMBER;

/// Check a name against SPEC 2.3.
///
/// The specification requires rejecting a name that breaks these rules rather
/// than sanitizing it, so this returns the rule broken and never a repaired
/// name. A payload is one file and never a location in a tree, and a name that
/// cannot express a path cannot express a traversal.
pub fn check_payload_name(name: &str) -> Result<(), NameError> {
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameError::Relative);
    }
    if name.contains('/') {
        return Err(NameError::Separator('/'));
    }
    if name.contains('\\') {
        return Err(NameError::Separator('\\'));
    }
    if name.contains(':') {
        return Err(NameError::Colon);
    }
    if name == METADATA_MEMBER {
        return Err(NameError::ReservedForMetadata);
    }
    Ok(())
}

/// Whether a decoded member name can be trusted to be the member's name.
///
/// The ZIP crate decodes names as SPEC 2.1 requires, UTF-8 where general
/// purpose bit 11 is set and CP437 otherwise, but on the UTF-8 branch it
/// substitutes U+FFFD for bytes that are not valid UTF-8 rather than reporting
/// them, and it exposes neither the flag nor a way back to the reader to read
/// the flag word again.
///
/// The flag is recoverable by implication. CP437 decoding is total over all 256
/// bytes and produces U+FFFD for none of them, so a decoded name carrying one
/// over raw bytes that are not valid UTF-8 can only have come from the lossy
/// branch. That member's true name is not a Rust string, so it equals no
/// `payload.file`, which always is one.
///
/// Without the guard, two members can decode to a single name and a
/// `payload.file` carrying U+FFFD can match a member named something else,
/// which would leave the answer to depend on the order members happen to sit
/// in — the one thing SPEC 3 forbids depending on.
fn decoding_is_faithful(name: &str, raw: &[u8]) -> bool {
    !(name.contains('\u{fffd}') && std::str::from_utf8(raw).is_err())
}

/// Does this member's name equal `want`, decoded as SPEC 2.1 requires?
pub(crate) fn matches(name: &str, raw: &[u8], want: &str) -> bool {
    decoding_is_faithful(name, raw) && name == want
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_plain_filename() {
        for n in [
            "report.pdf",
            "a",
            "..leading",
            "trailing..",
            "wide 名前.txt",
            "-",
        ] {
            assert_eq!(check_payload_name(n), Ok(()), "{n:?}");
        }
    }

    #[test]
    fn rejects_each_rule_in_2_3() {
        assert_eq!(check_payload_name(""), Err(NameError::Empty));
        assert_eq!(check_payload_name("."), Err(NameError::Relative));
        assert_eq!(check_payload_name(".."), Err(NameError::Relative));
        assert_eq!(check_payload_name("a/b"), Err(NameError::Separator('/')));
        assert_eq!(check_payload_name("../b"), Err(NameError::Separator('/')));
        assert_eq!(check_payload_name("a\\b"), Err(NameError::Separator('\\')));
        assert_eq!(check_payload_name("C:file"), Err(NameError::Colon));
        assert_eq!(
            check_payload_name(METADATA_MEMBER),
            Err(NameError::ReservedForMetadata)
        );
    }

    #[test]
    fn an_honest_name_matches_itself() {
        assert!(matches("report.pdf", b"report.pdf", "report.pdf"));
        // A CP437 name: the raw bytes are not UTF-8, and nothing was replaced.
        assert!(matches("caf\u{e7}.txt", b"caf\x87.txt", "caf\u{e7}.txt"));
    }

    #[test]
    fn a_lossily_decoded_name_matches_nothing() {
        // Bit 11 set over bytes that are not UTF-8: the crate hands back U+FFFD.
        let lossy = "caf\u{fffd}.txt";
        assert!(!matches(lossy, b"caf\xff.txt", lossy));
    }

    #[test]
    fn the_guard_does_not_catch_an_honest_replacement_character() {
        // U+FFFD is a character like any other, and a member may legitimately be
        // named with one. The raw bytes are valid UTF-8, so the name is faithful.
        let name = "caf\u{fffd}.txt";
        assert!(matches(name, name.as_bytes(), name));
    }
}
