// What the platform records about where a container came from, carried onto
// the payload taken out of it.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![cfg(feature = "provenance")]

use slpc::provenance::{arrived_from_elsewhere, carry, Mark};
use std::path::Path;

fn sandbox() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// Mark `path` as having come from the internet, the way this platform's
/// downloaders do. Returns false where the filesystem will not hold the mark,
/// which is a fact about the machine rather than a failure of the code.
fn mark_as_downloaded(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        xattr::set(path, "com.apple.quarantine", b"0083;68ae0000;Safari;").is_ok()
    }
    #[cfg(target_os = "linux")]
    {
        xattr::set(
            path,
            "user.xdg.origin.url",
            b"https://example.invalid/a.slpc",
        )
        .is_ok()
    }
    #[cfg(target_os = "windows")]
    {
        let mut stream = path.as_os_str().to_os_string();
        stream.push(":Zone.Identifier");
        std::fs::write(
            stream,
            b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.invalid/a.slpc\r\n",
        )
        .is_ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = path;
        false
    }
}

/// **The defect this catches is the one the module exists for**: a payload
/// taken out of a downloaded container reaching whatever opens it next as
/// something this machine made, with the warning its origin earned never
/// shown. Break `carry` to return `Mark::Silent` without writing anything and
/// this fails, because the copy then says nothing about where it came from.
///
/// Skipped where the filesystem will not hold a mark — `tmpfs` mounted
/// `nouser_xattr`, a FAT volume — because that is a fact about the machine and
/// not something this code can answer for. Announced rather than passed
/// quietly, so a run that proved nothing does not read like one that did.
#[test]
fn a_downloaded_container_puts_its_origin_on_the_payload() {
    let dir = sandbox();
    let container = dir.path().join("downloaded.slpc");
    let payload = dir.path().join("report.pdf");
    std::fs::write(&container, b"container").unwrap();
    std::fs::write(&payload, b"payload").unwrap();

    if !mark_as_downloaded(&container) {
        eprintln!("skipped: this filesystem will not hold a provenance mark");
        return;
    }
    assert!(
        arrived_from_elsewhere(&container),
        "the test could not mark the container it is about to carry from"
    );

    let mark = carry(&container, &payload).expect("carrying");
    assert!(
        matches!(mark, Mark::Carried | Mark::Noted),
        "a downloaded container carried nothing onto its payload: {mark:?}"
    );
    assert!(
        arrived_from_elsewhere(&payload),
        "the payload does not say it came from anywhere, so unpacking laundered it"
    );
}

/// The defect this catches is the repair above going too far and marking
/// everything. A mark that appears on every payload says nothing, and a warning
/// a person sees on every file is one they learn to dismiss.
#[test]
fn a_container_from_nowhere_marks_nothing() {
    let dir = sandbox();
    let container = dir.path().join("made-here.slpc");
    let payload = dir.path().join("report.pdf");
    std::fs::write(&container, b"container").unwrap();
    std::fs::write(&payload, b"payload").unwrap();

    assert_eq!(carry(&container, &payload).expect("carrying"), Mark::Silent);
    assert!(
        !arrived_from_elsewhere(&payload),
        "a payload from a container made here was reported as arriving from elsewhere"
    );
}

/// The defect this catches is `carry` reporting success without the copy being
/// gated — which is what makes its error mean *do not open this*. The write is
/// denied the way a sandbox denies it, by making the target unwritable, and on
/// a platform that gates on a mark the answer must be an error.
///
/// Linux is exempt and says so: nothing there consults a mark before opening a
/// file, so refusing a payload over a note nothing reads would be theatre, and
/// `Mark::Noted` is the separate answer that keeps the two from being confused.
#[test]
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn a_copy_that_could_not_be_gated_is_a_failure() {
    let dir = sandbox();
    let container = dir.path().join("downloaded.slpc");
    let payload = dir.path().join("report.pdf");
    std::fs::write(&container, b"container").unwrap();
    std::fs::write(&payload, b"payload").unwrap();

    if !mark_as_downloaded(&container) {
        eprintln!("skipped: this filesystem will not hold a provenance mark");
        return;
    }

    let mut mode = std::fs::metadata(&payload).unwrap().permissions();
    mode.set_readonly(true);
    std::fs::set_permissions(&payload, mode).unwrap();

    let outcome = carry(&container, &payload);

    // Put back before asserting: a read-only file survives the cleanup that a
    // failing test never reaches.
    let mut mode = std::fs::metadata(&payload).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    mode.set_readonly(false);
    std::fs::set_permissions(&payload, mode).unwrap();

    assert!(
        outcome.is_err(),
        "a payload that could not be gated was reported as carried, which is \
         the laundering this module exists to prevent"
    );
}
