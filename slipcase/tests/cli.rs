// The tool as a caller meets it: argv in, files and exit codes out.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

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
