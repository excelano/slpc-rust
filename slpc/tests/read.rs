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

// --- the payload's size ----------------------------------------------------

#[test]
fn reports_the_payloads_uncompressed_size() {
    let payload = b"%PDF-1.7 not really\n";
    let c = open(&container("report.pdf", payload)).unwrap();
    assert_eq!(c.payload_size().unwrap(), payload.len() as u64);
}

#[test]
fn the_name_and_the_size_can_be_asked_for_together() {
    // A shared borrow, so this composes in one expression. Anything reporting
    // what is in a container asks both at once, and an accessor needing `&mut`
    // makes that a borrow error on the first line a consumer writes.
    let c = open(&container("report.pdf", b"1234")).unwrap();
    assert_eq!(
        format!(
            "{} is {} bytes",
            c.payload_name(),
            c.payload_size().unwrap()
        ),
        "report.pdf is 4 bytes"
    );
}

#[test]
fn a_payload_of_zero_length_has_a_size_and_not_an_error() {
    // SPEC 2.3 permits a payload of any length, including zero, so this is a
    // number rather than a complaint.
    let c = open(&container("empty.bin", b"")).unwrap();
    assert_eq!(c.payload_size().unwrap(), 0);
}

#[test]
fn the_size_is_the_uncompressed_one() {
    // A deflated payload's stored length is not its length, and a caller sizing
    // a progress bar or a buffer wants what comes out rather than what sits in
    // the archive.
    let text = "a".repeat(4096);
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    w.start_file(METADATA_MEMBER, opts).unwrap();
    std::io::Write::write_all(&mut w, metadata("big.txt").as_bytes()).unwrap();
    w.start_file("big.txt", opts).unwrap();
    std::io::Write::write_all(&mut w, text.as_bytes()).unwrap();
    let bytes = w.finish().unwrap().into_inner();

    let c = open(&bytes).unwrap();
    assert_eq!(c.payload_size().unwrap(), 4096);
    assert!(
        bytes.len() < 2048,
        "the fixture did not compress, so this proves nothing"
    );
}

#[test]
fn an_unrecognized_version_has_no_payload_to_size() {
    // The payload was never located, because SPEC 3 forbids applying this
    // version's rules to a container declaring another. Same answer as asking
    // for the payload itself.
    let doc = "slipcase_version = \"9.4\"\n\n[payload]\nfile = \"report.pdf\"\n";
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, doc.as_bytes()),
        Member::new("report.pdf", b"x"),
    ]);
    let c = open(&bytes).unwrap();
    assert!(matches!(
        c.payload_size(),
        Err(Error::Unsupported(Unsupported::Version(v))) if v == "9.4"
    ));
}

// --- whether the payload can be read ---------------------------------------

/// A container whose members are deflated, built by an ordinary writer.
fn deflated_container() -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    w.start_file(METADATA_MEMBER, opts).unwrap();
    std::io::Write::write_all(&mut w, metadata("a.txt").as_bytes()).unwrap();
    w.start_file("a.txt", opts).unwrap();
    std::io::Write::write_all(&mut w, "a".repeat(4096).as_bytes()).unwrap();
    w.finish().unwrap().into_inner()
}

#[test]
fn a_payload_this_build_can_decode_is_readable() {
    let c = open(&container("report.pdf", b"%PDF-1.7 not really\n")).unwrap();
    assert!(c.check_payload_readable().is_ok());
    let c = open(&deflated_container()).unwrap();
    assert!(c.check_payload_readable().is_ok());
}

#[test]
fn the_name_and_the_check_can_be_asked_for_together() {
    // A shared borrow, like payload_name and payload_size. Anything describing
    // a payload before offering to open it asks all three at once, and `&mut`
    // on any of them makes that a borrow error rather than a line of code.
    let c = open(&container("report.pdf", b"1234")).unwrap();
    let line = match c.check_payload_readable() {
        Ok(()) => format!(
            "open {} ({} bytes)",
            c.payload_name(),
            c.payload_size().unwrap()
        ),
        Err(why) => format!("{} cannot be opened: {why}", c.payload_name()),
    };
    assert_eq!(line, "open report.pdf (4 bytes)");
}

#[test]
fn an_encrypted_payload_is_not_readable_and_the_container_still_conforms() {
    // SPEC 2.5 puts encryption outside the conformance question, so these two
    // answers are meant to differ. Folding one into the other would have this
    // build call a conformant container broken.
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"ciphertext").encrypted(),
    ]);
    assert!(slpc::validate(std::io::Cursor::new(bytes.clone()))
        .unwrap()
        .is_conformant());
    let c = open(&bytes).unwrap();
    assert!(matches!(
        c.check_payload_readable(),
        Err(Unsupported::Encrypted)
    ));
}

#[test]
fn a_payload_compressed_beyond_this_build_is_not_readable() {
    // Method 12 is bzip2, which the C-free feature set leaves out.
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"pretend this is bzip2").claims_method(12),
    ]);
    let c = open(&bytes).unwrap();
    assert!(matches!(
        c.check_payload_readable(),
        Err(Unsupported::Compression(12))
    ));
}

#[test]
fn an_unrecognised_version_has_no_payload_to_check() {
    // The payload was never located, because SPEC 3 forbids applying this
    // version's rules to a container declaring another. Same answer as asking
    // for the payload itself, and as asking for its size.
    let doc = "slipcase_version = \"9.4\"\n\n[payload]\nfile = \"a.txt\"\n";
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, doc.as_bytes()),
        Member::new("a.txt", b"x"),
    ]);
    let c = open(&bytes).unwrap();
    assert!(matches!(
        c.check_payload_readable(),
        Err(Unsupported::Version(v)) if v == "9.4"
    ));
}

#[test]
fn a_payload_that_is_both_encrypted_and_unreadable_reports_the_encryption() {
    // A member can be encrypted and carry a method this build lacks at once,
    // which is what every AES member is. The archive is asked about encryption
    // first, and this has to meet them in the same order or the two answers
    // name different reasons for one refusal. The fixture claims method 12
    // rather than AES's 99, because a well-formed AES member also carries an
    // extra field this suite does not stamp and the ZIP crate refuses the
    // header without it.
    let bytes = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new("a.txt", b"ciphertext")
            .encrypted()
            .claims_method(12),
    ]);
    let mut c = open(&bytes).unwrap();
    assert!(matches!(
        c.check_payload_readable(),
        Err(Unsupported::Encrypted)
    ));
    assert!(matches!(
        c.payload(),
        Err(Error::Unsupported(Unsupported::Encrypted))
    ));
}

#[test]
fn the_check_agrees_with_what_extraction_does() {
    // The check mirrors two tests the ZIP crate makes inside `payload`, and a
    // later version of that crate could add a third. This is what notices. The
    // direction that matters is a check saying yes where extraction says no,
    // since that is the answer a caller acts on.
    let unrecognised = "slipcase_version = \"9.4\"\n\n[payload]\nfile = \"a.txt\"\n";
    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("a stored payload", container("a.txt", b"plain\n")),
        ("a payload of zero length", container("empty.bin", b"")),
        ("a deflated payload", deflated_container()),
        (
            "an encrypted payload",
            raw_zip(&[
                Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
                Member::new("a.txt", b"ciphertext").encrypted(),
            ]),
        ),
        (
            "a payload compressed by method 12",
            raw_zip(&[
                Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
                Member::new("a.txt", b"pretend this is bzip2").claims_method(12),
            ]),
        ),
        (
            "a container declaring another version",
            raw_zip(&[
                Member::new(METADATA_MEMBER, unrecognised.as_bytes()),
                Member::new("a.txt", b"x"),
            ]),
        ),
    ];

    for (what, bytes) in fixtures {
        let mut c = open(&bytes).unwrap();
        // Both sides say the same sentence for the same refusal: `Error`
        // delegates its Display to the `Unsupported` it carries. Anything else
        // coming back from `payload` — an i/o error, say — reads as a
        // disagreement here, which is what it would be.
        let checked = c.check_payload_readable().err().map(|u| u.to_string());
        let extracted = c.payload().err().map(|e| e.to_string());
        assert_eq!(checked, extracted, "the two disagree about {what}");
    }
}

// --- the metadata of a container that will not open ------------------------

fn metadata_of(bytes: &[u8]) -> slpc::Result<slpc::toml_edit::DocumentMut> {
    slpc::metadata_of(std::io::Cursor::new(bytes.to_vec()))
}

#[test]
fn hands_back_the_document_of_a_conformant_container() {
    let doc = metadata_of(&container("report.pdf", b"x")).unwrap();
    assert_eq!(doc["payload"]["file"].as_str(), Some("report.pdf"));
}

#[test]
fn hands_back_the_document_when_payload_file_names_no_member() {
    // The container is not conformant and its metadata is perfectly readable.
    // `Container::read` cannot say both, which is why this exists.
    let bytes = raw_zip(&[Member::new(
        METADATA_MEMBER,
        metadata("absent.pdf").as_bytes(),
    )]);
    assert!(matches!(
        open(&bytes),
        Err(Error::Malformed(Malformed::NoPayloadMember(_)))
    ));

    let doc = metadata_of(&bytes).unwrap();
    assert_eq!(doc["payload"]["file"].as_str(), Some("absent.pdf"));
}

#[test]
fn hands_back_the_document_when_a_required_key_is_absent() {
    let bytes = raw_zip(&[Member::new(
        METADATA_MEMBER,
        b"title = \"a document with no version key\"\n",
    )]);
    assert!(matches!(
        open(&bytes),
        Err(Error::Malformed(Malformed::MissingKey(_)))
    ));

    let doc = metadata_of(&bytes).unwrap();
    assert_eq!(
        doc["title"].as_str(),
        Some("a document with no version key")
    );
}

#[test]
fn hands_back_the_document_when_payload_file_is_a_path() {
    let bytes = raw_zip(&[Member::new(
        METADATA_MEMBER,
        metadata("../etc/passwd").as_bytes(),
    )]);
    assert!(matches!(
        open(&bytes),
        Err(Error::Malformed(Malformed::PayloadName(
            NameError::Separator('/')
        )))
    ));
    assert_eq!(
        metadata_of(&bytes).unwrap()["payload"]["file"].as_str(),
        Some("../etc/passwd")
    );
}

#[test]
fn keeps_comments_and_key_order() {
    // The point of handing back a document rather than a struct: a program
    // showing a person what is in a container shows them what they wrote.
    let doc = "# who owns this\nslipcase_version = \"1.0\"\nzzz = 1\naaa = 2\n\n[payload]\nfile = \"absent.pdf\"\n";
    let bytes = raw_zip(&[Member::new(METADATA_MEMBER, doc.as_bytes())]);
    assert_eq!(metadata_of(&bytes).unwrap().to_string(), doc);
}

#[test]
fn refuses_what_spec_2_2_requires_of_the_member_itself() {
    // One metadata member, valid TOML, UTF-8. Everything past that is another
    // function's question.
    let no_member = raw_zip(&[Member::new("report.pdf", b"x")]);
    assert!(matches!(
        metadata_of(&no_member),
        Err(Error::Malformed(Malformed::NoMetadataMember))
    ));

    let two = raw_zip(&[
        Member::new(METADATA_MEMBER, metadata("a.txt").as_bytes()),
        Member::new(METADATA_MEMBER, metadata("b.txt").as_bytes()),
    ]);
    assert!(matches!(
        metadata_of(&two),
        Err(Error::Malformed(Malformed::DuplicateMetadataMember(2)))
    ));

    let not_toml = raw_zip(&[Member::new(METADATA_MEMBER, b"= not a document\n")]);
    assert!(matches!(
        metadata_of(&not_toml),
        Err(Error::Malformed(Malformed::MetadataNotToml(_)))
    ));

    let not_utf8 = raw_zip(&[Member::new(METADATA_MEMBER, b"title = \"\xff\xfe\"\n")]);
    assert!(matches!(
        metadata_of(&not_utf8),
        Err(Error::Malformed(Malformed::MetadataNotUtf8))
    ));
}

// ---------------------------------------------------------------------------
// SPEC 6: what a reader spends before it knows what it is holding
// ---------------------------------------------------------------------------

/// A metadata member over the bound is undetermined, not non-conformant.
///
/// Catches a reader that answers `NonConformant` when it runs out of its own
/// allowance. The bound belongs to the reader, so answering that would publish
/// this build's configuration as a property of somebody else's file, and two
/// readers with different bounds would disagree about conformance — which is
/// the disagreement SPEC 3 exists to prevent.
#[test]
fn a_metadata_member_over_the_bound_is_undetermined() {
    let bytes = container("report.pdf", b"payload\n");
    let mut limits = slpc::Limits::default();
    limits.metadata_bytes = 8;

    match slpc::validate_with(std::io::Cursor::new(bytes.clone()), limits) {
        Ok(slpc::Verdict::Undetermined(Unsupported::MetadataTooLarge { limit, .. })) => {
            assert_eq!(limit, 8);
        }
        other => panic!("expected undetermined over the bound, got {other:?}"),
    }

    // And the same container under a bound that fits is conformant, so the
    // test above is about the bound rather than about the fixture.
    assert!(slpc::validate(std::io::Cursor::new(bytes))
        .unwrap()
        .is_conformant());
}

/// The bound is applied to the bytes, not to the size the directory recorded.
///
/// Catches the obvious implementation of SPEC 6, which is to read the recorded
/// uncompressed size and refuse on that alone. Measured against `zip` 8.6: a
/// central directory rewritten to declare a hundred bytes for a member that
/// inflates to two hundred megabytes is not checked by anything, and the member
/// still arrives in full. Here the directory understates the member and the
/// only thing standing between the reader and the whole of it is the bound on
/// the read itself.
#[test]
fn a_lying_recorded_size_does_not_get_past_the_bound() {
    let mut bytes = container("report.pdf", b"payload\n");
    let member = metadata("report.pdf");

    // Rewrite the central directory's uncompressed size for the metadata
    // member to 1, which is under any bound worth setting.
    let at = find_central_size_field(&bytes, slpc::METADATA_MEMBER);
    bytes[at..at + 4].copy_from_slice(&1u32.to_le_bytes());

    let mut limits = slpc::Limits::default();
    limits.metadata_bytes = (member.len() - 1) as u64;

    match slpc::validate_with(std::io::Cursor::new(bytes), limits) {
        Ok(slpc::Verdict::Undetermined(Unsupported::MetadataTooLarge { declared, .. })) => {
            assert_eq!(declared, 1, "the recorded size is reported as recorded");
        }
        other => panic!("expected the read to stop at the bound, got {other:?}"),
    }
}

/// `metadata_of` is bounded too.
///
/// Catches bounding one entry point and not the other. Both reach the same
/// member by the same route and are exposed to the same thing, so a caller who
/// bounded `Container::read` and then called `metadata_of` would have bounded
/// nothing. The desktop viewer calls this one.
#[test]
fn metadata_of_is_bounded_as_well() {
    let bytes = container("report.pdf", b"payload\n");
    let mut limits = slpc::Limits::default();
    limits.metadata_bytes = 8;
    assert!(matches!(
        slpc::metadata_of_with(std::io::Cursor::new(bytes), limits),
        Err(Error::Unsupported(Unsupported::MetadataTooLarge { .. }))
    ));
}

/// Where the metadata member's recorded uncompressed size sits in the archive.
fn find_central_size_field(bytes: &[u8], name: &str) -> usize {
    let eocd = bytes
        .windows(4)
        .rposition(|w| w == 0x0605_4B50u32.to_le_bytes())
        .expect("an end of central directory record");
    let mut at = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    loop {
        assert_eq!(
            bytes[at..at + 4],
            0x0201_4B50u32.to_le_bytes(),
            "walked off the central directory without finding {name}"
        );
        let n = u16::from_le_bytes(bytes[at + 28..at + 30].try_into().unwrap()) as usize;
        let e = u16::from_le_bytes(bytes[at + 30..at + 32].try_into().unwrap()) as usize;
        let c = u16::from_le_bytes(bytes[at + 32..at + 34].try_into().unwrap()) as usize;
        if &bytes[at + 46..at + 46 + n] == name.as_bytes() {
            return at + 24;
        }
        at += 46 + n + e + c;
    }
}

// ---------------------------------------------------------------------------
// The mode the archive recorded, for saying and not for applying
// ---------------------------------------------------------------------------

/// A recorded mode comes back, with the file-type bits masked off.
///
/// Catches an accessor that hands back the whole mode. A caller asking whether
/// a payload was executable tests `& 0o111`, and `0o100_755 & 0o111` and
/// `0o755 & 0o111` agree — but a caller printing the answer, or comparing it to
/// a mode of its own, would see `0o100755` and be wrong about what it means.
#[test]
fn a_recorded_mode_comes_back_without_its_file_type_bits() {
    let bytes = raw_zip(&[
        Member::new(slpc::METADATA_MEMBER, metadata("build.sh").as_bytes()),
        Member::new("build.sh", b"#!/bin/sh\n").with_mode(0o100_755),
    ]);
    assert_eq!(open(&bytes).unwrap().payload_mode().unwrap(), Some(0o755));
}

/// A setuid payload is reported as setuid and not swallowed.
///
/// Catches masking to `0o777` rather than `0o7777`. SPEC 2.5 lets a container
/// record this and SPEC 3 forbids applying it, and the whole value of the
/// accessor is that something can say so.
#[test]
fn a_setuid_payload_is_reported() {
    let bytes = raw_zip(&[
        Member::new(slpc::METADATA_MEMBER, metadata("tool").as_bytes()),
        Member::new("tool", b"\x7fELF\n").with_mode(0o104_755),
    ]);
    assert_eq!(open(&bytes).unwrap().payload_mode().unwrap(), Some(0o4755));
}

/// A container recording no mode says nothing, rather than saying 0o664.
///
/// This is the defect the accessor exists to avoid, and it is one the obvious
/// implementation walks into. The ZIP crate's `unix_mode` invents a mode for an
/// archive made on DOS — `S_IFREG | 0o664`, or `0o444` where the read-only bit
/// is set — because for its purposes a guess beats nothing. Here a guess is
/// worse than nothing: a card saying *the extracted copy will not be
/// executable* about a container that never said whether it was is a sentence
/// nobody can check, and every container written by a Windows tool would get
/// one.
#[test]
fn a_container_recording_no_mode_says_nothing() {
    let bytes = raw_zip(&[
        Member::new(slpc::METADATA_MEMBER, metadata("report.pdf").as_bytes()).dos_made(),
        Member::new("report.pdf", b"%PDF\n").dos_made(),
    ]);
    assert_eq!(open(&bytes).unwrap().payload_mode().unwrap(), None);

    // The container still opens, which is the other half: SPEC 2.3 requires a
    // regular file entry, and `EntryKind` answers that one by defaulting to
    // regular where the archive is silent. Only the permissions are unknowable,
    // and only they go quiet.
    assert_eq!(open(&bytes).unwrap().payload_name(), "report.pdf");
}

// ---------------------------------------------------------------------------
// SPEC 2.1: one file, one answer about which members it holds
// ---------------------------------------------------------------------------

/// An archive with two members named `report.pdf`, plus whatever the caller
/// does to the end of central directory record afterwards.
fn duplicate_payload_archive() -> Vec<u8> {
    raw_zip(&[
        Member::new(slpc::METADATA_MEMBER, metadata("report.pdf").as_bytes()),
        Member::new("report.pdf", b"FIRST\n"),
        Member::new("report.pdf", b"SECOND\n"),
    ])
}

/// Where the end of central directory record starts.
fn eocd_at(bytes: &[u8]) -> usize {
    bytes
        .windows(4)
        .rposition(|w| w == 0x0605_4B50u32.to_le_bytes())
        .expect("an end of central directory record")
}

/// The two entry counts must agree, and a duplicate hidden behind them is found.
///
/// **The defect this catches is the one SPEC 3's enumeration rule exists to
/// prevent, arriving through the rule's own implementation.** Byte 8 of the
/// record is *entries on this disk* and byte 10 is *entries in total*; this
/// crate counted the total and its ZIP dependency counts the ones on this disk.
/// Declaring three and two therefore hid the third member from the duplicate
/// check while leaving it in the archive the payload is read from — a
/// conformant verdict over one set of members and a payload served from
/// another. Measured 2026-08-27. Set both fields to 3 and the duplicate is
/// caught the ordinary way, which is the assertion below it.
#[test]
fn the_two_entry_counts_must_agree() {
    let mut bytes = duplicate_payload_archive();
    let at = eocd_at(&bytes);
    bytes[at + 10..at + 12].copy_from_slice(&2u16.to_le_bytes());

    let verdict = slpc::validate(std::io::Cursor::new(bytes.clone())).unwrap();
    assert!(!verdict.is_conformant(), "{verdict}");
    assert!(verdict.to_string().contains("single-disk"), "{verdict}");

    // Honest counts: rejected, and for the duplicate rather than for the record.
    bytes[at + 10..at + 12].copy_from_slice(&3u16.to_le_bytes());
    let verdict = slpc::validate(std::io::Cursor::new(bytes)).unwrap();
    assert!(verdict.to_string().contains("report.pdf"), "{verdict}");
}

/// The record must be the last thing in the file, its comment included.
///
/// Catches a reader that takes the last signature it finds without checking
/// what the record claims about its own length. The ZIP crate checks, and keeps
/// looking when the answer does not fit, so a file with a second record whose
/// comment length overruns leaves the two halves of this crate reading two
/// different central directories.
#[test]
fn the_record_must_end_the_file() {
    let mut bytes = duplicate_payload_archive();
    let at = eocd_at(&bytes);
    bytes[at + 20..at + 22].copy_from_slice(&0xFFFFu16.to_le_bytes());

    let verdict = slpc::validate(std::io::Cursor::new(bytes)).unwrap();
    assert!(!verdict.is_conformant(), "{verdict}");
    assert!(
        verdict.to_string().contains("does not end the file"),
        "{verdict}"
    );
}

/// An archive split across disks is refused.
///
/// Catches the third field in the same record that decides which members a
/// reader sees. Nothing produces a multi-disk container and nothing here could
/// read one, so the honest answer is to say so rather than to read whichever
/// part happens to be in front of us.
#[test]
fn a_multi_disk_archive_is_refused() {
    // An otherwise ordinary container, so the disk field is the only thing
    // there is to reject it for. Built with the duplicate removed, and asserted
    // conformant first, or this would pass for the wrong reason.
    let good = container("report.pdf", b"payload\n");
    assert!(slpc::validate(std::io::Cursor::new(good.clone()))
        .unwrap()
        .is_conformant());

    for field in [4usize, 6] {
        let mut bytes = good.clone();
        let at = eocd_at(&bytes);
        bytes[at + field..at + field + 2].copy_from_slice(&1u16.to_le_bytes());

        let verdict = slpc::validate(std::io::Cursor::new(bytes)).unwrap();
        assert!(!verdict.is_conformant(), "field {field}: {verdict}");
        assert!(verdict.to_string().contains("single-disk"), "{verdict}");
    }
}

/// The Zip64 record is consulted on the same signal the ZIP crate uses.
///
/// **The third way to split this crate's two directory readers, and the one the
/// other tests here do not reach.** The ZIP crate goes looking for a Zip64 end
/// of central directory record when *any* of the plain record's three fields is
/// saturated, the directory size included; this crate looked at only the count
/// and the offset. So a plain record that is complete, consistent, and merely
/// understates the entry count — with the size field carrying the only sentinel
/// — sent the two readers to different records, and the member visible to just
/// one of them was a duplicate payload.
///
/// Change the gate back to `count || offset` and this reports conformant.
#[test]
fn the_zip64_record_is_consulted_on_the_size_sentinel() {
    let plain = duplicate_payload_archive();
    let at = eocd_at(&plain);
    let count = u16::from_le_bytes(plain[at + 10..at + 12].try_into().unwrap());
    let size = u32::from_le_bytes(plain[at + 12..at + 16].try_into().unwrap());
    let offset = u32::from_le_bytes(plain[at + 16..at + 20].try_into().unwrap());

    // Everything up to the plain record, then a Zip64 record holding the truth,
    // its locator, and a plain record that understates the count and marks
    // itself Zip64 with the size field alone.
    let mut bytes = plain[..at].to_vec();
    let z64_at = bytes.len() as u64;

    bytes.extend_from_slice(&0x0606_4B50u32.to_le_bytes());
    bytes.extend_from_slice(&44u64.to_le_bytes()); // the rest of this record
    bytes.extend_from_slice(&0x031Eu16.to_le_bytes());
    bytes.extend_from_slice(&45u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // this disk
    bytes.extend_from_slice(&0u32.to_le_bytes()); // disk with the directory
    bytes.extend_from_slice(&u64::from(count).to_le_bytes()); // entries here
    bytes.extend_from_slice(&u64::from(count).to_le_bytes()); // entries in total
    bytes.extend_from_slice(&u64::from(size).to_le_bytes());
    bytes.extend_from_slice(&u64::from(offset).to_le_bytes());

    bytes.extend_from_slice(&0x0706_4B50u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&z64_at.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());

    bytes.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&(count - 1).to_le_bytes()); // one short, on purpose
    bytes.extend_from_slice(&(count - 1).to_le_bytes());
    bytes.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // the only sentinel
    bytes.extend_from_slice(&offset.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    let verdict = slpc::validate(std::io::Cursor::new(bytes)).unwrap();
    assert!(!verdict.is_conformant(), "{verdict}");
    assert!(
        verdict.to_string().contains("report.pdf"),
        "the duplicate the Zip64 record holds should be what is reported: {verdict}"
    );
}
