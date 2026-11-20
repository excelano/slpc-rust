// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::error::Error as _;
use std::path::PathBuf;

use slpc::{EntryKind, Error, Malformed, MemberError, MemberNameError, NameError, Unsupported};

fn names() -> Vec<NameError> {
    vec![
        NameError::Empty,
        NameError::Relative,
        NameError::Separator('/'),
        NameError::Colon,
        NameError::ControlCharacter('\u{7}'),
        NameError::ReservedForFlyleaf,
        NameError::NotUtf8,
    ]
}

fn malformed() -> Vec<Malformed> {
    let mut all = vec![
        Malformed::NoFlyleafMember,
        Malformed::FlyleafNotUtf8,
        Malformed::FlyleafNotToml("expected `]`".into()),
        Malformed::MissingKey("slipcase_version"),
        Malformed::KeyNotAString("content.file"),
        Malformed::NoContentMember("a.pdf".into()),
        Malformed::ContentNotARegularFile {
            name: "a.pdf".into(),
            kind: EntryKind::Symlink,
        },
        Malformed::DuplicateFlyleafMember(2),
        Malformed::DuplicateContentMember {
            name: "a.pdf".into(),
            count: 3,
        },
        Malformed::ContentNameTaken("a.pdf".into()),
        Malformed::ContentPathName {
            path: PathBuf::from("a:b.pdf"),
            cause: NameError::Colon,
        },
        Malformed::Disagrees {
            key: "content.file",
            found: "a.pdf".into(),
            writing: "b.pdf".into(),
        },
        Malformed::ContentNotATable,
    ];
    all.extend(names().into_iter().map(Malformed::ContentName));
    all
}

#[test]
fn every_rule_a_container_breaks_cites_the_section_that_states_it() {
    for m in malformed() {
        let text = m.to_string();
        assert!(text.contains("(SPEC 2."), "{text}");
    }
    for n in [
        MemberNameError::EmptySegment,
        MemberNameError::RelativeSegment,
        MemberNameError::Backslash,
        MemberNameError::Colon,
        MemberNameError::ControlCharacter('\u{1b}'),
        MemberNameError::Undecodable,
    ] {
        let text = n.to_string();
        assert!(text.contains("(SPEC "), "{text}");
    }
}

#[test]
fn values_are_written_into_the_message_as_they_were_found() {
    assert_eq!(
        NameError::ControlCharacter('\u{7}').to_string(),
        "content.file contains U+0007, a control character (SPEC 2.3)"
    );
    assert_eq!(
        Malformed::ContentNotARegularFile {
            name: "a.pdf".into(),
            kind: EntryKind::Other(0o060_000),
        }
        .to_string(),
        "the content member \"a.pdf\" is an entry of type 0o60000 rather than a regular file entry (SPEC 2.3)"
    );
    assert_eq!(
        Malformed::ContentPathName {
            path: PathBuf::from("a:b.pdf"),
            cause: NameError::Colon,
        }
        .to_string(),
        "a:b.pdf cannot be packed under its own name: content.file contains ':', which is read as rooting a path on some platforms (SPEC 2.3)"
    );
    assert_eq!(
        MemberError::Ambiguous {
            name: "a.txt".into(),
            count: 2,
        }
        .to_string(),
        "2 members are named \"a.txt\", so which one is meant cannot be told (SPEC 3)"
    );
    assert_eq!(
        MemberError::Name {
            name: "a\\b".into(),
            cause: MemberNameError::Backslash,
        }
        .to_string(),
        "\"a\\\\b\" cannot be a member's name: it contains '\\' (SPEC 3)"
    );
    assert_eq!(
        MemberError::DirectoryClash {
            file: "a".into(),
            under: "a/b".into(),
        }
        .to_string(),
        "\"a\" would be written as a file, and \"a/b\" needs it to be a directory (SPEC 3)"
    );
}

#[test]
fn a_flyleaf_over_the_bound_says_whether_its_directory_said_so() {
    let declared_over = Unsupported::FlyleafTooLarge {
        limit: 100,
        declared: 200,
    };
    assert!(declared_over
        .to_string()
        .contains("declares 200 bytes, over"));
    let read_past = Unsupported::FlyleafTooLarge {
        limit: 100,
        declared: 50,
    };
    assert!(read_past
        .to_string()
        .contains("declares 50 bytes and read past"));
    assert_eq!(
        Unsupported::Archive("multi-disk".into()).to_string(),
        "this build cannot read the archive: multi-disk"
    );
}

#[test]
fn only_an_io_error_has_a_source_beneath_it() {
    let io = Error::from(std::io::Error::other("disk full"));
    assert_eq!(io.to_string(), "i/o error: disk full");
    assert!(io.source().is_some());
    assert!(Error::from(Malformed::NoFlyleafMember).source().is_none());
    assert!(Error::from(Unsupported::Encrypted).source().is_none());
    assert_eq!(
        [EntryKind::Regular, EntryKind::Directory, EntryKind::Symlink].map(|k| k.to_string()),
        [
            "a regular file",
            "a directory entry",
            "a symbolic link entry"
        ]
    );
}
