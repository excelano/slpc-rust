// The tool as a caller meets it: argv in, files and exit codes out.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use testsupport::mark_as_downloaded;

const BIN: &str = env!("CARGO_BIN_EXE_slipcase");

/// A working directory that cleans itself up.
struct Sandbox(tempfile::TempDir);

impl Sandbox {
    fn new() -> Self {
        Self(tempfile::tempdir().unwrap())
    }
    fn path(&self) -> &Path {
        self.0.path()
    }
    fn file(&self, name: &str, body: &[u8]) -> PathBuf {
        let p = self.path().join(name);
        std::fs::write(&p, body).unwrap();
        p
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .args(args)
            .current_dir(self.path())
            .output()
            .unwrap()
    }
    fn pipe(&self, args: &[&str], stdin: &[u8]) -> Output {
        use std::io::Write;
        let mut child = Command::new(BIN)
            .args(args)
            .current_dir(self.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A tool that refuses before reading its input is a case under test
        // here, and the broken pipe that produces is the child behaving, not a
        // failure. Every fixture is far under a pipe buffer, so a short write
        // cannot happen for any other reason.
        match child.stdin.take().unwrap().write_all(stdin) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {}
            Err(e) => panic!("writing to the child: {e}"),
        }
        child.wait_with_output().unwrap()
    }
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}
fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// --- pack ------------------------------------------------------------------

#[test]
fn packs_and_unpacks_a_round_trip() {
    let s = Sandbox::new();
    s.file("report.pdf", b"the payload\n");

    let o = s.run(&["pack", "report.pdf"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(s.path().join("report.pdf.slpc").exists());

    std::fs::create_dir(s.path().join("out")).unwrap();
    let o = s.run(&["unpack", "report.pdf.slpc", "--dest", "out"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(
        std::fs::read(s.path().join("out/report.pdf")).unwrap(),
        b"the payload\n"
    );
}

#[test]
fn pack_takes_extra_keys_from_a_meta_file() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.file("m.toml", b"title = \"Q3\"\n\n[custom]\nowner = \"ops\"\n");

    assert_eq!(code(&s.run(&["pack", "a.txt", "--meta", "m.toml"])), 0);
    let o = s.run(&["info", "a.txt.slpc"]);
    assert!(out(&o).contains("title = \"Q3\""), "{}", out(&o));
    assert!(out(&o).contains("owner = \"ops\""));
    assert!(out(&o).contains("slipcase_version = \"1.0\""));
}

#[test]
fn pack_refuses_a_meta_file_that_names_a_different_payload() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.file("m.toml", b"[payload]\nfile = \"somethingelse.txt\"\n");

    let o = s.run(&["pack", "a.txt", "--meta", "m.toml"]);
    assert_eq!(code(&o), 1);
    assert!(err(&o).contains("payload.file"), "{}", err(&o));
    // And nothing was left on disk, because the write goes through a rename.
    assert!(!s.path().join("a.txt.slpc").exists());
}

#[test]
fn pack_from_standard_input_needs_a_name() {
    let s = Sandbox::new();
    let o = s.pipe(&["pack", "-"], b"streamed\n");
    assert_eq!(code(&o), 1);
    assert!(err(&o).contains("--name"), "{}", err(&o));
}

#[test]
fn pack_from_standard_input_with_a_name() {
    let s = Sandbox::new();
    let o = s.pipe(&["pack", "-", "--name", "streamed.txt"], b"from a pipe\n");
    assert_eq!(code(&o), 0, "{}", err(&o));

    let o = s.run(&["unpack", "streamed.txt.slpc"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert_eq!(
        std::fs::read(s.path().join("streamed.txt")).unwrap(),
        b"from a pipe\n"
    );
}

#[test]
fn pack_refuses_a_payload_name_that_is_not_a_plain_filename() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    let o = s.run(&["pack", "a.txt", "--name", "../escape.txt", "-o", "out.slpc"]);
    assert_eq!(code(&o), 1);
    assert!(err(&o).contains("SPEC 2.3"), "{}", err(&o));
}

#[test]
fn nothing_is_overwritten_without_force() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.file("a.txt.slpc", b"in the way");

    let o = s.run(&["pack", "a.txt"]);
    assert_eq!(code(&o), 1);
    assert!(err(&o).contains("--force"), "{}", err(&o));
    assert_eq!(
        std::fs::read(s.path().join("a.txt.slpc")).unwrap(),
        b"in the way"
    );

    assert_eq!(code(&s.run(&["pack", "a.txt", "--force"])), 0);
    assert_ne!(
        std::fs::read(s.path().join("a.txt.slpc")).unwrap(),
        b"in the way"
    );
}

// --- unpack ----------------------------------------------------------------

#[test]
fn unpack_writes_the_metadata_only_when_asked() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.run(&["pack", "a.txt"]);
    std::fs::remove_file(s.path().join("a.txt")).unwrap();

    assert_eq!(code(&s.run(&["unpack", "a.txt.slpc"])), 0);
    assert!(!s.path().join("slipcase.metadata.toml").exists());

    std::fs::remove_file(s.path().join("a.txt")).unwrap();
    assert_eq!(code(&s.run(&["unpack", "a.txt.slpc", "--metadata"])), 0);
    assert!(s.path().join("slipcase.metadata.toml").exists());
}

#[test]
fn unpack_refuses_a_destination_that_is_not_there() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.run(&["pack", "a.txt"]);
    let o = s.run(&["unpack", "a.txt.slpc", "--dest", "nowhere"]);
    assert_eq!(code(&o), 1);
    assert!(err(&o).contains("nowhere"), "{}", err(&o));
}

// --- info and validate -----------------------------------------------------

#[test]
fn info_prints_the_metadata_member_verbatim() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.file("m.toml", b"# a comment worth keeping\ntitle = \"kept\"\n");
    s.run(&["pack", "a.txt", "--meta", "m.toml"]);

    let o = s.run(&["info", "a.txt.slpc"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(
        out(&o).starts_with("# a comment worth keeping\n"),
        "{}",
        out(&o)
    );
}

#[test]
fn info_reads_standard_input() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.run(&["pack", "a.txt"]);
    let bytes = std::fs::read(s.path().join("a.txt.slpc")).unwrap();

    let o = s.pipe(&["info", "-"], &bytes);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("file = \"a.txt\""), "{}", out(&o));
}

#[test]
fn validate_reports_conformance() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.run(&["pack", "a.txt"]);

    let o = s.run(&["validate", "a.txt.slpc"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&o).contains("conformant"), "{}", out(&o));
}

#[test]
fn validate_rejects_something_that_is_not_a_container() {
    let s = Sandbox::new();
    s.file("not.slpc", b"just some bytes, not a ZIP at all");
    let o = s.run(&["validate", "not.slpc"]);
    assert_eq!(code(&o), 1);
    assert!(!err(&o).is_empty());
}

#[test]
fn a_missing_file_is_bad_input_and_not_a_bad_command_line() {
    let s = Sandbox::new();
    let o = s.run(&["validate", "absent.slpc"]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(err(&o).contains("absent.slpc"), "{}", err(&o));
}

/// A container declaring another version, which the library refuses to write.
fn future_container() -> Vec<u8> {
    use std::io::Write;
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("slipcase.metadata.toml", opts).unwrap();
    w.write_all(b"slipcase_version = \"9.4\"\n\n[payload]\nfile = \"a.txt\"\n")
        .unwrap();
    w.start_file("a.txt", opts).unwrap();
    w.write_all(b"from the future\n").unwrap();
    w.finish().unwrap().into_inner()
}

#[test]
fn a_container_this_build_cannot_speak_to_is_neither_conformant_nor_not() {
    // SPEC 2.4 puts another version outside the conformance question rather
    // than failing it, and SPEC 3 forbids answering either way. Exit 3 is how
    // that is said to a caller branching on the status.
    let s = Sandbox::new();
    s.file("future.slpc", &future_container());

    let o = s.run(&["validate", "future.slpc"]);
    assert_eq!(code(&o), 3, "{}", err(&o));
    assert!(err(&o).contains("9.4"), "{}", err(&o));
    assert!(
        !err(&o).to_lowercase().contains("not conformant"),
        "must not call it non-conformant: {}",
        err(&o)
    );
}

#[test]
fn a_rejected_container_and_a_missing_one_are_not_exit_three() {
    let s = Sandbox::new();
    s.file("not.slpc", b"not a ZIP at all");
    assert_eq!(code(&s.run(&["validate", "not.slpc"])), 1);
    assert_eq!(code(&s.run(&["validate", "absent.slpc"])), 1);
}

// --- the command line itself -----------------------------------------------

#[test]
fn a_bad_command_line_is_two() {
    let s = Sandbox::new();
    assert_eq!(code(&s.run(&["validate", "--nonsense", "a.slpc"])), 2);
    assert_eq!(code(&s.run(&["frobnicate"])), 2);
    assert_eq!(code(&s.run(&[])), 2);
    assert_eq!(code(&s.run(&["validate"])), 2);
}

#[test]
fn help_states_the_exit_codes() {
    let s = Sandbox::new();
    let o = s.run(&["--help"]);
    assert_eq!(code(&o), 0);
    let text = out(&o);
    for line in [
        "Exit codes:",
        "0  success",
        "1  bad input",
        "2  bad command line",
    ] {
        assert!(text.contains(line), "missing {line:?} in:\n{text}");
    }
    assert!(text.contains("`-` names standard input"));
}

#[test]
fn version_and_help_have_both_spellings() {
    let s = Sandbox::new();
    for flags in [["--version"], ["-V"]] {
        let o = s.run(&flags);
        assert_eq!(code(&o), 0);
        assert!(out(&o).contains("slipcase"));
    }
    for flags in [["--help"], ["-h"]] {
        assert_eq!(code(&s.run(&flags)), 0);
    }
}

// --- repack ----------------------------------------------------------------

/// A container the tool itself cannot write: one carrying a member and a
/// metadata key that mean nothing to it, which is what SPEC 3 requires a
/// rewrite to preserve.
fn container_with_extras() -> Vec<u8> {
    use std::io::Write;
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    w.start_file("slipcase.metadata.toml", opts).unwrap();
    w.write_all(
        b"# hand written\nslipcase_version = \"1.0\"\ntitle = \"the quarterly\"\n\n[payload]\nfile = \"a.txt\"\n",
    )
    .unwrap();
    w.start_file("a.txt", opts).unwrap();
    w.write_all(b"first\n").unwrap();
    w.start_file("notes.md", opts).unwrap();
    w.write_all(b"a member nothing here understands\n").unwrap();
    w.finish().unwrap().into_inner()
}

/// Every member of a container on disk, in order.
fn member_names(path: &Path) -> Vec<String> {
    let f = std::fs::File::open(path).unwrap();
    let mut a = zip::ZipArchive::new(f).unwrap();
    (0..a.len())
        .map(|i| a.by_index_raw(i).unwrap().name().to_owned())
        .collect()
}

#[test]
fn repack_changes_the_metadata_in_place() {
    let s = Sandbox::new();
    s.file("c.slpc", &container_with_extras());
    s.file(
        "m.toml",
        b"slipcase_version = \"1.0\"\ntitle = \"revised\"\n\n[payload]\nfile = \"a.txt\"\n",
    );

    let o = s.run(&["repack", "--meta", "m.toml", "c.slpc"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(out(&s.run(&["info", "c.slpc"])).contains("revised"));
    assert_eq!(code(&s.run(&["validate", "c.slpc"])), 0);
}

#[test]
fn repack_replaces_the_payload_and_moves_payload_file_with_it() {
    let s = Sandbox::new();
    s.file("c.slpc", &container_with_extras());
    s.file("b.txt", b"second\n");

    let o = s.run(&["repack", "--payload", "b.txt", "c.slpc"]);
    assert_eq!(code(&o), 0, "{}", err(&o));

    // The old member is replaced rather than joined, and it keeps its place.
    assert_eq!(
        member_names(&s.path().join("c.slpc")),
        ["slipcase.metadata.toml", "b.txt", "notes.md"]
    );
    assert!(out(&s.run(&["info", "c.slpc"])).contains("file = \"b.txt\""));

    std::fs::create_dir(s.path().join("out")).unwrap();
    assert_eq!(code(&s.run(&["unpack", "c.slpc", "--dest", "out"])), 0);
    assert_eq!(
        std::fs::read(s.path().join("out/b.txt")).unwrap(),
        b"second\n"
    );
}

#[test]
fn repack_preserves_what_it_does_not_recognise() {
    // SPEC 3: members an implementation does not recognize survive a rewrite,
    // and so do metadata keys. Nothing else in this suite can reach that
    // requirement, because nothing else changes a container that already
    // exists.
    let s = Sandbox::new();
    s.file("c.slpc", &container_with_extras());
    s.file("b.txt", b"second\n");

    assert_eq!(code(&s.run(&["repack", "--payload", "b.txt", "c.slpc"])), 0);

    let names = member_names(&s.path().join("c.slpc"));
    assert!(names.contains(&"notes.md".to_owned()), "{names:?}");

    let metadata = out(&s.run(&["info", "c.slpc"]));
    assert!(metadata.contains("title = \"the quarterly\""), "{metadata}");
    assert!(metadata.starts_with("# hand written\n"), "{metadata}");

    // And the member itself, byte for byte.
    let f = std::fs::File::open(s.path().join("c.slpc")).unwrap();
    let mut a = zip::ZipArchive::new(f).unwrap();
    let mut notes = String::new();
    std::io::Read::read_to_string(&mut a.by_name("notes.md").unwrap(), &mut notes).unwrap();
    assert_eq!(notes, "a member nothing here understands\n");
}

#[test]
fn repack_leaves_the_container_alone_when_it_refuses() {
    let s = Sandbox::new();
    s.file("c.slpc", &container_with_extras());
    let before = std::fs::read(s.path().join("c.slpc")).unwrap();
    s.file("b.txt", b"second\n");

    // A name another member already carries, which SPEC 2.1 forbids.
    let o = s.run(&[
        "repack",
        "--payload",
        "b.txt",
        "--name",
        "notes.md",
        "c.slpc",
    ]);
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(err(&o).contains("notes.md"), "{}", err(&o));

    assert_eq!(std::fs::read(s.path().join("c.slpc")).unwrap(), before);
    // And nothing half written left beside it.
    let left: Vec<_> = std::fs::read_dir(s.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left.len(), 2, "{left:?}");
}

#[test]
fn repack_needs_something_to_change() {
    let s = Sandbox::new();
    s.file("c.slpc", &container_with_extras());
    assert_eq!(code(&s.run(&["repack", "c.slpc"])), 2);
}

#[test]
fn repack_moves_a_container_through_a_pipeline() {
    let s = Sandbox::new();
    s.file("c.slpc", &container_with_extras());

    // Metadata in on standard input, the container out on standard output.
    let o = s.pipe(
        &["repack", "--meta", "-", "c.slpc", "-o", "-"],
        b"slipcase_version = \"1.0\"\ntitle = \"piped\"\n\n[payload]\nfile = \"a.txt\"\n",
    );
    assert_eq!(code(&o), 0, "{}", err(&o));

    s.file("piped.slpc", &o.stdout);
    assert_eq!(code(&s.run(&["validate", "piped.slpc"])), 0);
    assert!(out(&s.run(&["info", "piped.slpc"])).contains("piped"));
    // The source was not touched, because a destination was named.
    assert!(out(&s.run(&["info", "c.slpc"])).contains("the quarterly"));
}

#[test]
fn repack_from_standard_input_needs_somewhere_to_write() {
    let s = Sandbox::new();
    s.file(
        "m.toml",
        b"slipcase_version = \"1.0\"\n\n[payload]\nfile = \"a.txt\"\n",
    );
    let o = s.pipe(
        &["repack", "--meta", "m.toml", "-"],
        &container_with_extras(),
    );
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(err(&o).contains("-o"), "{}", err(&o));
}

#[test]
fn repack_will_not_read_standard_input_twice() {
    let s = Sandbox::new();
    s.file("c.slpc", &container_with_extras());
    let o = s.pipe(
        &[
            "repack",
            "--meta",
            "-",
            "--payload",
            "-",
            "--name",
            "b.txt",
            "c.slpc",
        ],
        b"x",
    );
    assert_eq!(code(&o), 1, "{}", err(&o));
    assert!(err(&o).contains("standard input"), "{}", err(&o));
}

#[test]
fn repack_of_a_version_this_build_cannot_speak_to_is_no_verdict() {
    let s = Sandbox::new();
    s.file("future.slpc", &future_container());
    s.file(
        "m.toml",
        b"slipcase_version = \"1.0\"\n\n[payload]\nfile = \"a.txt\"\n",
    );

    let o = s.run(&["repack", "--meta", "m.toml", "future.slpc"]);
    assert_eq!(code(&o), 3, "{}", err(&o));
    assert!(err(&o).contains("9.4"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn repack_keeps_the_containers_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let s = Sandbox::new();
    let c = s.file("c.slpc", &container_with_extras());
    s.file("b.txt", b"second\n");
    std::fs::set_permissions(&c, std::fs::Permissions::from_mode(0o640)).unwrap();

    assert_eq!(code(&s.run(&["repack", "--payload", "b.txt", "c.slpc"])), 0);

    let mode = std::fs::metadata(&c).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640, "a rename must not narrow a container to 0600");
}

#[cfg(unix)]
#[test]
fn repack_through_a_symlink_changes_the_container_and_not_the_link() {
    let s = Sandbox::new();
    s.file("c.slpc", &container_with_extras());
    s.file("b.txt", b"second\n");
    std::os::unix::fs::symlink("c.slpc", s.path().join("link.slpc")).unwrap();

    let o = s.run(&["repack", "--payload", "b.txt", "link.slpc"]);
    assert_eq!(code(&o), 0, "{}", err(&o));

    assert!(
        s.path().join("link.slpc").is_symlink(),
        "the link was replaced by a file"
    );
    assert!(out(&s.run(&["info", "c.slpc"])).contains("file = \"b.txt\""));
}

// --- what the files come out as -------------------------------------------

#[cfg(unix)]
#[test]
fn what_it_writes_is_readable_by_whoever_the_umask_said() {
    // Against a file this test process creates the ordinary way, rather than
    // against a number: the answer depends on the umask, and hard-coding 0644
    // would pass under one and be wrong under another. A container is a file
    // like any other, and it should come out like one.
    use std::os::unix::fs::PermissionsExt;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

    let s = Sandbox::new();
    let reference = s.file("reference.txt", b"what an ordinary file gets");
    let want = mode(&reference);

    s.file("a.txt", b"x");
    assert_eq!(code(&s.run(&["pack", "a.txt"])), 0);
    assert_eq!(
        mode(&s.path().join("a.txt.slpc")),
        want,
        "a packed container"
    );

    std::fs::remove_file(s.path().join("a.txt")).unwrap();
    assert_eq!(code(&s.run(&["unpack", "a.txt.slpc", "--metadata"])), 0);
    assert_eq!(mode(&s.path().join("a.txt")), want, "an unpacked payload");
    assert_eq!(
        mode(&s.path().join("slipcase.metadata.toml")),
        want,
        "an unpacked metadata member"
    );

    // A repack takes the container's own permissions instead, since it is
    // replacing a file rather than creating one.
    std::fs::set_permissions(
        s.path().join("a.txt.slpc"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    s.file("b.txt", b"y");
    assert_eq!(
        code(&s.run(&["repack", "--payload", "b.txt", "a.txt.slpc"])),
        0
    );
    assert_eq!(
        mode(&s.path().join("a.txt.slpc")),
        0o600,
        "a repacked container"
    );
}

#[test]
fn the_permission_probe_leaves_nothing_behind() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    assert_eq!(code(&s.run(&["pack", "a.txt"])), 0);

    let mut left: Vec<String> = std::fs::read_dir(s.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["a.txt", "a.txt.slpc"]);
}

// --- provenance ------------------------------------------------------------

/// The defect this catches is the one that shipped: `slipcase unpack` on a
/// downloaded container writing a payload that says nothing about where it came
/// from, so that whatever opens it next sees a file this machine made and the
/// warning the container would have raised never appears.
///
/// Through the binary rather than the library, because the library was never
/// what was wrong — nothing called it. Remove the `carry` from `unpack` and
/// this fails while every test in `slpc` still passes.
#[test]
fn unpack_carries_where_the_container_came_from_onto_the_payload() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.run(&["pack", "a.txt"]);
    std::fs::remove_file(s.path().join("a.txt")).unwrap();

    if !mark_as_downloaded(&s.path().join("a.txt.slpc")) {
        eprintln!("skipped: this filesystem will not hold a provenance mark");
        return;
    }

    assert_eq!(code(&s.run(&["unpack", "a.txt.slpc"])), 0);
    assert!(
        slpc::provenance::arrived_from_elsewhere(&s.path().join("a.txt")),
        "the unpacked payload does not say it arrived from anywhere, so \
         unpacking a downloaded container laundered it"
    );
}

/// The defect this catches is the repair above marking everything it touches.
/// A warning a person sees on every file is one they stop reading.
#[test]
fn unpack_of_a_container_made_here_marks_nothing() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.run(&["pack", "a.txt"]);
    std::fs::remove_file(s.path().join("a.txt")).unwrap();

    assert_eq!(code(&s.run(&["unpack", "a.txt.slpc"])), 0);
    assert!(!slpc::provenance::arrived_from_elsewhere(
        &s.path().join("a.txt")
    ));
}

/// A container read from standard input has no source to read a mark from, and
/// unpacking one is not a failure. The defect this catches is the `is_dash`
/// guard going missing, which would make every piped unpack an error about a
/// file that is not there.
#[test]
fn unpack_from_standard_input_is_not_a_provenance_failure() {
    let s = Sandbox::new();
    s.file("a.txt", b"x");
    s.run(&["pack", "a.txt"]);
    let container = std::fs::read(s.path().join("a.txt.slpc")).unwrap();
    std::fs::remove_file(s.path().join("a.txt")).unwrap();

    let o = s.pipe(&["unpack", "-"], &container);
    assert_eq!(code(&o), 0, "{}", err(&o));
    assert!(s.path().join("a.txt").exists());
}

/// `validate` escapes a bidirectional override before printing the name.
///
/// Catches the line going out raw. A payload called `report<U+202E>fdp.exe`
/// reads as `report.pdf` in every terminal that applies the override, and this
/// line is what somebody reads to decide what a container holds. SPEC 3
/// requires the escaping and SPEC 2.3 deliberately permits the name, so the
/// container here is conformant and the output is the only thing that changes.
#[test]
fn validate_escapes_a_bidi_override_in_the_payload_name() {
    let s = Sandbox::new();
    let o = s.pipe(
        &[
            "pack",
            "-",
            "--name",
            "report\u{202E}fdp.exe",
            "-o",
            "c.slpc",
        ],
        b"MZ\n",
    );
    assert_eq!(code(&o), 0, "{}", err(&o));

    let o = s.run(&["validate", "c.slpc"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let line = out(&o);
    assert!(
        line.contains(r"report\u{202E}fdp.exe"),
        "the override went out raw: {line:?}"
    );
    assert!(
        !line.contains('\u{202E}'),
        "the override went out raw: {line:?}"
    );
}

/// `info` redirected reproduces the member byte for byte.
///
/// The other half of the split, and the one that would break quietly. `info`
/// into a file or a pipe is how a caller gets the metadata out, so escaping
/// there would hand them a document the container does not contain. Only a
/// terminal gets the escaped form, and this harness is a pipe.
#[test]
fn info_redirected_is_the_member_and_not_a_rendering() {
    let s = Sandbox::new();
    let o = s.pipe(
        &[
            "pack",
            "-",
            "--name",
            "report\u{202E}fdp.exe",
            "-o",
            "c.slpc",
        ],
        b"MZ\n",
    );
    assert_eq!(code(&o), 0, "{}", err(&o));

    let o = s.run(&["info", "c.slpc"]);
    assert_eq!(code(&o), 0, "{}", err(&o));
    let doc = out(&o);
    assert!(
        doc.contains('\u{202E}'),
        "a redirected dump was escaped: {doc:?}"
    );

    // And it is the member, which is the claim worth making: what comes out
    // parses back to the name that went in.
    let parsed: slpc::toml_edit::DocumentMut = doc.parse().unwrap();
    assert_eq!(
        parsed["payload"]["file"].as_str().unwrap(),
        "report\u{202E}fdp.exe"
    );
}
