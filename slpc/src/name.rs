// Member names: what `payload.file` may be.
//
// Deciding whether a member's name equals another is a different question and
// lives in `central.rs`, with the flag that decodes it.
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
    // U+0000 to U+001F and U+007F. `char::is_control` is broader than that — it
    // covers the C1 range too — so it is narrowed to ASCII, which is what the
    // specification excludes.
    if let Some(c) = name.chars().find(|c| c.is_ascii() && c.is_control()) {
        return Err(NameError::ControlCharacter(c));
    }
    if name == METADATA_MEMBER {
        return Err(NameError::ReservedForMetadata);
    }
    Ok(())
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
    fn a_double_dot_is_only_excluded_as_the_whole_name() {
        // SPEC 2.3 excludes the names `.` and `..`, not the sequence wherever it
        // appears. A name that cannot express a path cannot express a traversal,
        // so `a..b` needs no defending against.
        for n in ["a..b", "..leading", "trailing..", "...."] {
            assert_eq!(check_payload_name(n), Ok(()), "{n:?}");
        }
    }

    #[test]
    fn rejects_every_control_character() {
        for c in (0u8..=0x1f).chain(std::iter::once(0x7f)) {
            let name = format!("rep{}ort.pdf", c as char);
            assert_eq!(
                check_payload_name(&name),
                Err(NameError::ControlCharacter(c as char)),
                "U+{c:04X}"
            );
        }
        // A character that merely looks exotic is not a control character.
        assert_eq!(check_payload_name("rep\u{200b}ort.pdf"), Ok(()));
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
}
