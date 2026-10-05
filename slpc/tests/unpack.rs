// Extracting a container into a directory: the `fs` feature's `Unpack`.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![cfg(feature = "fs")]

mod support;

use std::path::{Path, PathBuf};

use support::{flyleaf, open, raw_zip, Member};

use slpc::{Error, MemberError, MemberNameError, Unpack, Unsupported, FLYLEAF_MEMBER};

fn sandbox() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// Every file and directory under `dir`, relative and sorted, so a leftover is
/// visible.
fn tree(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, at: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(at).unwrap() {
            let p = e.unwrap().path();
            out.push(
                p.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
            if std::fs::symlink_metadata(&p).unwrap().is_dir() {
                walk(root, &p, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Paths resolved to the files they name, so the verbatim form `Unpack`
/// returns on Windows compares equal to a path built by hand.
fn resolved(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().map(|p| std::fs::canonicalize(p).unwrap()).collect()
}

fn with_members(extra: Vec<Member>) -> Vec<u8> {
    let mut all = vec![
        Member::new(FLYLEAF_MEMBER, flyleaf("a.txt").as_bytes()),
        Member::new("a.txt", b"content\n"),
    ];
    all.extend(extra);
    raw_zip(&all)
}

fn extras() -> Vec<u8> {
    with_members(vec![
        Member::new("notes.md", b"notes\n"),
        Member::new("records/", b"").with_mode(0o040_755),
        Member::new("records/events.toml", b"[[event]]\n"),
    ])
}

fn unpack(bytes: &[u8], dir: &Path, u: fn(Unpack) -> Unpack) -> slpc::Result<Vec<PathBuf>> {
    u(Unpack::new(dir)).write(&mut open(bytes).unwrap())
}

/// A refused request, and the proof that it left nothing behind.
fn refused(bytes: &[u8], u: fn(Unpack) -> Unpack) -> Error {
    let s = sandbox();
    let e = unpack(bytes, s.path(), u).unwrap_err();
    assert_eq!(
        tree(s.path()),
        Vec::<String>::new(),
        "a refused request wrote: {e}"
    );
    e
}

#[test]
fn writes_the_content_file_and_nothing_else_by_default() {
    let s = sandbox();
    let written = unpack(&extras(), s.path(), |u| u).unwrap();
    assert_eq!(tree(s.path()), ["a.txt"]);
    assert_eq!(resolved(&written), resolved(&[s.path().join("a.txt")]));
    assert_eq!(std::fs::read(s.path().join("a.txt")).unwrap(), b"content\n");
}

#[test]
fn writes_the_flyleaf_and_named_members() {
    let s = sandbox();
    let written = unpack(&extras(), s.path(), |u| {
        u.flyleaf().member("records/events.toml")
    })
    .unwrap();
    assert_eq!(
        tree(s.path()),
        ["a.txt", "records", "records/events.toml", FLYLEAF_MEMBER]
    );
    assert_eq!(
        resolved(&written),
        resolved(&[
            s.path().join(FLYLEAF_MEMBER),
            s.path().join("a.txt"),
            s.path().join("records").join("events.toml"),
        ])
    );
    assert_eq!(
        std::fs::read(s.path().join("records/events.toml")).unwrap(),
        b"[[event]]\n"
    );
}

#[test]
fn writes_every_member_when_asked_and_passes_over_directory_entries() {
    let s = sandbox();
    unpack(&extras(), s.path(), |u| u.all_members()).unwrap();
    assert_eq!(
        tree(s.path()),
        ["a.txt", "notes.md", "records", "records/events.toml"]
    );
}

#[test]
fn a_directory_entry_asked_for_by_name_is_passed_over() {
    let s = sandbox();
    unpack(&extras(), s.path(), |u| u.member("records/")).unwrap();
    assert_eq!(tree(s.path()), ["a.txt"]);
}

#[test]
fn refuses_what_it_cannot_identify() {
    assert!(matches!(
        refused(&extras(), |u| u.member("missing.md")),
        Error::Member(MemberError::Missing(_))
    ));
    assert!(matches!(
        refused(&extras(), |u| u.member("notes.md").member("notes.md")),
        Error::Member(MemberError::RequestedTwice(_))
    ));
    assert!(matches!(
        refused(&extras(), |u| u.member(FLYLEAF_MEMBER)),
        Error::Member(MemberError::Reserved(_))
    ));
    assert!(matches!(
        refused(&extras(), |u| u.member("a.txt")),
        Error::Member(MemberError::Reserved(_))
    ));

    let twice = with_members(vec![Member::new("n.md", b"1"), Member::new("n.md", b"2")]);
    for u in [|u: Unpack| u.member("n.md"), |u: Unpack| u.all_members()] {
        match refused(&twice, u) {
            Error::Member(MemberError::Ambiguous { name, count }) => {
                assert_eq!((name.as_str(), count), ("n.md", 2));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }
}

#[test]
fn refuses_a_member_that_is_not_a_regular_file() {
    let bytes = with_members(vec![Member::new("link", b"/etc/passwd").symlink()]);
    assert!(matches!(
        refused(&bytes, |u| u.all_members()),
        Error::Member(MemberError::NotARegularFile { .. })
    ));
}

#[test]
fn refuses_a_name_the_rule_forbids_rather_than_sanitizing_it() {
    for (raw, cause) in [
        (&b"../escape"[..], MemberNameError::RelativeSegment),
        (b"/rooted", MemberNameError::EmptySegment),
        (b"a\\b", MemberNameError::Backslash),
        (b"C:x", MemberNameError::Colon),
        (b"bell\x07", MemberNameError::ControlCharacter('\u{7}')),
    ] {
        let bytes = with_members(vec![Member::named_raw(raw, b"x")]);
        match refused(&bytes, |u| u.all_members()) {
            Error::Member(MemberError::Name { cause: c, .. }) => assert_eq!(c, cause),
            other => panic!("{raw:?}: expected Name, got {other:?}"),
        }
    }
    let bytes = with_members(vec![Member::named_raw(b"caf\xff.txt", b"x").flagged_utf8()]);
    match refused(&bytes, |u| u.all_members()) {
        Error::Member(MemberError::Name { cause, .. }) => {
            assert_eq!(cause, MemberNameError::Undecodable);
        }
        other => panic!("expected Undecodable, got {other:?}"),
    }
}

#[test]
fn refuses_a_member_this_build_cannot_decode_before_writing_anything() {
    let sealed = with_members(vec![Member::new("sealed.bin", b"x").encrypted()]);
    assert!(matches!(
        refused(&sealed, |u| u.all_members()),
        Error::Unsupported(Unsupported::Encrypted)
    ));
    let opaque = with_members(vec![Member::new("opaque.bin", b"x").claims_method(12)]);
    assert!(matches!(
        refused(&opaque, |u| u.all_members()),
        Error::Unsupported(Unsupported::Compression(12))
    ));
}

#[test]
fn refuses_a_file_another_name_needs_as_a_directory() {
    let bytes = with_members(vec![Member::new("x", b"1"), Member::new("x/y", b"2")]);
    match refused(&bytes, |u| u.all_members()) {
        Error::Member(MemberError::DirectoryClash { file, under }) => {
            assert_eq!((file.as_str(), under.as_str()), ("x", "x/y"));
        }
        other => panic!("expected DirectoryClash, got {other:?}"),
    }
    let bytes = with_members(vec![Member::new("a.txt/y", b"2")]);
    assert!(matches!(
        refused(&bytes, |u| u.all_members()),
        Error::Member(MemberError::DirectoryClash { .. })
    ));
}

#[test]
fn replaces_nothing_without_force() {
    let s = sandbox();
    std::fs::write(s.path().join("notes.md"), b"mine\n").unwrap();
    let e = unpack(&extras(), s.path(), |u| u.member("notes.md")).unwrap_err();
    assert!(
        matches!(&e, Error::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists),
        "{e:?}"
    );
    assert_eq!(std::fs::read(s.path().join("notes.md")).unwrap(), b"mine\n");
    assert_eq!(
        tree(s.path()),
        ["notes.md"],
        "the content file was left behind"
    );

    unpack(&extras(), s.path(), |u| u.member("notes.md").force(true)).unwrap();
    assert_eq!(
        std::fs::read(s.path().join("notes.md")).unwrap(),
        b"notes\n"
    );
}

#[test]
#[cfg(unix)]
fn follows_no_link_planted_in_the_destination() {
    let s = sandbox();
    let elsewhere = sandbox();
    std::os::unix::fs::symlink(elsewhere.path(), s.path().join("records")).unwrap();

    let e = unpack(&extras(), s.path(), |u| u.member("records/events.toml")).unwrap_err();
    assert!(e.to_string().contains("symbolic link"), "{e}");
    assert_eq!(
        tree(elsewhere.path()),
        Vec::<String>::new(),
        "it wrote through the link"
    );
    assert_eq!(
        tree(s.path()),
        ["records"],
        "the content file was left behind"
    );
}

#[test]
#[cfg(unix)]
fn removes_what_it_created_when_a_write_fails_partway() {
    let bytes = with_members(vec![
        Member::new("dir/one.txt", b"1"),
        Member::new("two.txt", b"2"),
    ]);
    let s = sandbox();
    std::os::unix::fs::symlink("/nonexistent/nowhere", s.path().join("two.txt")).unwrap();

    unpack(&bytes, s.path(), |u| u.all_members()).unwrap_err();
    assert_eq!(tree(s.path()), ["two.txt"]);
}

#[test]
#[cfg(unix)]
fn writes_files_with_the_umasks_permissions_rather_than_the_archives() {
    use std::os::unix::fs::PermissionsExt;
    let bytes = with_members(vec![
        Member::new("tool.sh", b"#!/bin/sh\n").with_mode(0o104_777)
    ]);
    let s = sandbox();
    unpack(&bytes, s.path(), |u| u.member("tool.sh")).unwrap();

    std::fs::write(s.path().join("ordinary"), b"").unwrap();
    let mode = |n: &str| {
        std::fs::metadata(s.path().join(n))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    };
    assert_eq!(mode("tool.sh"), mode("ordinary"));
}

#[test]
fn a_case_collision_fails_whole_where_the_filesystem_folds_case() {
    let probe = sandbox();
    std::fs::write(probe.path().join("x"), b"").unwrap();
    if !probe.path().join("X").exists() {
        eprintln!("skipped: this filesystem is case-sensitive");
        return;
    }
    let bytes = with_members(vec![
        Member::new("Notes.md", b"1"),
        Member::new("notes.md", b"2"),
    ]);
    refused(&bytes, |u| u.all_members());
}
