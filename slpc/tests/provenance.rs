// What the platform records about where a container came from, carried onto
// the payload taken out of it.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![cfg(feature = "provenance")]

use slpc::provenance::{arrived_from_elsewhere, carry, Mark};
use testsupport::mark_as_downloaded;

fn sandbox() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
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

/// **Rewriting a container in place does not launder it.**
///
/// The defect this catches was live from the day `Destination::in_place`
/// existed until 2026-08-27, and it is the module's own subject arriving by a
/// door nobody watched. `in_place` replaces a file by renaming a fresh one over
/// it, and a fresh file carries no mark — so editing the metadata of a
/// downloaded container and saving stripped whatever the platform had recorded
/// about where it came from. Measured before the fix: a container marked as
/// downloaded came back from `slipcase repack --meta` with no mark at all, and
/// every payload extracted from it afterwards was unmarked too, because
/// `carry` copies from the container.
///
/// Break the carry in `commit` and this fails at the second assertion.
///
/// Skipped where the filesystem will not hold a mark, and announced rather than
/// passed quietly.
#[test]
#[cfg(feature = "fs")]
fn rewriting_a_container_in_place_keeps_where_it_came_from() {
    use std::io::Write as _;

    let s = sandbox();
    let path = s.path().join("downloaded.slpc");
    std::fs::write(&path, b"the original bytes").unwrap();

    if !mark_as_downloaded(&path) {
        eprintln!("skipped: this filesystem will not hold a mark");
        return;
    }
    assert!(
        arrived_from_elsewhere(&path),
        "the fixture is marked before anything touches it"
    );

    let mut d = slpc::Destination::in_place(&path).unwrap();
    d.writer().write_all(b"the rewritten bytes").unwrap();
    d.commit().unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), b"the rewritten bytes");
    assert!(
        arrived_from_elsewhere(&path),
        "the rewrite laundered the container"
    );
}

/// A file written to a path nobody was replacing inherits nothing.
///
/// The other direction, and the one an over-eager fix breaks. `Destination::new`
/// is a caller naming an output file: there is no original whose origin the new
/// file inherits, and inventing one would be this library claiming a download
/// that never happened. It is the same line `new` already takes about
/// permissions.
#[test]
#[cfg(feature = "fs")]
fn writing_a_new_file_inherits_nothing() {
    use std::io::Write as _;

    let s = sandbox();
    let neighbour = s.path().join("downloaded.slpc");
    std::fs::write(&neighbour, b"marked").unwrap();
    if !mark_as_downloaded(&neighbour) {
        eprintln!("skipped: this filesystem will not hold a mark");
        return;
    }

    let fresh = s.path().join("fresh.slpc");
    let mut d = slpc::Destination::new(&fresh, false).unwrap();
    d.writer().write_all(b"mine").unwrap();
    d.commit().unwrap();
    assert!(!arrived_from_elsewhere(&fresh));

    // And the case that actually bites. Writing to a path that does not exist
    // proves nothing: there is no original to inherit from whatever the code
    // does. `new` over a file that *is* marked is where an over-eager carry
    // shows — it would take the mark of the file it is replacing, which is
    // `in_place`'s job and not this one. A caller naming an output file is
    // creating a file there, and one that happened to be in the way does not
    // decide where the new one came from.
    let over = s.path().join("over.slpc");
    std::fs::write(&over, b"in the way").unwrap();
    if !mark_as_downloaded(&over) {
        eprintln!("skipped: this filesystem will not hold a mark");
        return;
    }
    assert!(arrived_from_elsewhere(&over));

    let mut d = slpc::Destination::new(&over, true).unwrap();
    d.writer().write_all(b"mine").unwrap();
    d.commit().unwrap();

    assert!(
        !arrived_from_elsewhere(&over),
        "a new file inherited the mark of the one it replaced"
    );
}
