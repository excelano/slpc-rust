// The write path: packing a container, and rewriting one's metadata.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

mod support;

use std::io::{Read, Write};
use support::{metadata, open, payload_of, raw_zip, Member};

use slpc::{Error, Malformed, NameError, Unsupported, METADATA_MEMBER, VERSION_KEY};
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
        pipe(b"payload\n"),
        DocumentMut::new(),
        &mut out,
    )
    .unwrap();

    let mut c = open(&out.0).unwrap();
    assert_eq!(c.version(), "1.0");
    assert_eq!(c.payload_name(), "report.pdf");
    assert_eq!(payload_of(&mut c), b"payload\n");
}

#[test]
fn sets_both_required_keys_itself() {
    let mut out = WriteOnly::default();
    slpc::pack_reader("a.txt", pipe(b"x"), DocumentMut::new(), &mut out).unwrap();

    let c = open(&out.0).unwrap();
    assert_eq!(c.metadata()[VERSION_KEY].as_str(), Some("1.0"));
    assert_eq!(c.metadata()["payload"]["file"].as_str(), Some("a.txt"));
}

#[test]
fn the_metadata_it_generates_looks_like_the_specification_example() {
    // Not a conformance rule: an inline table would be valid TOML and a
    // conformant container. It is that this is the implementation whose output
    // everyone will copy, so what it writes should look like SPEC 2.2.
    let mut out = WriteOnly::default();
    slpc::pack_reader("report.pdf", pipe(b"x"), DocumentMut::new(), &mut out).unwrap();

    let c = open(&out.0).unwrap();
    assert_eq!(
        String::from_utf8_lossy(c.metadata_bytes()),
        "slipcase_version = \"1.0\"\n\n[payload]\nfile = \"report.pdf\"\n"
    );
}

#[test]
fn passes_everything_else_in_the_metadata_through() {
    let given = doc("title = \"Q3 results\"\n\n[custom]\nnested = { deep = [1, 2] }\n");
    let mut out = WriteOnly::default();
    slpc::pack_reader("a.txt", pipe(b"x"), given, &mut out).unwrap();

    let c = open(&out.0).unwrap();
    assert_eq!(c.metadata()["title"].as_str(), Some("Q3 results"));
    assert!(c.metadata()["custom"]["nested"]["deep"].is_array());
}

#[test]
fn accepts_metadata_that_already_agrees() {
    let given = doc("slipcase_version = \"1.0\"\n\n[payload]\nfile = \"a.txt\"\n");
    let mut out = WriteOnly::default();
    slpc::pack_reader("a.txt", pipe(b"x"), given, &mut out).unwrap();
    assert_eq!(open(&out.0).unwrap().payload_name(), "a.txt");
}

#[test]
fn refuses_metadata_that_names_a_different_payload() {
    let given = doc("[payload]\nfile = \"something-else.txt\"\n");
    let e = slpc::pack_reader("a.txt", pipe(b"x"), given, WriteOnly::default()).unwrap_err();
    match e {
        Error::Malformed(Malformed::Disagrees {
            key,
            found,
            writing,
        }) => {
            assert_eq!(key, "payload.file");
            assert_eq!(found, "something-else.txt");
            assert_eq!(writing, "a.txt");
        }
        other => panic!("expected Disagrees, got {other:?}"),
    }
}

#[test]
fn refuses_metadata_that_claims_a_different_version() {
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
fn refuses_metadata_whose_payload_is_not_a_table() {
    let given = doc("payload = 3\n");
    let e = slpc::pack_reader("a.txt", pipe(b"x"), given, WriteOnly::default()).unwrap_err();
    assert!(matches!(e, Error::Malformed(Malformed::PayloadNotATable)));
}

#[test]
fn refuses_a_payload_name_that_is_not_a_plain_filename() {
    for (name, want) in [
        ("", NameError::Empty),
        ("..", NameError::Relative),
        ("../etc/passwd", NameError::Separator('/')),
        ("a\\b", NameError::Separator('\\')),
        ("C:evil", NameError::Colon),
        (METADATA_MEMBER, NameError::ReservedForMetadata),
    ] {
        let e = slpc::pack_reader(name, pipe(b"x"), DocumentMut::new(), WriteOnly::default())
            .unwrap_err();
        match e {
            Error::Malformed(Malformed::PayloadName(got)) => assert_eq!(got, want, "{name:?}"),
            other => panic!("expected PayloadName for {name:?}, got {other:?}"),
        }
    }
}

#[test]
fn packs_a_payload_of_unknown_length_into_a_writer_that_cannot_seek() {
    // Neither end is seekable, which is the case the reader form exists for.
    let payload: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    let mut out = WriteOnly::default();
    slpc::pack_reader("big.bin", pipe(&payload), DocumentMut::new(), &mut out).unwrap();

    let mut c = open(&out.0).unwrap();
    assert_eq!(payload_of(&mut c), payload);
}

#[test]
fn a_non_ascii_payload_name_round_trips() {
    let mut out = WriteOnly::default();
    slpc::pack_reader(
        "caf\u{e9} r\u{e9}sum\u{e9}.txt",
        pipe(b"x"),
        DocumentMut::new(),
        &mut out,
    )
    .unwrap();
    let c = open(&out.0).unwrap();
    assert_eq!(c.payload_name(), "caf\u{e9} r\u{e9}sum\u{e9}.txt");
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
    assert_eq!(c.payload_name(), "report.pdf");
    assert_eq!(payload_of(&mut c), b"on disk\n");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pack_file_names_the_path_when_the_file_cannot_be_packed_as_itself() {
    // A directory entry, so there is no filename to take payload.file from.
    let e = slpc::pack_file("..", DocumentMut::new(), WriteOnly::default()).unwrap_err();
    match e {
        Error::Malformed(Malformed::PayloadPathName { path, cause }) => {
            assert_eq!(path, std::path::Path::new(".."));
            assert_eq!(cause, NameError::Empty);
        }
        other => panic!("expected PayloadPathName, got {other:?}"),
    }
}

// --- Rewriting -------------------------------------------------------------

fn source_with_extras() -> Vec<u8> {
    raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"payload\n"),
        Member::new("notes.md", b"a member nothing here understands\n"),
        Member::new("opaque.bin", b"pretend this is bzip2").claims_method(12),
    ])
}

#[test]
fn rewriting_replaces_the_metadata_and_keeps_everything_else() {
    let src = source_with_extras();
    let new =
        "slipcase_version = \"1.0\"\ntitle = \"added later\"\n\n[payload]\nfile = \"a.txt\"\n";

    let mut out = WriteOnly::default();
    slpc::rewrite_metadata_bytes(std::io::Cursor::new(src.clone()), new.as_bytes(), &mut out)
        .unwrap();

    let mut c = open(&out.0).unwrap();
    assert_eq!(c.metadata()["title"].as_str(), Some("added later"));
    assert_eq!(payload_of(&mut c), b"payload\n");

    let names: Vec<String> = members(&out.0).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [METADATA_MEMBER, "a.txt", "notes.md", "opaque.bin"]);
}

#[test]
fn rewriting_copies_a_member_it_cannot_decompress() {
    let src = source_with_extras();
    let new = metadata("a.txt");
    let mut out = WriteOnly::default();
    slpc::rewrite_metadata_bytes(std::io::Cursor::new(src), new.as_bytes(), &mut out).unwrap();

    let opaque = members(&out.0)
        .into_iter()
        .find(|(n, _)| n == "opaque.bin")
        .unwrap();
    assert_eq!(format!("{:?}", opaque.1), "Unsupported(12)");
}

#[test]
fn rewriting_stores_the_bytes_exactly_as_handed_in() {
    let src = source_with_extras();
    let new = "# hand written, and it stays that way\nslipcase_version   =   \"1.0\"\n\n[payload]\nfile = \"a.txt\"   # kept\n";
    let mut out = WriteOnly::default();
    slpc::rewrite_metadata_bytes(std::io::Cursor::new(src), new.as_bytes(), &mut out).unwrap();

    assert_eq!(open(&out.0).unwrap().metadata_bytes(), new.as_bytes());
}

#[test]
fn rewriting_from_a_document_keeps_its_formatting() {
    let src = source_with_extras();
    let mut d: DocumentMut =
        "# a comment\nslipcase_version = \"1.0\"\n\n[payload]\nfile = \"a.txt\"\n"
            .parse()
            .unwrap();
    d["title"] = toml_edit::value("added");

    let mut out = WriteOnly::default();
    slpc::rewrite_metadata(std::io::Cursor::new(src), &d, &mut out).unwrap();

    let c = open(&out.0).unwrap();
    assert!(String::from_utf8_lossy(c.metadata_bytes()).starts_with("# a comment\n"));
    assert_eq!(c.metadata()["title"].as_str(), Some("added"));
}

#[test]
fn rewriting_may_repoint_the_payload_at_another_member() {
    let src = source_with_extras();
    let new = metadata("notes.md");
    let mut out = WriteOnly::default();
    slpc::rewrite_metadata_bytes(std::io::Cursor::new(src), new.as_bytes(), &mut out).unwrap();

    let mut c = open(&out.0).unwrap();
    assert_eq!(c.payload_name(), "notes.md");
    assert_eq!(payload_of(&mut c), b"a member nothing here understands\n");
}

#[test]
fn rewriting_refuses_metadata_naming_a_member_that_is_not_there() {
    let src = source_with_extras();
    let new = metadata("absent.txt");
    let e = slpc::rewrite_metadata_bytes(
        std::io::Cursor::new(src),
        new.as_bytes(),
        WriteOnly::default(),
    )
    .unwrap_err();
    assert!(matches!(e, Error::Malformed(Malformed::NoPayloadMember(_))));
}

#[test]
fn rewriting_refuses_metadata_that_is_not_a_conformant_document() {
    let src = source_with_extras();
    for (bytes, want) in [
        (b"not = = toml\n".to_vec(), "MetadataNotToml"),
        (b"slipcase_version = \"1.0\"\n".to_vec(), "MissingKey"),
        (b"\xff\xfe".to_vec(), "MetadataNotUtf8"),
    ] {
        let e = slpc::rewrite_metadata_bytes(
            std::io::Cursor::new(src.clone()),
            &bytes,
            WriteOnly::default(),
        )
        .unwrap_err();
        assert!(format!("{e:?}").contains(want), "{want}: got {e:?}");
    }
}

#[test]
fn rewriting_refuses_metadata_that_claims_a_version_this_build_does_not_write() {
    let src = source_with_extras();
    let new = "slipcase_version = \"9.4\"\n\n[payload]\nfile = \"a.txt\"\n";
    let e = slpc::rewrite_metadata_bytes(
        std::io::Cursor::new(src),
        new.as_bytes(),
        WriteOnly::default(),
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
            METADATA_MEMBER,
            b"slipcase_version = \"9.4\"\n\n[payload]\nfile = \"a.txt\"\n",
        ),
        Member::new("a.txt", b"x"),
    ]);
    let new = metadata("a.txt");
    let e = slpc::rewrite_metadata_bytes(
        std::io::Cursor::new(src),
        new.as_bytes(),
        WriteOnly::default(),
    )
    .unwrap_err();
    match e {
        Error::Unsupported(Unsupported::Version(v)) => assert_eq!(v, "9.4"),
        other => panic!("expected Unsupported::Version, got {other:?}"),
    }
}

#[test]
fn rewriting_refuses_a_source_that_is_not_a_container() {
    let src = raw_zip(&[Member::new("a.txt", b"no metadata member here")]);
    let new = metadata("a.txt");
    let e = slpc::rewrite_metadata_bytes(
        std::io::Cursor::new(src),
        new.as_bytes(),
        WriteOnly::default(),
    )
    .unwrap_err();
    assert!(matches!(e, Error::Malformed(Malformed::NoMetadataMember)));
}

#[test]
fn a_rewrite_survives_a_second_rewrite() {
    let src = source_with_extras();
    let mut once = WriteOnly::default();
    slpc::rewrite_metadata_bytes(
        std::io::Cursor::new(src),
        metadata("a.txt").as_bytes(),
        &mut once,
    )
    .unwrap();

    let mut twice = WriteOnly::default();
    slpc::rewrite_metadata_bytes(
        std::io::Cursor::new(once.0.clone()),
        metadata("notes.md").as_bytes(),
        &mut twice,
    )
    .unwrap();

    let names: Vec<String> = members(&twice.0).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [METADATA_MEMBER, "a.txt", "notes.md", "opaque.bin"]);
    assert_eq!(open(&twice.0).unwrap().payload_name(), "notes.md");
}
