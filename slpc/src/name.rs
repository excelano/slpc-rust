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

/// A member name in a form safe to put in front of a person.
///
/// SPEC 3 requires that the Unicode bidirectional formatting characters be
/// rendered escaped rather than applied wherever `payload.file` or a member
/// name is displayed. This is that rendering: each one comes back spelled out
/// as `\u{202E}`, everything else is untouched, and a name carrying none is
/// returned borrowed and unchanged.
///
/// SPEC 2.3 lets these through deliberately and DESIGN says why: they are legal
/// on every filesystem, a writer may hold a file that genuinely has one in its
/// name, and putting them in the name rules would turn a rule about paths into
/// a table of special cases. The container is conformant; the problem is the
/// display, which is where this belongs.
///
/// U+202E RIGHT-TO-LEFT OVERRIDE is the one worth naming. A payload called
/// `report<U+202E>fdp.exe` reads as `report.pdf` wherever the override is
/// applied, beside a button that will hand the file to whatever the system has
/// registered for `.exe`.
///
/// **Escaping rather than isolating.** Wrapping the name in U+2066 and U+2069
/// confines the reordering to one field, and a name that reads as `report.pdf`
/// inside its own field is still a name that reads as `report.pdf`. An override
/// with no terminator also runs to the end of the paragraph rather than the end
/// of the string, so anything relying on containment has to emit the terminator
/// itself and cannot trust the name to carry one.
///
/// The C0 controls and U+007F are not here: SPEC 2.3 excludes those from
/// `payload.file` outright, so a conformant container has none to display.
///
/// ```
/// assert_eq!(slpc::display_name("report.pdf"), "report.pdf");
/// assert_eq!(
///     slpc::display_name("report\u{202E}fdp.exe"),
///     "report\\u{202E}fdp.exe"
/// );
/// ```
#[must_use]
pub fn display_name(name: &str) -> std::borrow::Cow<'_, str> {
    use std::fmt::Write as _;

    if !name.chars().any(is_bidi_formatting) {
        return std::borrow::Cow::Borrowed(name);
    }
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if is_bidi_formatting(c) {
            // Infallible into a String; the result is discarded rather than
            // unwrapped so that this cannot panic on a name.
            let _ = write!(out, "\\u{{{:04X}}}", c as u32);
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// The characters SPEC 3 names, and no others.
///
/// U+061C ARABIC LETTER MARK is the one to watch: it is the only one of the
/// seven outside the U+200x and U+206x blocks, and the one a range check
/// written from memory leaves out.
fn is_bidi_formatting(c: char) -> bool {
    matches!(c,
        '\u{061C}'                  // ARABIC LETTER MARK
        | '\u{200E}'..='\u{200F}'   // LEFT-TO-RIGHT MARK, RIGHT-TO-LEFT MARK
        | '\u{202A}'..='\u{202E}'   // the embeddings, the pop, the overrides
        | '\u{2066}'..='\u{2069}'   // the isolates and their pop
    )
}

#[cfg(test)]
mod tests {

    /// Every character SPEC 3 names is escaped, and nothing else is touched.
    ///
    /// Catches a range check written from memory. U+061C ARABIC LETTER MARK is
    /// the one outside the U+200x and U+206x blocks, and U+2069 and U+202E are
    /// the ends of their ranges, which an off-by-one drops.
    #[test]
    fn every_bidi_formatting_character_is_escaped() {
        for c in [
            '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
            '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            let name = format!("a{c}b");
            assert_eq!(
                display_name(&name),
                format!("a\\u{{{:04X}}}b", c as u32),
                "U+{:04X} was not escaped",
                c as u32
            );
        }
    }

    /// The characters either side of each range are left alone.
    ///
    /// Catches a range that reaches too far and mangles ordinary names. U+200D
    /// ZERO WIDTH JOINER and U+2060 WORD JOINER are invisible too and are
    /// deliberately not here: SPEC 3 names the bidirectional set, and a rule
    /// that quietly grew to every format character would be this crate's
    /// opinion rather than the specification's.
    #[test]
    fn neighbouring_characters_are_left_alone() {
        for c in [
            '\u{061B}', '\u{061D}', '\u{200D}', '\u{2010}', '\u{2029}', '\u{202F}', '\u{2060}',
            '\u{2065}', '\u{206A}', 'a', '.', 'é', '日',
        ] {
            let name = format!("a{c}b");
            assert_eq!(display_name(&name), name, "U+{:04X} was changed", c as u32);
        }
    }

    /// A name with nothing to escape is returned borrowed.
    ///
    /// Catches an implementation that allocates for every name it is shown.
    /// This runs on every payload card and every line of CLI output, and almost
    /// every name has nothing in it.
    #[test]
    fn an_ordinary_name_is_not_copied() {
        assert!(matches!(
            display_name("Q3 report final.pdf"),
            std::borrow::Cow::Borrowed(_)
        ));
        assert!(matches!(
            display_name("report\u{202E}fdp.exe"),
            std::borrow::Cow::Owned(_)
        ));
    }

    /// The escaped form does not read as the name it was hiding.
    ///
    /// The point of the whole exercise, stated as an assertion: a payload
    /// called `report<U+202E>fdp.exe` reads as `report.pdf` wherever the
    /// override is applied, and what comes out of here must still end in
    /// `.exe` however it is rendered, because nothing left in it reorders
    /// anything.
    #[test]
    fn an_override_no_longer_hides_the_extension() {
        let shown = display_name("report\u{202E}fdp.exe");
        assert!(shown.ends_with(".exe"), "{shown}");
        assert!(!shown.chars().any(is_bidi_formatting), "{shown}");
    }

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
