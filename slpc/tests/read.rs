// The read path, against archives the suite builds for itself.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

mod support;

use support::{container, metadata, open, payload_of, raw_zip, Member};

use slpc::{EntryKind, Error, Malformed, NameError, Unsupported, METADATA_MEMBER};

#[test]
fn reads_a_container() {
    let bytes = container("report.pdf", b"%PDF-1.7 not really\n");
    let mut c = open(&bytes).unwrap();
    assert_eq!(c.version(), "1.0");
    assert_eq!(c.payload_name(), "report.pdf");
    assert_eq!(payload_of(&mut c), b"%PDF-1.7 not really\n");
}

#[test]
fn reads_a_deflated_payload() {
    // The one fixture built by an ordinary writer, because compressing by hand
    // would test the test rather than the library.
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    w.start_file(METADATA_MEMBER, opts).unwrap();
    std::io::Write::write_all(&mut w, metadata("big.bin").as_bytes()).unwrap();
    w.start_file("big.bin", opts).unwrap();
    std::io::Write::write_all(&mut w, &payload).unwrap();
    let bytes = w.finish().unwrap().into_inner();

    let mut c = open(&bytes).unwrap();
    assert_eq!(c.payload_name(), "big.bin");
    assert_eq!(payload_of(&mut c), payload);
}

#[test]
fn passes_through_keys_and_members_it_does_not_recognise() {
    let meta = "slipcase_version = \"1.0\"\ntitle = \"Q3\"\n\n[payload]\nfile = \"a.txt\"\n\n[custom]\nnested = { deep = [1, 2] }\n";
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, meta.as_bytes()),
        Member::new("a.txt", b"payload\n"),
        Member::new("__MACOSX/._a.txt", b"junk"),
        Member::new(".DS_Store", b"junk"),
    ]);
    let c = open(&bytes).unwrap();
    assert_eq!(c.metadata()["title"].as_str(), Some("Q3"));
    assert!(c.metadata()["custom"]["nested"]["deep"].is_array());
}

#[test]
fn metadata_bytes_are_the_member_as_stored() {
    let meta =
        "# hand written\nslipcase_version   =    \"1.0\"\n\n[payload]\nfile = \"a.txt\"   # kept\n";
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, meta.as_bytes()),
        Member::new("a.txt", b"x"),
    ]);
    let c = open(&bytes).unwrap();
    assert_eq!(c.metadata_bytes(), meta.as_bytes());
    // And the document model agrees, down to the whitespace and the comments.
    assert_eq!(c.metadata().to_string(), meta);
}

#[test]
fn member_order_does_not_matter() {
    let payload_first = raw_zip(&[
        Member::new("a.txt", b"payload\n"),
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
    ]);
    let mut c = open(&payload_first).unwrap();
    assert_eq!(payload_of(&mut c), b"payload\n");
}

// --- Non-conformance, one rule at a time -----------------------------------

#[test]
fn rejects_an_archive_with_no_metadata_member() {
    let bytes = raw_zip(&[Member::new("a.txt", b"lonely")]);
    assert!(matches!(
        open(&bytes),
        Err(Error::Malformed(Malformed::NoMetadataMember))
    ));
}

#[test]
fn rejects_metadata_that_is_not_utf8() {
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, b"slipcase_version = \"\xff\xfe\"\n"),
        Member::new("a.txt", b"x"),
    ]);
    assert!(matches!(
        open(&bytes),
        Err(Error::Malformed(Malformed::MetadataNotUtf8))
    ));
}

#[test]
fn rejects_metadata_that_is_not_toml() {
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, b"this is not = = toml\n"),
        Member::new("a.txt", b"x"),
    ]);
    assert!(matches!(
        open(&bytes),
        Err(Error::Malformed(Malformed::MetadataNotToml(_)))
    ));
}

#[test]
fn rejects_metadata_missing_either_required_key() {
    for (meta, missing) in [
        ("[payload]\nfile = \"a.txt\"\n", "slipcase_version"),
        ("slipcase_version = \"1.0\"\n", "payload.file"),
        ("slipcase_version = \"1.0\"\n[payload]\n", "payload.file"),
    ] {
        let bytes = raw_zip(&[
            Member::new(METADATA_MEMBER, meta.as_bytes()),
            Member::new("a.txt", b"x"),
        ]);
        match open(&bytes) {
            Err(Error::Malformed(Malformed::MissingKey(k))) => assert_eq!(k, missing),
            other => panic!(
                "expected MissingKey({missing}), got {other:?}",
                other = other.err()
            ),
        }
    }
}

#[test]
fn rejects_required_keys_that_are_not_strings() {
    for (meta, key) in [
        (
            "slipcase_version = 1.0\n[payload]\nfile = \"a.txt\"\n",
            "slipcase_version",
        ),
        (
            "slipcase_version = \"1.0\"\n[payload]\nfile = 7\n",
            "payload.file",
        ),
    ] {
        let bytes = raw_zip(&[
            Member::new(METADATA_MEMBER, meta.as_bytes()),
            Member::new("a.txt", b"x"),
        ]);
        match open(&bytes) {
            Err(Error::Malformed(Malformed::KeyNotAString(k))) => assert_eq!(k, key),
            other => panic!(
                "expected KeyNotAString({key}), got {other:?}",
                other = other.err()
            ),
        }
    }
}

#[test]
fn rejects_a_payload_file_that_is_not_a_plain_filename() {
    for (name, want) in [
        ("", NameError::Empty),
        (".", NameError::Relative),
        ("..", NameError::Relative),
        ("../etc/passwd", NameError::Separator('/')),
        ("..\\windows", NameError::Separator('\\')),
        ("C:evil", NameError::Colon),
        (METADATA_MEMBER, NameError::ReservedForMetadata),
    ] {
        let bytes = raw_zip(&[
            Member::new(METADATA_MEMBER, metadata(name).as_bytes()),
            Member::new("a.txt", b"x"),
        ]);
        match open(&bytes) {
            Err(Error::Malformed(Malformed::PayloadName(e))) => assert_eq!(e, want, "{name:?}"),
            other => panic!(
                "expected PayloadName({want:?}) for {name:?}, got {:?}",
                other.err()
            ),
        }
    }
}

#[test]
fn rejects_a_payload_file_that_names_nothing() {
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("absent.txt").as_bytes()),
        Member::new("present.txt", b"x"),
    ]);
    match open(&bytes) {
        Err(Error::Malformed(Malformed::NoPayloadMember(n))) => assert_eq!(n, "absent.txt"),
        other => panic!("expected NoPayloadMember, got {:?}", other.err()),
    }
}

#[test]
fn rejects_a_payload_that_is_not_a_regular_file_entry() {
    // SPEC 2.3 excludes every entry type but one, so each is checked rather
    // than only the symbolic link the earlier text named.
    for (mode, want) in [
        (0o120_777, EntryKind::Symlink),
        (0o040_755, EntryKind::Directory),
        (0o010_644, EntryKind::Other(0o1)),
        (0o140_644, EntryKind::Other(0o14)),
        (0o020_644, EntryKind::Other(0o2)),
    ] {
        let bytes = raw_zip(&[
            Member::new(METADATA_MEMBER, metadata("odd").as_bytes()),
            Member::new("odd", b"payload").with_mode(mode),
        ]);
        match open(&bytes) {
            Err(Error::Malformed(Malformed::PayloadNotARegularFile { kind, .. })) => {
                assert_eq!(kind, want, "mode {mode:o}");
            }
            other => panic!(
                "mode {mode:o}: expected PayloadNotARegularFile, got {:?}",
                other.err()
            ),
        }
    }
}

#[test]
fn rejects_more_than_one_member_of_either_name() {
    // SPEC 2.1 requires exactly one of each. Two agreeing metadata members are
    // the case worth having: taking the first would read them as one container
    // and never notice.
    let two_metadata = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"x"),
    ]);
    match open(&two_metadata) {
        Err(Error::Malformed(Malformed::DuplicateMetadataMember(n))) => assert_eq!(n, 2),
        other => panic!("expected DuplicateMetadataMember, got {:?}", other.err()),
    }

    let two_payloads = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"first"),
        Member::new("a.txt", b"second"),
    ]);
    match open(&two_payloads) {
        Err(Error::Malformed(Malformed::DuplicatePayloadMember { name, count })) => {
            assert_eq!((name.as_str(), count), ("a.txt", 2));
        }
        other => panic!("expected DuplicatePayloadMember, got {:?}", other.err()),
    }
}

#[test]
fn rejects_a_payload_file_containing_a_control_character() {
    for c in ['\u{0}', '\n', '\r', '\u{1f}', '\u{7f}'] {
        let name = format!("rep{c}ort.pdf");
        let bytes = raw_zip(&[
            Member::new(METADATA_MEMBER, metadata(&name).as_bytes()),
            Member::new(&name, b"x"),
        ]);
        match open(&bytes) {
            Err(Error::Malformed(Malformed::PayloadName(NameError::ControlCharacter(got)))) => {
                assert_eq!(got, c);
            }
            other => panic!("U+{:04X}: got {:?}", c as u32, other.err()),
        }
    }
}

#[test]
fn an_entry_made_on_dos_is_not_taken_for_a_symlink() {
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"from windows\n").dos_made(),
    ]);
    let mut c = open(&bytes).unwrap();
    assert_eq!(payload_of(&mut c), b"from windows\n");
}

// --- Member names ----------------------------------------------------------

#[test]
fn matches_a_name_stored_as_cp437() {
    // Bit 11 clear, so the name is CP437: 0x87 is U+00E7.
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("caf\u{e7}.txt").as_bytes()),
        Member::named_raw(b"caf\x87.txt", b"cp437\n"),
    ]);
    let mut c = open(&bytes).unwrap();
    assert_eq!(c.payload_name(), "caf\u{e7}.txt");
    assert_eq!(payload_of(&mut c), b"cp437\n");
}

#[test]
fn never_matches_a_name_the_crate_decoded_lossily() {
    // Bit 11 set over bytes that are not UTF-8. The ZIP crate hands back
    // U+FFFD; a payload.file copied from that lossy name must not match, or the
    // answer would depend on the order the members happen to sit in.
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("caf\u{fffd}.txt").as_bytes()),
        Member::named_raw(b"caf\xff.txt", b"impostor\n").flagged_utf8(),
    ]);
    match open(&bytes) {
        Err(Error::Malformed(Malformed::NoPayloadMember(n))) => assert_eq!(n, "caf\u{fffd}.txt"),
        other => panic!("expected NoPayloadMember, got {:?}", other.err()),
    }
}

// --- Conformant, and this build cannot read it ------------------------------

#[test]
fn an_unrecognised_version_parses_and_reports_but_yields_no_payload() {
    let meta = "slipcase_version = \"9.4\"\n\n[payload]\nfile = \"a.txt\"\n";
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, meta.as_bytes()),
        Member::new("a.txt", b"x"),
    ]);
    let mut c = open(&bytes).unwrap();
    assert_eq!(c.version(), "9.4");
    assert_eq!(c.payload_name(), "a.txt");
    assert_eq!(c.metadata_bytes(), meta.as_bytes());
    let got = c.payload();
    match got {
        Err(Error::Unsupported(Unsupported::Version(v))) => assert_eq!(v, "9.4"),
        other => panic!("expected Unsupported::Version, got {:?}", other.err()),
    }
}

#[test]
fn a_payload_compressed_beyond_this_build_still_validates() {
    // Method 12 is bzip2, which the C-free feature set leaves out. SPEC 2.5
    // forbids rejecting the container for it.
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"pretend this is bzip2").claims_method(12),
    ]);
    slpc::validate(std::io::Cursor::new(bytes.clone())).unwrap();
    let mut c = open(&bytes).unwrap();
    let got = c.payload();
    match got {
        Err(Error::Unsupported(Unsupported::Compression(m))) => assert_eq!(m, 12),
        other => panic!("expected Unsupported::Compression, got {:?}", other.err()),
    }
}

#[test]
fn an_encrypted_payload_still_validates() {
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"ciphertext").encrypted(),
    ]);
    slpc::validate(std::io::Cursor::new(bytes.clone())).unwrap();
    let mut c = open(&bytes).unwrap();
    let got = c.payload();
    assert!(matches!(
        got,
        Err(Error::Unsupported(Unsupported::Encrypted))
    ));
}

#[test]
fn a_container_may_be_its_own_payload() {
    let inner = container("report.pdf", b"inner\n");
    let outer = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("report.pdf.slpc").as_bytes()),
        Member::new("report.pdf.slpc", &inner),
    ]);
    let mut c = open(&outer).unwrap();
    let nested = payload_of(&mut c);
    assert_eq!(nested, inner);
    let mut c = open(&nested).unwrap();
    assert_eq!(c.payload_name(), "report.pdf");
    assert_eq!(payload_of(&mut c), b"inner\n");
}
