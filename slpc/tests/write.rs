// The write path: packing a container, and rewriting one's flyleaf.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

mod support;

use std::io::{Read, Write};
use support::{content_of, flyleaf, open, raw_zip, Member};

use slpc::{Error, Malformed, NameError, Unsupported, FLYLEAF_MEMBER, VERSION_KEY};
use toml_edit::DocumentMut;

/// A sink that is only a `Write`, to hold the writer bound honest.
#[derive(Default)]
struct WriteOnly(Vec<u8>);
impl Write for WriteOnly {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A sink that can also seek, which repacking asks for and packing does not.
#[derive(Default)]
struct Seekable(std::io::Cursor<Vec<u8>>);
impl Seekable {
    fn bytes(&self) -> &[u8] {
        self.0.get_ref()
    }
}
impl Write for Seekable {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl std::io::Seek for Seekable {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(to)
    }
}

/// A reader that is only a `Read`, standing in for a pipe.
struct ReadOnly(std::io::Cursor<Vec<u8>>);
impl Read for ReadOnly {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(b)
    }
}

fn pipe(bytes: &[u8]) -> ReadOnly {
    ReadOnly(std::io::Cursor::new(bytes.to_vec()))
}

fn doc(text: &str) -> DocumentMut {
    text.parse().unwrap()
}

/// Every member of an archive, in order, with its compression method.
fn members(bytes: &[u8]) -> Vec<(String, zip::CompressionMethod)> {
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    (0..a.len())
        .map(|i| {
            let f = a.by_index_raw(i).unwrap();
            (f.name().to_owned(), f.compression())
        })
        .collect()
}

// --- Packing ---------------------------------------------------------------

#[test]
fn packs_a_container_that_reads_back() {
    let mut out = WriteOnly::default();
    slpc::pack_reader(
        "report.pdf",
        pipe(b"content\n"),
        DocumentMut::new(),
        &mut out,
    )
    .unwrap();

    let mut c = open(&out.0).unwrap();
    assert_eq!(c.version(), "1.1");
    assert_eq!(c.content_name(), "report.pdf");
    assert_eq!(content_of(&mut c), b"content\n");
}

#[test]
fn sets_both_required_keys_itself() {
    let mut out = WriteOnly::default();
    slpc::pack_reader("a.txt", pipe(b"x"), DocumentMut::new(), &mut out).unwrap();

    let c = open(&out.0).unwrap();
    assert_eq!(c.flyleaf()[VERSION_KEY].as_str(), Some("1.1"));
    assert_eq!(c.flyleaf()["content"]["file"].as_str(), Some("a.txt"));
}

#[test]
fn the_flyleaf_it_generates_looks_like_the_specification_example() {
    // Not a conformance rule: an inline table would be valid TOML and a
    // conformant container. It is that this is the implementation whose output
    // everyone will copy, so what it writes should look like SPEC 2.2.
    let mut out = WriteOnly::default();
    slpc::pack_reader("report.pdf", pipe(b"x"), DocumentMut::new(), &mut out).unwrap();

    let c = open(&out.0).unwrap();
    assert_eq!(
        String::from_utf8_lossy(c.flyleaf_bytes()),
        "slipcase_version = \"1.1\"\n\n[content]\nfile = \"report.pdf\"\n"
    );
}

#[test]
fn passes_everything_else_in_the_flyleaf_through() {
    let given = doc("title = \"Q3 results\"\n\n[custom]\nnested = { deep = [1, 2] }\n");
    let mut out = WriteOnly::default();
    slpc::pack_reader("a.txt", pipe(b"x"), given, &mut out).unwrap();

    let c = open(&out.0).unwrap();
    assert_eq!(c.flyleaf()["title"].as_str(), Some("Q3 results"));
    assert!(c.flyleaf()["custom"]["nested"]["deep"].is_array());
}

#[test]
fn accepts_flyleaf_that_already_agrees() {
    let given = doc("slipcase_version = \"1.1\"\n\n[content]\nfile = \"a.txt\"\n");
    let mut out = WriteOnly::default();
    slpc::pack_reader("a.txt", pipe(b"x"), given, &mut out).unwrap();
    assert_eq!(open(&out.0).unwrap().content_name(), "a.txt");
}

#[test]
fn refuses_flyleaf_that_names_a_different_content() {
    let given = doc("[content]\nfile = \"something-else.txt\"\n");
    let e = slpc::pack_reader("a.txt", pipe(b"x"), given, WriteOnly::default()).unwrap_err();
    match e {
        Error::Malformed(Malformed::Disagrees {
            key,
            found,
            writing,
        }) => {
            assert_eq!(key, "content.file");
            assert_eq!(found, "something-else.txt");
            assert_eq!(writing, "a.txt");
        }
        other => panic!("expected Disagrees, got {other:?}"),
    }
}

#[test]
fn refuses_flyleaf_that_claims_a_different_version() {
    let given = doc("slipcase_version = \"9.4\"\n");
    let e = slpc::pack_reader("a.txt", pipe(b"x"), given, WriteOnly::default()).unwrap_err();
    assert!(matches!(
        e,
        Error::Malformed(Malformed::Disagrees {
            key: "slipcase_version",
            ..
        })
    ));
}

#[test]
fn refuses_flyleaf_whose_content_is_not_a_table() {
    let given = doc("content = 3\n");
    let e = slpc::pack_reader("a.txt", pipe(b"x"), given, WriteOnly::default()).unwrap_err();
    assert!(matches!(e, Error::Malformed(Malformed::ContentNotATable)));
}

#[test]
fn refuses_a_content_name_that_is_not_a_plain_filename() {
    for (name, want) in [
        ("", NameError::Empty),
        ("..", NameError::Relative),
        ("../etc/passwd", NameError::Separator('/')),
        ("a\\b", NameError::Separator('\\')),
        ("C:evil", NameError::Colon),
        (FLYLEAF_MEMBER, NameError::ReservedForFlyleaf),
    ] {
        let e = slpc::pack_reader(name, pipe(b"x"), DocumentMut::new(), WriteOnly::default())
            .unwrap_err();
        match e {
            Error::Malformed(Malformed::ContentName(got)) => assert_eq!(got, want, "{name:?}"),
            other => panic!("expected ContentName for {name:?}, got {other:?}"),
        }
    }
}

#[test]
fn packs_a_content_of_unknown_length_into_a_writer_that_cannot_seek() {
    // Neither end is seekable, which is the case the reader form exists for.
    let content: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let mut out = WriteOnly::default();
    slpc::pack_reader("big.bin", pipe(&content), DocumentMut::new(), &mut out).unwrap();

    let mut c = open(&out.0).unwrap();
    assert_eq!(content_of(&mut c), content);
}

#[test]
fn a_non_ascii_content_name_round_trips() {
    let mut out = WriteOnly::default();
    slpc::pack_reader(
        "caf\u{e9} r\u{e9}sum\u{e9}.txt",
        pipe(b"x"),
        DocumentMut::new(),
        &mut out,
    )
    .unwrap();
    let c = open(&out.0).unwrap();
    assert_eq!(c.content_name(), "caf\u{e9} r\u{e9}sum\u{e9}.txt");
}

#[test]
fn what_it_packs_it_validates() {
    let mut out = WriteOnly::default();
    slpc::pack_reader("a.txt", pipe(b"x"), DocumentMut::new(), &mut out).unwrap();
    slpc::validate(std::io::Cursor::new(out.0)).unwrap();
}

// --- Packing from a path ---------------------------------------------------

#[test]
fn pack_file_takes_the_name_from_the_path() {
    let dir = std::env::temp_dir().join(format!("slpc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("report.pdf");
    std::fs::write(&path, b"on disk\n").unwrap();

    let mut out = WriteOnly::default();
    slpc::pack_file(&path, DocumentMut::new(), &mut out).unwrap();
    let mut c = open(&out.0).unwrap();
    assert_eq!(c.content_name(), "report.pdf");
    assert_eq!(content_of(&mut c), b"on disk\n");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pack_file_names_the_path_when_the_file_cannot_be_packed_as_itself() {
    // A directory entry, so there is no filename to take content.file from.
    let e = slpc::pack_file("..", DocumentMut::new(), WriteOnly::default()).unwrap_err();
    match e {
        Error::Malformed(Malformed::ContentPathName { path, cause }) => {
            assert_eq!(path, std::path::Path::new(".."));
            assert_eq!(cause, NameError::Empty);
        }
        other => panic!("expected ContentPathName, got {other:?}"),
    }
}

// --- Rewriting -------------------------------------------------------------

fn source_with_extras() -> Vec<u8> {
    raw_zip(&[
        Member::new(FLYLEAF_MEMBER, flyleaf("a.txt").as_bytes()),
        Member::new("a.txt", b"content\n"),
        Member::new("notes.md", b"a member nothing here understands\n"),
        Member::new("opaque.bin", b"pretend this is bzip2").claims_method(12),
    ])
}

#[test]
fn rewriting_replaces_the_flyleaf_and_keeps_everything_else() {
    let src = source_with_extras();
    let new =
        "slipcase_version = \"1.1\"\ntitle = \"added later\"\n\n[content]\nfile = \"a.txt\"\n";

    let mut out = Seekable::default();
    slpc::rewrite_flyleaf_bytes(std::io::Cursor::new(src.clone()), new.as_bytes(), &mut out)
        .unwrap();

    let mut c = open(out.bytes()).unwrap();
    assert_eq!(c.flyleaf()["title"].as_str(), Some("added later"));
    assert_eq!(content_of(&mut c), b"content\n");

    let names: Vec<String> = members(out.bytes()).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [FLYLEAF_MEMBER, "a.txt", "notes.md", "opaque.bin"]);
}

#[test]
fn rewriting_copies_a_member_it_cannot_decompress() {
    let src = source_with_extras();
    let new = flyleaf("a.txt");
    let mut out = Seekable::default();
    slpc::rewrite_flyleaf_bytes(std::io::Cursor::new(src), new.as_bytes(), &mut out).unwrap();

    let opaque = members(out.bytes())
        .into_iter()
        .find(|(n, _)| n == "opaque.bin")
        .unwrap();
    assert_eq!(format!("{:?}", opaque.1), "Unsupported(12)");
}

#[test]
fn rewriting_stores_the_bytes_exactly_as_handed_in() {
    let src = source_with_extras();
    let new = "# hand written, and it stays that way\nslipcase_version   =   \"1.1\"\n\n[content]\nfile = \"a.txt\"   # kept\n";
    let mut out = Seekable::default();
    slpc::rewrite_flyleaf_bytes(std::io::Cursor::new(src), new.as_bytes(), &mut out).unwrap();

    assert_eq!(open(out.bytes()).unwrap().flyleaf_bytes(), new.as_bytes());
}

#[test]
fn rewriting_from_a_document_keeps_its_formatting() {
    let src = source_with_extras();
    let mut d: DocumentMut =
        "# a comment\nslipcase_version = \"1.1\"\n\n[content]\nfile = \"a.txt\"\n"
            .parse()
            .unwrap();
    d["title"] = toml_edit::value("added");

    let mut out = Seekable::default();
    slpc::rewrite_flyleaf(std::io::Cursor::new(src), &d, &mut out).unwrap();

    let c = open(out.bytes()).unwrap();
    assert!(String::from_utf8_lossy(c.flyleaf_bytes()).starts_with("# a comment\n"));
    assert_eq!(c.flyleaf()["title"].as_str(), Some("added"));
}

#[test]
fn rewriting_may_repoint_the_content_at_another_member() {
    let src = source_with_extras();
    let new = flyleaf("notes.md");
    let mut out = Seekable::default();
    slpc::rewrite_flyleaf_bytes(std::io::Cursor::new(src), new.as_bytes(), &mut out).unwrap();

    let mut c = open(out.bytes()).unwrap();
    assert_eq!(c.content_name(), "notes.md");
    assert_eq!(content_of(&mut c), b"a member nothing here understands\n");
}

#[test]
fn rewriting_refuses_flyleaf_naming_a_member_that_is_not_there() {
    let src = source_with_extras();
    let new = flyleaf("absent.txt");
    let e = slpc::rewrite_flyleaf_bytes(
        std::io::Cursor::new(src),
        new.as_bytes(),
        Seekable::default(),
    )
    .unwrap_err();
    assert!(matches!(e, Error::Malformed(Malformed::NoContentMember(_))));
}

#[test]
fn rewriting_refuses_flyleaf_that_is_not_a_conformant_document() {
    let src = source_with_extras();
    for (bytes, want) in [
        (b"not = = toml\n".to_vec(), "FlyleafNotToml"),
        (b"slipcase_version = \"1.1\"\n".to_vec(), "MissingKey"),
        (b"\xff\xfe".to_vec(), "FlyleafNotUtf8"),
    ] {
        let e = slpc::rewrite_flyleaf_bytes(
            std::io::Cursor::new(src.clone()),
            &bytes,
            Seekable::default(),
        )
        .unwrap_err();
        assert!(format!("{e:?}").contains(want), "{want}: got {e:?}");
    }
}

#[test]
fn rewriting_refuses_flyleaf_that_claims_a_version_this_build_does_not_write() {
    let src = source_with_extras();
    let new = "slipcase_version = \"9.4\"\n\n[content]\nfile = \"a.txt\"\n";
    let e = slpc::rewrite_flyleaf_bytes(
        std::io::Cursor::new(src),
        new.as_bytes(),
        Seekable::default(),
    )
    .unwrap_err();
    assert!(matches!(
        e,
        Error::Malformed(Malformed::Disagrees {
            key: "slipcase_version",
            ..
        })
    ));
}

#[test]
fn rewriting_refuses_a_source_whose_version_it_does_not_recognise() {
    let src = raw_zip(&[
        Member::new(
            FLYLEAF_MEMBER,
            b"slipcase_version = \"9.4\"\n\n[content]\nfile = \"a.txt\"\n",
        ),
        Member::new("a.txt", b"x"),
    ]);
    let new = flyleaf("a.txt");
    let e = slpc::rewrite_flyleaf_bytes(
        std::io::Cursor::new(src),
        new.as_bytes(),
        Seekable::default(),
    )
    .unwrap_err();
    match e {
        Error::Unsupported(Unsupported::Version(v)) => assert_eq!(v, "9.4"),
        other => panic!("expected Unsupported::Version, got {other:?}"),
    }
}

#[test]
fn rewriting_refuses_a_source_that_is_not_a_container() {
    let src = raw_zip(&[Member::new("a.txt", b"no flyleaf member here")]);
    let new = flyleaf("a.txt");
    let e = slpc::rewrite_flyleaf_bytes(
        std::io::Cursor::new(src),
        new.as_bytes(),
        Seekable::default(),
    )
    .unwrap_err();
    assert!(matches!(e, Error::Malformed(Malformed::NoFlyleafMember)));
}

#[test]
fn a_rewrite_survives_a_second_rewrite() {
    let src = source_with_extras();
    let mut once = Seekable::default();
    slpc::rewrite_flyleaf_bytes(
        std::io::Cursor::new(src),
        flyleaf("a.txt").as_bytes(),
        &mut once,
    )
    .unwrap();

    let mut twice = Seekable::default();
    slpc::rewrite_flyleaf_bytes(
        std::io::Cursor::new(once.bytes().to_vec()),
        flyleaf("notes.md").as_bytes(),
        &mut twice,
    )
    .unwrap();

    let names: Vec<String> = members(twice.bytes()).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [FLYLEAF_MEMBER, "a.txt", "notes.md", "opaque.bin"]);
    assert_eq!(open(twice.bytes()).unwrap().content_name(), "notes.md");
}

// --- Repacking -------------------------------------------------------------

/// A container whose flyleaf carries a comment and a key nothing here knows.
///
/// Both are what SPEC 3 requires to survive, and neither can survive by
/// accident: a document that is parsed and re-serialized keeps them only
/// because the representation was chosen to keep them.
fn source_with_history() -> Vec<u8> {
    raw_zip(&[
        Member::new(
            FLYLEAF_MEMBER,
            b"# written by hand\nslipcase_version = \"1.1\"\ntitle = \"the quarterly\"\n\n[content]\nfile = \"a.txt\"\n",
        ),
        Member::new("a.txt", b"content\n"),
        Member::new("notes.md", b"a member nothing here understands\n"),
        Member::new("opaque.bin", b"pretend this is bzip2").claims_method(12),
    ])
}

#[test]
fn repacking_replaces_the_content_and_keeps_everything_else() {
    let src = source_with_extras();
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src))
        .content("a.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let mut c = open(out.bytes()).unwrap();
    assert_eq!(content_of(&mut c), b"revised\n");
    assert_eq!(c.content_name(), "a.txt");

    let names: Vec<String> = members(out.bytes()).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [FLYLEAF_MEMBER, "a.txt", "notes.md", "opaque.bin"]);
}

#[test]
fn repacking_a_content_under_its_own_name_does_not_touch_the_flyleaf_member() {
    let src = source_with_history();
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src.clone()))
        .content("a.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    // The same bytes, and stored rather than deflated: the member was copied
    // through rather than written again. Nothing in it changed, so nothing
    // about it should have.
    let before = open(&src).unwrap().flyleaf_bytes().to_vec();
    assert_eq!(open(out.bytes()).unwrap().flyleaf_bytes(), before);
    assert_eq!(
        members(out.bytes())[0].1,
        zip::CompressionMethod::Stored,
        "the flyleaf member was re-emitted rather than copied"
    );
}

#[test]
fn repacking_under_a_new_name_moves_content_file_with_it() {
    let src = source_with_extras();
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src))
        .content("b.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let mut c = open(out.bytes()).unwrap();
    assert_eq!(c.content_name(), "b.txt");
    assert_eq!(content_of(&mut c), b"revised\n");

    // The old member is gone rather than left beside the new one, and the new
    // one sits where it sat.
    let names: Vec<String> = members(out.bytes()).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [FLYLEAF_MEMBER, "b.txt", "notes.md", "opaque.bin"]);
}

#[test]
fn repacking_keeps_the_comments_and_the_keys_it_does_not_know() {
    let src = source_with_history();
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src))
        .content("b.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let c = open(out.bytes()).unwrap();
    let text = String::from_utf8(c.flyleaf_bytes().to_vec()).unwrap();
    assert!(text.starts_with("# written by hand\n"), "{text}");
    assert_eq!(c.flyleaf()["title"].as_str(), Some("the quarterly"));
    assert_eq!(c.content_name(), "b.txt");
}

#[test]
fn repointing_content_file_keeps_the_comment_beside_it() {
    // The key the library edits itself is the one key SPEC 3's guarantee is
    // easiest to lose: everything else survives because nothing touches it.
    let src = raw_zip(&[
        Member::new(
            FLYLEAF_MEMBER,
            b"slipcase_version = \"1.1\"\n\n[content]\nfile = \"a.txt\"  # SPEC 2.2\n",
        ),
        Member::new("a.txt", b"content\n"),
    ]);

    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src))
        .content("b.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let c = open(out.bytes()).unwrap();
    assert_eq!(c.content_name(), "b.txt");
    assert_eq!(
        String::from_utf8(c.flyleaf_bytes().to_vec()).unwrap(),
        "slipcase_version = \"1.1\"\n\n[content]\nfile = \"b.txt\"  # SPEC 2.2\n"
    );
}

#[test]
fn repointing_content_file_keeps_the_whitespace_of_a_multiline_inline_table() {
    // A value written inside a multi-line inline table carries the space before
    // its trailing comma as decor. Replacing the value rather than the item is
    // what keeps the table looking the way it was written.
    let written = "slipcase_version = \"1.1\"\ncontent = {\n    file = \"a.txt\" ,\n}\n";
    let src = raw_zip(&[
        Member::new(FLYLEAF_MEMBER, written.as_bytes()),
        Member::new("a.txt", b"content\n"),
    ]);

    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src))
        .content("b.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let c = open(out.bytes()).unwrap();
    assert_eq!(
        String::from_utf8(c.flyleaf_bytes().to_vec()).unwrap(),
        written.replace("a.txt", "b.txt")
    );
}

#[test]
fn repacking_under_the_name_the_document_already_carries_rewrites_nothing() {
    // Handing over a document and replacing the content file under the name it
    // already had are two things a caller does in one write. The name did not
    // change, so `content.file` should not be written at all.
    let written = "slipcase_version = \"1.1\"\n\n[content]\nfile = \"a.txt\"  # SPEC 2.2\n";
    let src = raw_zip(&[
        Member::new(FLYLEAF_MEMBER, written.as_bytes()),
        Member::new("a.txt", b"content\n"),
    ]);
    let given = doc(written);

    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src))
        .flyleaf(&given)
        .content("a.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let mut c = open(out.bytes()).unwrap();
    assert_eq!(content_of(&mut c), b"revised\n");
    assert_eq!(
        String::from_utf8(c.flyleaf_bytes().to_vec()).unwrap(),
        written
    );
}

#[test]
fn repacking_refuses_a_name_another_member_already_carries() {
    let src = source_with_extras();
    let e = slpc::Repack::new(std::io::Cursor::new(src))
        .content("notes.md", pipe(b"revised\n"))
        .write(Seekable::default())
        .unwrap_err();
    match e {
        Error::Malformed(Malformed::ContentNameTaken(n)) => assert_eq!(n, "notes.md"),
        other => panic!("expected ContentNameTaken, got {other:?}"),
    }
}

#[test]
fn repacking_refuses_a_content_name_that_is_not_a_plain_filename() {
    for name in ["", ".", "..", "a/b", "a\\b", "C:x", FLYLEAF_MEMBER] {
        let e = slpc::Repack::new(std::io::Cursor::new(source_with_extras()))
            .content(name, pipe(b"x"))
            .write(Seekable::default())
            .unwrap_err();
        assert!(
            matches!(e, Error::Malformed(Malformed::ContentName(_))),
            "{name:?}: {e:?}"
        );
    }
}

#[test]
fn repacking_takes_a_content_name_from_a_path() {
    let dir = std::env::temp_dir().join(format!("slpc-repack-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("b.txt");
    std::fs::write(&path, b"from a file\n").unwrap();

    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(source_with_extras()))
        .content_file(&path)
        .unwrap()
        .write(&mut out)
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let mut c = open(out.bytes()).unwrap();
    assert_eq!(c.content_name(), "b.txt");
    assert_eq!(content_of(&mut c), b"from a file\n");
}

#[test]
fn repacking_sets_content_file_in_a_document_and_refuses_bytes_that_disagree() {
    let src = source_with_extras();

    // A document is edited, because the value it carried named the member being
    // replaced and cannot have meant anything else.
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src.clone()))
        .flyleaf(&doc(
            "slipcase_version = \"1.1\"\n\n[content]\nfile = \"a.txt\"\n",
        ))
        .content("b.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();
    assert_eq!(open(out.bytes()).unwrap().content_name(), "b.txt");

    // Bytes are stored as handed in, so they are refused rather than corrected.
    let e = slpc::Repack::new(std::io::Cursor::new(src))
        .flyleaf_bytes(flyleaf("a.txt").as_bytes())
        .content("b.txt", pipe(b"revised\n"))
        .write(Seekable::default())
        .unwrap_err();
    assert!(
        matches!(
            e,
            Error::Malformed(Malformed::Disagrees {
                key: "content.file",
                ..
            })
        ),
        "{e:?}"
    );
}

#[test]
fn repacking_changes_both_halves_at_once() {
    let src = source_with_extras();
    let new = "slipcase_version = \"1.1\"\ntitle = \"revised\"\n\n[content]\nfile = \"b.txt\"\n";

    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src))
        .flyleaf_bytes(new.as_bytes())
        .content("b.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let mut c = open(out.bytes()).unwrap();
    assert_eq!(c.flyleaf()["title"].as_str(), Some("revised"));
    assert_eq!(content_of(&mut c), b"revised\n");
    let names: Vec<String> = members(out.bytes()).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [FLYLEAF_MEMBER, "b.txt", "notes.md", "opaque.bin"]);
}

#[test]
fn repacking_nothing_copies_the_container_through() {
    let src = source_with_history();
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(src.clone()))
        .write(&mut out)
        .unwrap();

    assert_eq!(
        open(out.bytes()).unwrap().flyleaf_bytes(),
        open(&src).unwrap().flyleaf_bytes()
    );
    // Every member under the name and the compression method it arrived with:
    // nothing was decompressed in order to be written again.
    assert_eq!(members(out.bytes()), members(&src));
}

#[test]
fn repacking_copies_a_member_it_cannot_decompress() {
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(source_with_extras()))
        .content("a.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let opaque = members(out.bytes())
        .into_iter()
        .find(|(n, _)| n == "opaque.bin")
        .unwrap();
    assert_eq!(format!("{:?}", opaque.1), "Unsupported(12)");
}

#[test]
fn repacking_refuses_a_source_whose_version_it_does_not_recognise() {
    let src = raw_zip(&[
        Member::new(
            FLYLEAF_MEMBER,
            b"slipcase_version = \"9.4\"\n\n[content]\nfile = \"a.txt\"\n",
        ),
        Member::new("a.txt", b"x"),
    ]);
    let e = slpc::Repack::new(std::io::Cursor::new(src))
        .content("a.txt", pipe(b"revised\n"))
        .write(Seekable::default())
        .unwrap_err();
    match e {
        Error::Unsupported(Unsupported::Version(v)) => assert_eq!(v, "9.4"),
        other => panic!("expected Unsupported::Version, got {other:?}"),
    }
}

#[test]
fn what_it_repacks_it_validates() {
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(source_with_history()))
        .content("b.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let v = slpc::validate(std::io::Cursor::new(out.bytes().to_vec())).unwrap();
    assert!(v.is_conformant(), "{v}");
}

#[test]
fn repacking_streams_a_content_of_unknown_length_into_a_writer_that_cannot_seek() {
    let big = vec![b'x'; 200_000];
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(source_with_extras()))
        .content("a.txt", pipe(&big))
        .write(&mut out)
        .unwrap();

    assert_eq!(content_of(&mut open(out.bytes()).unwrap()), big);
}

#[test]
fn repacking_leaves_no_member_promising_a_data_descriptor() {
    // A ZIP written to a stream cannot know a member's size before the data
    // goes down, so it writes zeroes into the local header, sets general
    // purpose bit 3, and puts the sizes in a data descriptor after the data.
    // A member copied through raw already knows its sizes, and `zip` 8.6 sets
    // the bit for it and then writes no descriptor: the local header claims a
    // length of zero and nothing supplies the real one. Every reader that walks
    // the central directory is fine, which is why the rest of this suite could
    // not see it, and every reader that walks forward is not. Info-ZIP calls
    // the result overlapping components and exits 12.
    //
    // Repacking therefore writes to a seekable stream, where the header is
    // filled in afterwards and no descriptor is promised at all.
    let mut out = Seekable::default();
    slpc::Repack::new(std::io::Cursor::new(source_with_extras()))
        .content("a.txt", pipe(b"revised\n"))
        .write(&mut out)
        .unwrap();

    let bytes = out.bytes();
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    for i in 0..a.len() {
        let f = a.by_index_raw(i).unwrap();
        let name = f.name().to_owned();
        let at = usize::try_from(f.header_start()).unwrap();
        let flags = u16::from_le_bytes([bytes[at + 6], bytes[at + 7]]);
        let local_size = u32::from_le_bytes(bytes[at + 18..at + 22].try_into().unwrap());

        assert_eq!(flags & (1 << 3), 0, "{name} claims a data descriptor");
        assert_eq!(
            u64::from(local_size),
            f.compressed_size(),
            "{name}: the local header disagrees with the central directory"
        );
    }
}
