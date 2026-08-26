// Putting a container on disk: the `fs` feature.
//
// Everything here needs a real filesystem — a rename across one is the whole
// mechanism — so these are files in a temporary directory rather than cursors.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![cfg(feature = "fs")]

mod support;

use std::io::Write;
use std::path::Path;

use support::container;

use slpc::{Destination, Error};

/// Everything in one temporary directory, removed when the test ends.
fn sandbox() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// What is in a directory, sorted, so a leftover is visible.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn nothing_appears_until_it_is_committed() {
    let s = sandbox();
    let path = s.path().join("report.pdf.slpc");

    let mut d = Destination::new(&path, false).unwrap();
    d.writer()
        .write_all(&container("report.pdf", b"x"))
        .unwrap();
    assert!(!path.exists(), "the destination appeared before the commit");

    d.commit().unwrap();
    assert!(path.exists());
    assert_eq!(entries(s.path()), ["report.pdf.slpc"]);
}

#[test]
fn dropping_one_leaves_nothing_behind() {
    // Including the probe, which is created and removed while the destination
    // is being reserved rather than at the end.
    let s = sandbox();
    let mut d = Destination::new(s.path().join("c.slpc"), false).unwrap();
    d.writer().write_all(b"half a container").unwrap();
    drop(d);
    assert!(entries(s.path()).is_empty(), "{:?}", entries(s.path()));
}

#[test]
fn refuses_an_existing_file_unless_forced() {
    let s = sandbox();
    let path = s.path().join("c.slpc");
    std::fs::write(&path, b"already here").unwrap();

    let e = Destination::new(&path, false).unwrap_err();
    assert!(
        matches!(&e, Error::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists),
        "{e}"
    );
    assert!(e.to_string().contains("c.slpc"), "{e}");
    assert_eq!(std::fs::read(&path).unwrap(), b"already here");

    let mut d = Destination::new(&path, true).unwrap();
    d.writer().write_all(b"replaced").unwrap();
    d.commit().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"replaced");
}

#[test]
fn refuses_a_directory_that_is_not_there_and_names_it() {
    let s = sandbox();
    let e = Destination::new(s.path().join("nowhere").join("c.slpc"), false).unwrap_err();
    assert!(e.to_string().contains("nowhere"), "{e}");
}

#[test]
fn reads_back_what_was_written() {
    let s = sandbox();
    let bytes = container("report.pdf", b"%PDF");
    let mut d = Destination::new(s.path().join("c.slpc"), false).unwrap();
    d.writer().write_all(&bytes).unwrap();

    // The read-back a caller does before replacing anything with it.
    assert!(slpc::validate(d.written().unwrap())
        .unwrap()
        .is_conformant());
    d.commit().unwrap();
}

#[cfg(unix)]
mod permissions {
    use super::{container, entries, sandbox, Destination};
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    /// What a file created the ordinary way gets here, whatever the umask is.
    ///
    /// Measured rather than written down: hard-coding 0644 would pass under one
    /// umask and fail under another, and the claim is that a container comes
    /// out like any other file rather than like a particular number.
    fn ordinary(dir: &Path) -> u32 {
        let p = dir.join("ordinary.txt");
        std::fs::write(&p, b"reference").unwrap();
        let m = mode(&p);
        std::fs::remove_file(&p).unwrap();
        m
    }

    fn write_to(d: &mut Destination) {
        d.writer().write_all(&container("a.txt", b"x")).unwrap();
    }

    #[test]
    fn a_new_file_gets_what_the_umask_decided() {
        // The rename is what makes this worth a test: a temporary file is
        // private to its owner, and carrying that mode onto the destination is
        // the defect this exists to prevent.
        let s = sandbox();
        let want = ordinary(s.path());
        let path = s.path().join("c.slpc");

        let mut d = Destination::new(&path, false).unwrap();
        write_to(&mut d);
        d.commit().unwrap();

        assert_eq!(mode(&path), want, "a renamed file must not come out 0600");
        assert_eq!(entries(s.path()), ["c.slpc"], "the probe was left behind");
    }

    #[test]
    fn forcing_over_a_file_still_gets_what_the_umask_decided() {
        // `new` creates a file at a path the caller named; a file that happened
        // to be in the way does not get to decide who can read what replaces
        // it. Carrying a mode across is what `in_place` is for.
        let s = sandbox();
        let want = ordinary(s.path());
        let path = s.path().join("c.slpc");
        std::fs::write(&path, b"in the way").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let mut d = Destination::new(&path, true).unwrap();
        write_to(&mut d);
        d.commit().unwrap();

        assert_eq!(mode(&path), want);
    }

    #[test]
    fn writing_in_place_keeps_the_files_own_permissions() {
        let s = sandbox();
        let path = s.path().join("c.slpc");
        std::fs::write(&path, container("a.txt", b"x")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        let mut d = Destination::in_place(&path).unwrap();
        write_to(&mut d);
        d.commit().unwrap();

        assert_eq!(mode(&path), 0o640);
        assert_eq!(entries(s.path()), ["c.slpc"], "in place needs no probe");
    }

    #[test]
    fn writing_in_place_replaces_the_file_and_not_the_link_to_it() {
        let s = sandbox();
        let real = s.path().join("c.slpc");
        let link = s.path().join("link.slpc");
        std::fs::write(&real, container("a.txt", b"x")).unwrap();
        std::os::unix::fs::symlink("c.slpc", &link).unwrap();

        let mut d = Destination::in_place(&link).unwrap();
        d.writer().write_all(&container("b.txt", b"y")).unwrap();
        d.commit().unwrap();

        assert!(link.is_symlink(), "the link was replaced by a file");
        let c = slpc::Container::open(&real).unwrap();
        assert_eq!(c.payload_name(), "b.txt");
    }

    #[test]
    fn in_place_refuses_a_file_that_is_not_there() {
        let s = sandbox();
        assert!(Destination::in_place(s.path().join("absent.slpc")).is_err());
    }
}

/// Where a container's payload name becomes a path, and what it is shown as.
mod payload_paths {
    use super::sandbox;
    use slpc::{display_path, payload_path};
    use std::path::Path;

    /// Every name Win32 resolves to a device wherever it appears. `LPT1` and
    /// `PRN` are here even though they failed cleanly rather than hanging,
    /// because a clean failure is still a conformant container this build
    /// refuses; `NUL` is here because it succeeded while discarding the bytes,
    /// which is the worst of the three answers.
    ///
    /// Gated with the test that reads it: everywhere else these are ordinary
    /// filenames and the list would be an unused one.
    #[cfg(windows)]
    const DEVICE_NAMES: [&str; 8] = [
        "CON", "CON.txt", "con", "COM1", "AUX", "LPT1", "PRN", "NUL",
    ];

    /// The defect this catches is a payload landing on a device instead of in
    /// the directory. `dir.join("CON")` is not a path in `dir` on Windows — it
    /// is the console — so extraction wrote to the terminal, left no file, and
    /// anything reading the result back waited forever on input that never
    /// came. `check_payload_name` accepts these names because SPEC 2.3 does,
    /// and the conformance corpus carries a case for one.
    ///
    /// The directory is listed before the payload is read, and that order is
    /// deliberate: against the defect the listing is empty and this fails
    /// there, where reading first would hang the whole suite instead.
    #[test]
    #[cfg(windows)]
    fn a_name_windows_reads_as_a_device_still_lands_in_the_directory() {
        for name in DEVICE_NAMES {
            let dir = sandbox();
            let bytes = format!("bytes for {name}").into_bytes();

            let path = payload_path(dir.path(), name).expect("a path for the name");
            std::fs::write(&path, &bytes).expect("writes the payload");

            let listed: Vec<_> = std::fs::read_dir(dir.path())
                .expect("the directory")
                .map(|e| e.expect("an entry").file_name())
                .collect();
            assert!(
                listed.iter().any(|entry| entry == name),
                "{name}: nothing by that name is in the directory, so the payload went \
                 to a device rather than to a file. Listed: {listed:?}"
            );

            assert_eq!(
                std::fs::read(&path).expect("reads the payload back"),
                bytes,
                "{name}: what came back is not what was written"
            );
            std::fs::remove_file(&path).expect("the payload is removable");
        }
    }

    /// An ordinary name lands where it always did, on every platform. The
    /// defect this catches is the repair above changing where a payload goes
    /// for the overwhelming majority of names, which have never had a problem.
    #[test]
    fn an_ordinary_name_lands_in_the_directory_it_was_given() {
        let dir = sandbox();
        let path = payload_path(dir.path(), "report.pdf").expect("a path");

        std::fs::write(&path, b"payload").expect("writes");
        assert_eq!(std::fs::read(&path).expect("reads back"), b"payload");
        assert_eq!(path.file_name().expect("a filename"), "report.pdf");
        assert_eq!(
            display_path(&path),
            dir.path().join("report.pdf").display().to_string(),
            "what a person is shown is not the path they would have written"
        );
    }

    /// The defect this catches is `canonicalize` failing at the first write
    /// rather than where the caller can say something useful about it.
    #[test]
    #[cfg(windows)]
    fn a_directory_that_is_not_there_is_an_error_here() {
        let dir = sandbox();
        assert!(payload_path(&dir.path().join("no-such-directory"), "report.pdf").is_err());
    }

    /// The defect this catches is a person being told their payload went
    /// somewhere they have never seen and could not type. Nothing else takes
    /// the prefix off, so a caller that reaches for `Path::display` directly
    /// prints `\\?\C:\…`.
    #[test]
    fn the_verbatim_prefix_is_not_shown_to_a_person() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Users\a\report.pdf")),
            r"C:\Users\a\report.pdf"
        );
    }

    /// The same trick applied to a network path, where dropping the prefix
    /// whole would leave `server\share` — a relative path, and not anywhere.
    #[test]
    fn a_verbatim_network_path_keeps_its_two_separators() {
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\report.pdf")),
            r"\\server\share\report.pdf"
        );
    }

    /// The defect this catches is the stripping going too far and rewriting
    /// paths that never carried the prefix — which is every path on every
    /// platform but one.
    #[test]
    fn a_path_that_never_had_the_prefix_is_untouched() {
        for ordinary in [r"C:\Users\a\report.pdf", "/home/a/report.pdf", "report.pdf"] {
            assert_eq!(display_path(Path::new(ordinary)), ordinary);
        }
    }
}

/// The defect this catches is a refusal telling somebody that
/// `\\?\C:\…\report.pdf` exists — a spelling they have never seen and did not
/// write. `payload_path` is what can put a verbatim path into this message, so
/// it is what makes this reachable.
#[test]
#[cfg(windows)]
fn a_refusal_names_the_file_the_way_a_person_wrote_it() {
    let dir = sandbox();
    let path = slpc::payload_path(dir.path(), "report.pdf").expect("a path");
    std::fs::write(&path, b"already here").expect("the file in the way");

    let refusal = Destination::new(&path, false).expect_err("must refuse");
    let said = refusal.to_string();
    assert!(
        !said.contains(r"\\?\"),
        "the refusal shows the verbatim form: {said}"
    );
    assert!(said.contains("report.pdf"), "the refusal does not name the file: {said}");
}
