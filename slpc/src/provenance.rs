//! Where a container came from, carried onto the payload taken out of it.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)
//
//! A container downloaded from the internet is marked as such by the platform
//! that downloaded it: `com.apple.quarantine` on macOS, a `Zone.Identifier`
//! stream on Windows. Both are consulted before a file is opened, and both are
//! properties of the file rather than of its contents — so a payload written
//! out of that container carries neither unless something puts them there.
//!
//! **Without it, unpacking is laundering.** Somebody downloads a container,
//! takes the payload out, and it reaches its handler as something this machine
//! created rather than as something that arrived from elsewhere; the warning
//! the platform would have shown never appears. That is the shape of defect
//! that made disk images and archives the delivery vehicle of choice, and it is
//! why refusing a payload name with a separator is not the end of what
//! unpacking owes.
//!
//! **Except under the macOS App Sandbox, where the platform marks the copy
//! first.** Measured 2026-08-25: a payload written by a sandboxed process came
//! out carrying `com.apple.quarantine` naming that process, from a container
//! carrying none, and the write attempted here was then refused — replacing one
//! quarantine value with another is how forgery would work. So the premise
//! above is false in that one configuration, and [`carry`] is written to
//! survive it being false.
//!
//! **The policy lives here rather than in the caller.** [`carry`] fails only
//! when the platform keeps a mark that gates opening, the source carries one,
//! and the copy ends up carrying none. Everything else — no mark, no such mark
//! on this platform, a note nothing enforces, a mark the platform put there
//! itself — succeeds. So the rule for a caller about to hand a payload to the
//! system is the whole of the rule: an error means do not open it.
//!
//! That is a test of the copy rather than of the write's own success: a copy the
//! platform marked is gated whoever marked it, so the harm does not arise, and
//! the source's own value is detail lost rather than a control given up. Asking
//! the file rather than the environment is why no arm here asks whether it is
//! sandboxed.
//!
//! Requires the `provenance` feature, which is off by default. It adds nothing
//! at all on Windows, where a stream is reached through `std::fs`, and on Unix
//! one crate on top of `fs` or four without it — `fs` already carries most of
//! what `xattr` needs.

use std::path::Path;

use crate::error::Result;

/// What was carried from a container onto the payload taken out of it.
///
/// `#[non_exhaustive]` for the reason every other public enum here carries it:
/// what a platform records about a downloaded file is that platform's to
/// change, and a fourth kind of answer must not cost a major version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Mark {
    /// The source said where it came from, and the copy now says the same. The
    /// platform will consult it before opening the payload.
    Carried,
    /// The same, except that nothing on this platform consults it. Linux keeps
    /// provenance as a note rather than as a gate, so this is hygiene and not a
    /// control, and it is a separate answer so that nothing mistakes it for one.
    Noted,
    /// The copy already said it arrived from elsewhere, and this module is not
    /// what put it there. The platform marks what a sandboxed process writes
    /// and then refuses to have that mark replaced, so the source's own value
    /// is lost while the gate it exists for is in place. A separate answer from
    /// [`Carried`](Mark::Carried) because the copy does not say what the source
    /// said, only that it came from somewhere.
    AlreadyMarked,
    /// The platform's own mark stands on the copy, and the source's value was
    /// kept beside it under an attribute of this crate's own.
    ///
    /// [`AlreadyMarked`](Mark::AlreadyMarked) with the detail recovered. The
    /// gate is the platform's either way; what this adds is the answer to
    /// *where did it come from*, which the platform destroys and will not let
    /// anything put back. It is a note and never a control — nothing but this
    /// crate reads it, exactly as [`Noted`](Mark::Noted) is a note on Linux —
    /// and it is as forgeable and as removable as any attribute on a file
    /// somebody can write. That costs nothing, because a forged one can only
    /// make a caller report provenance it cannot prove, and over-reporting is
    /// the side this module errs on deliberately.
    Recorded,
    /// The source said nothing about where it came from, or this platform keeps
    /// nothing that would say.
    Silent,
}

/// Carry whatever the platform records about `from` onto `to`.
///
/// # Errors
///
/// Returns the write error when this platform gates opening on a mark, `from`
/// carries one, and `to` ends up carrying none. A caller that is about to open
/// `to` must not, because the copy would be trusted where the original was not.
pub fn carry(from: &Path, to: &Path) -> Result<Mark> {
    match platform::carry(from, to) {
        // A refused write is only a failure if the copy is unmarked after it.
        // Under the App Sandbox it is not: the platform marked the copy on
        // creation, which is both why the write was refused and why refusing it
        // costs nothing that matters. Asked of the file rather than of the
        // process, so this is one branch on all three platforms and not a
        // sandbox check.
        Err(_) if platform::carries_a_mark(to) => Ok(if platform::note_origin(from, to) {
            Mark::Recorded
        } else {
            Mark::AlreadyMarked
        }),
        other => Ok(other?),
    }
}

/// Whether the platform records this file as having arrived from somewhere
/// other than the process asking.
///
/// For a caller that wants to *say* where a container came from rather than act
/// on it. It reports and never gates: what the platform will do about a mark is
/// the platform's business, and a person deciding whether to open a payload is
/// better served by knowing where the container came from than by being stopped.
///
/// **Not the same question as whether the file is gated**, and the two are one
/// function on the platforms where nothing writes a mark on a caller's behalf.
/// Under the macOS App Sandbox the platform marks whatever the calling process
/// writes, so a container that process saved carries a mark saying only that.
/// [`carry`] wants the gating question; this one is about origin and disregards
/// a mark whose agent is the calling executable. Anything it cannot read as the
/// caller's own it reports, because over-reporting provenance costs a person one
/// line of caution and under-reporting it is the defect this module exists to
/// prevent.
#[must_use]
pub fn arrived_from_elsewhere(path: &Path) -> bool {
    platform::arrived_from_elsewhere(path)
}

// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod platform {
    use super::{Mark, Path};
    use std::io;

    /// The attribute Launch Services and Gatekeeper both read. Its value
    /// encodes the agent, a timestamp, and an event identifier, and none of
    /// that is this crate's business: it is copied as opaque bytes, because
    /// rewriting it would be claiming the download was the caller's.
    const QUARANTINE: &str = "com.apple.quarantine";

    /// Where the source's mark is kept when the platform will not let it be
    /// carried, which under the App Sandbox is always.
    ///
    /// Named for the format rather than for this crate or for any application:
    /// `slipcase` the tool and Slipcase the application both write and read it,
    /// and it matches `com.excelano.slipcase`, the type the desktop bundle
    /// exports. The name is public surface the moment it ships.
    ///
    /// The value is the source's quarantine attribute copied verbatim, so the
    /// agent, the timestamp and the event identifier all survive, which is
    /// strictly more than the platform leaves behind.
    const ORIGIN_NOTE: &str = "com.excelano.slipcase.origin";

    // Deny the quarantine write the way the App Sandbox denies it, which is
    // the one thing no unsandboxed test can cause.
    //
    // The sandbox refuses *this attribute* and permits every other write to the
    // same file, and nothing available to a test reproduces that asymmetry.
    // Measured 2026-08-28: `0o444` denies our own attribute exactly as it
    // denies quarantine, so the `unwritable` seam the tests already use reaches
    // `Mark::AlreadyMarked` and can never reach `Mark::Recorded`; and the
    // kernel validates the value not at all — an empty one, `garbage`, and
    // non-hex flags are all accepted — so a malformed mark is no seam either.
    //
    // A branch this module cannot test is a branch that rots, and this one
    // decides whether provenance survives a save. Hence a seam, confined to
    // `cfg(test)` so nothing of it ships.
    #[cfg(test)]
    thread_local! {
        pub(super) static DENY_QUARANTINE_WRITE: std::cell::Cell<bool> =
            const { std::cell::Cell::new(false) };
    }

    fn set_quarantine(to: &Path, value: &[u8]) -> io::Result<()> {
        #[cfg(test)]
        if DENY_QUARANTINE_WRITE.with(std::cell::Cell::get) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        xattr::set(to, QUARANTINE, value)
    }

    pub fn carry(from: &Path, to: &Path) -> io::Result<Mark> {
        match xattr::get(from, QUARANTINE)? {
            Some(value) => {
                set_quarantine(to, &value)?;
                Ok(Mark::Carried)
            }
            None => Ok(Mark::Silent),
        }
    }

    /// Keep the source's mark beside the copy's own, and say whether it stuck.
    ///
    /// Called only where [`super::carry`] has found that the platform marked
    /// the copy itself and refused to have that replaced. Measured 2026-08-28
    /// inside a bundle signed with the App Sandbox entitlement: the refusal is
    /// specific to `com.apple.quarantine`, an attribute of our own goes on
    /// without complaint, and it survives `-[NSFileManager replaceItemAtURL:]`
    /// — which is the operation that destroys the attribution in the first
    /// place, so the note outlives the save that costs the mark.
    ///
    /// Best effort in both directions, like the Linux notes: a filesystem that
    /// will not hold the attribute is not a reason to refuse a payload, because
    /// the copy is gated by the platform's own mark whatever happens here.
    pub fn note_origin(from: &Path, to: &Path) -> bool {
        let Ok(Some(value)) = xattr::get(from, QUARANTINE) else {
            return false;
        };
        xattr::set(to, ORIGIN_NOTE, &value).is_ok()
    }

    fn note_of(path: &Path) -> Option<Vec<u8>> {
        xattr::get(path, ORIGIN_NOTE).ok().flatten()
    }

    /// Whether anything at all will consult a mark before opening this file.
    pub fn carries_a_mark(path: &Path) -> bool {
        value_of(path).is_some()
    }

    /// **The note is consulted here and nowhere else.** This is the question
    /// the card asks — *did this come from somewhere* — and a note answers it.
    /// [`carries_a_mark`] is the other question, *will anything gate on this*,
    /// and a note answers nothing there because nothing but this crate reads
    /// it. Collapsing the two is the defect that made a container somebody made
    /// on this machine report itself as downloaded, and they are separate for
    /// that reason.
    pub fn arrived_from_elsewhere(path: &Path) -> bool {
        if note_of(path).is_some() {
            return true;
        }
        match value_of(path) {
            Some(value) => !this_process_wrote(&value),
            None => false,
        }
    }

    fn value_of(path: &Path) -> Option<Vec<u8>> {
        xattr::get(path, QUARANTINE).ok().flatten()
    }

    /// Whether the mark records the calling process writing the file rather
    /// than the file arriving from anywhere.
    ///
    /// The value is `flags;timestamp;agent;event-uuid`, and the agent is the
    /// only field read here — the rest stays the opaque thing the constant
    /// above says it is. Measured under a sandbox on 2026-08-25: the agent of a
    /// mark the platform wrote on a process's behalf is that executable's own
    /// filename, so that is what it is compared against rather than a string
    /// spelled out here, and a binary renamed keeps agreeing with itself.
    ///
    /// Every uncertainty answers false, which reports the file as having
    /// arrived from elsewhere. A value with no third field, an executable this
    /// process cannot name: neither is evidence that the mark is the caller's,
    /// and the safe direction is to keep saying so.
    fn this_process_wrote(value: &[u8]) -> bool {
        use std::os::unix::ffi::OsStrExt;

        let Some(agent) = value.split(|b| *b == b';').nth(2) else {
            return false;
        };
        let Ok(us) = std::env::current_exe() else {
            return false;
        };
        us.file_name().is_some_and(|name| name.as_bytes() == agent)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{Mark, Path};
    use std::ffi::OsString;
    use std::io;

    /// The alternate data stream every downloader writes and the shell reads.
    /// It needs no API of its own: a stream is addressed by appending `:name`
    /// to the path, so `std::fs` reaches it and nothing here is FFI.
    const ZONE: &str = ":Zone.Identifier";

    /// The section the shell reads and the key inside it, both folded to lower
    /// case because both are matched without regard to case.
    const SECTION: &[u8] = b"[zonetransfer]";
    const ZONE_ID: &[u8] = b"zoneid";

    /// The lowest zone the shell stops for. 3 is the internet and 4 is the
    /// untrusted zone; 0, 1 and 2 are this machine, the local network, and a
    /// site somebody trusted, and nothing stops for those.
    const GATED_FROM: u32 = 3;

    fn stream_of(path: &Path) -> OsString {
        let mut named = path.as_os_str().to_os_string();
        named.push(ZONE);
        named
    }

    /// Nothing, deliberately, and this arm can reach the branch that calls it.
    ///
    /// [`super::carry`]'s fallback fires here whenever the zone write fails
    /// over a copy that already carries a gating stream, so unlike Linux this
    /// is not dead code. A second stream beside `Zone.Identifier` would hold a
    /// note perfectly well — a stream is addressed by appending `:name` and
    /// `std::fs` reaches it. What is missing is a measurement: nothing has
    /// established what the shell does with an unknown stream, whether it
    /// survives the copies and the archivers that strip `Zone.Identifier`, or
    /// whether a packaged install sees it at all. Writing it here on the
    /// strength of the macOS result would be exactly the inference this crate
    /// keeps refusing to make.
    pub fn note_origin(_from: &Path, _to: &Path) -> bool {
        false
    }

    pub fn carry(from: &Path, to: &Path) -> io::Result<Mark> {
        let zone = match std::fs::read(stream_of(from)) {
            Ok(bytes) => bytes,
            // No stream is the ordinary case for a container somebody made
            // here, and is not a failure to carry anything.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Mark::Silent),
            Err(e) => return Err(e),
        };
        std::fs::write(stream_of(to), zone)?;
        Ok(Mark::Carried)
    }

    /// Whether the shell will stop before opening this file.
    ///
    /// **Not whether the stream exists**, which is a different question on this
    /// platform than on the other two. A quarantine attribute is written whole
    /// or not at all; `std::fs::write` creates this stream and then writes into
    /// it, so a write that fails partway — a full disk being the realistic one
    /// — leaves a stream that exists and carries no `ZoneId`. [`super::carry`]
    /// reads a true answer here as licence to hand the payload over, so a
    /// stream the shell does not act on must not be one.
    ///
    /// Measured 2026-08-26 by running a script under `-ExecutionPolicy
    /// RemoteSigned`, which resolves a zone through this stream. Refused: a
    /// `ZoneId` of 3, 4 or 99, in either case, with spaces around the `=`, with
    /// `\n` alone for a line ending, with no trailing line ending, and after
    /// other keys. Ran, and so gates nothing: 0, 1, 2, -3, an empty value, and
    /// a `ZoneId` under another section or under none — the header carries
    /// weight. Where two `ZoneId` lines disagreed the last one decided, so this
    /// keeps the last rather than the first.
    ///
    /// One measured case is deliberately not reproduced. A value that is not a
    /// number at all still gates — `junk3` was refused — and this reads it as
    /// no gate. Being wrong that way costs a refusal to unpack; being wrong the
    /// other way hands over a payload nothing will stop for, which is the
    /// laundering this module exists to prevent.
    pub fn carries_a_mark(path: &Path) -> bool {
        let stream = std::fs::read(stream_of(path)).unwrap_or_default();
        zone_id(&stream).is_some_and(|zone| zone >= GATED_FROM)
    }

    /// The zone the shell would read out of this stream, where it would read
    /// one at all.
    fn zone_id(stream: &[u8]) -> Option<u32> {
        let mut reading = false;
        let mut zone = None;
        for line in stream.split(|b| matches!(b, b'\r' | b'\n')) {
            let line = line.trim_ascii();
            if line.starts_with(b"[") {
                reading = line.eq_ignore_ascii_case(SECTION);
                continue;
            }
            if !reading {
                continue;
            }
            let mut halves = line.splitn(2, |b| *b == b'=');
            let (Some(key), Some(value)) = (halves.next(), halves.next()) else {
                continue;
            };
            if key.trim_ascii().eq_ignore_ascii_case(ZONE_ID) {
                // Assigned rather than returned, so that a later line in the
                // section replaces this one the way the shell lets it.
                zone = std::str::from_utf8(value.trim_ascii())
                    .ok()
                    .and_then(|value| value.parse().ok());
            }
        }
        zone
    }

    /// A different question from the one above, and the difference is the whole
    /// reason they are two functions. A stream nothing gates on was still
    /// written by something, and nothing on Windows writes one on a caller's
    /// behalf. So anything at all here is reported, including the residue
    /// [`carries_a_mark`] refuses to treat as a gate: over-reporting provenance
    /// costs a person one line of caution, and under-reporting it is the defect
    /// this module exists to prevent.
    pub fn arrived_from_elsewhere(path: &Path) -> bool {
        std::fs::metadata(stream_of(path)).is_ok()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{Mark, Path};
    use std::io;

    /// What browsers write on a downloaded file, by freedesktop convention.
    /// Nothing consults either one before opening it — Linux has no counterpart
    /// to quarantine or the zone stream — so carrying them preserves provenance
    /// and gates nothing, which is what `Mark::Noted` says.
    const ORIGIN: [&str; 2] = ["user.xdg.origin.url", "user.xdg.referrer.url"];

    // The `Result` is the shape the other platforms need, not this one: on
    // Linux nothing here can fail, because a note nothing reads is not worth
    // refusing a payload over. Narrowing the signature would make the arms
    // disagree and push the difference into every caller.
    #[allow(clippy::unnecessary_wraps)]
    pub fn carry(from: &Path, to: &Path) -> io::Result<Mark> {
        let mut carried = false;
        for name in ORIGIN {
            // Best effort in both directions. A filesystem that will not hold
            // a `user.` attribute is not an error here, because refusing to
            // open a payload over a note nothing reads would be theatre.
            if let Ok(Some(value)) = xattr::get(from, name) {
                if xattr::set(to, name, &value).is_ok() {
                    carried = true;
                }
            }
        }
        Ok(if carried { Mark::Noted } else { Mark::Silent })
    }

    /// Nothing, and unreachable besides. [`super::carry`] only reaches this
    /// where the platform's write failed, and `carry` above cannot fail on this
    /// platform — a note nothing reads is not worth refusing a payload over, so
    /// every arm of it returns `Ok`. It exists so that the wrapper has one
    /// shape on every platform rather than a `cfg` in the middle of the rule.
    ///
    /// There is nothing to recover here in any case: what would be noted is
    /// already what this arm carries, and nothing overwrites it.
    pub fn note_origin(_from: &Path, _to: &Path) -> bool {
        false
    }

    /// The same question here, and neither answer gates anything: these
    /// attributes are a note, so nothing on this platform consults one before
    /// opening a file and nothing writes one on a caller's behalf.
    pub fn carries_a_mark(path: &Path) -> bool {
        arrived_from_elsewhere(path)
    }

    pub fn arrived_from_elsewhere(path: &Path) -> bool {
        ORIGIN
            .iter()
            .any(|name| matches!(xattr::get(path, name), Ok(Some(_))))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use super::{Mark, Path};
    use std::io;

    #[allow(clippy::unnecessary_wraps)]
    pub fn carry(_from: &Path, _to: &Path) -> io::Result<Mark> {
        Ok(Mark::Silent)
    }

    pub fn carries_a_mark(_path: &Path) -> bool {
        false
    }

    pub fn note_origin(_from: &Path, _to: &Path) -> bool {
        false
    }

    pub fn arrived_from_elsewhere(_path: &Path) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
//
// These came with the module from `excelano/slipcase-desktop`, where they were
// written against the same code. They are here rather than in `tests/` because
// several of them ask `platform::` directly — whether a zone gates, whether a
// mark is the calling process's own — and those are the questions the arms get
// wrong. `tests/provenance.rs` covers what a caller can reach.

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{carry, Mark};

    /// A container carrying no provenance leaves the copy carrying none, rather
    /// than inventing one or reporting that something was carried.
    #[test]
    fn a_container_from_nowhere_marks_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("plain.slpc");
        let to = dir.path().join("payload.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");

        assert_eq!(carry(&from, &to).expect("carrying"), Mark::Silent);
        assert!(xattr::get(&to, "user.xdg.origin.url")
            .expect("reading")
            .is_none());
    }

    /// The defect this catches is the whole point of the module: a payload
    /// extracted from a downloaded container arriving with no record of where
    /// the container came from.
    #[test]
    fn a_downloaded_container_puts_its_origin_on_the_payload() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("payload.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        xattr::set(
            &from,
            "user.xdg.origin.url",
            b"https://example.invalid/a.slpc",
        )
        .expect("marking the source");

        assert_eq!(carry(&from, &to).expect("carrying"), Mark::Noted);
        assert_eq!(
            xattr::get(&to, "user.xdg.origin.url").expect("reading"),
            Some(b"https://example.invalid/a.slpc".to_vec()),
        );
    }

    /// Both attributes are carried, not just the first one found. Catches a
    /// loop that returns as soon as it has something.
    #[test]
    fn the_referrer_is_carried_as_well_as_the_origin() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("payload.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        xattr::set(
            &from,
            "user.xdg.origin.url",
            b"https://example.invalid/a.slpc",
        )
        .expect("marking the origin");
        xattr::set(
            &from,
            "user.xdg.referrer.url",
            b"https://example.invalid/page",
        )
        .expect("marking the referrer");

        assert_eq!(carry(&from, &to).expect("carrying"), Mark::Noted);
        assert_eq!(
            xattr::get(&to, "user.xdg.referrer.url").expect("reading"),
            Some(b"https://example.invalid/page".to_vec()),
        );
    }

    /// Carrying replaces what the destination already said rather than leaving
    /// a stale origin from whatever wrote that file before.
    #[test]
    fn an_origin_already_on_the_copy_is_replaced() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("payload.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        xattr::set(&from, "user.xdg.origin.url", b"https://example.invalid/new")
            .expect("marking the source");
        xattr::set(&to, "user.xdg.origin.url", b"https://example.invalid/stale")
            .expect("marking the destination");

        carry(&from, &to).expect("carrying");
        assert_eq!(
            xattr::get(&to, "user.xdg.origin.url").expect("reading"),
            Some(b"https://example.invalid/new".to_vec()),
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::platform::carries_a_mark;
    use super::{arrived_from_elsewhere, carry, Mark};

    const ORIGIN_NOTE: &str = "com.excelano.slipcase.origin";

    /// Deny the quarantine write for the duration of one call, which is what
    /// the App Sandbox does and what no permission bit can imitate — `0o444`
    /// denies our own attribute too, so it can only ever produce
    /// `AlreadyMarked`. Restored on the way out so one test cannot leak into
    /// the next.
    fn with_quarantine_denied<T>(f: impl FnOnce() -> T) -> T {
        super::platform::DENY_QUARANTINE_WRITE.with(|d| d.set(true));
        let out = f();
        super::platform::DENY_QUARANTINE_WRITE.with(|d| d.set(false));
        out
    }

    const QUARANTINE: &str = "com.apple.quarantine";
    const FROM_SAFARI: &[u8] = b"0083;6a8dbb61;Safari;B8AC643B-5609-41D4-A666-ACC147704C79";
    const FROM_US: &[u8] = b"0082;6a8dc724;some-other-application;";

    /// A file that will not accept an attribute, so that the write `carry`
    /// attempts fails the way the App Sandbox makes it fail. A test cannot
    /// enter a sandbox; what it can do is deny the same write for a reason of
    /// its own and hold `carry` to the same rule.
    fn unwritable(path: &std::path::Path) {
        let mut mode = std::fs::metadata(path).expect("the file").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut mode, 0o444);
        std::fs::set_permissions(path, mode).expect("making it unwritable");
    }

    /// The defect this catches is Extract and Open failing outright under the
    /// App Sandbox for every container that arrived from elsewhere — the
    /// containers the whole module exists for. Measured 2026-08-25: the
    /// platform marks what a sandboxed process writes and then refuses to have
    /// that mark replaced, so `carry` failed, and `copy_out` turns a failure
    /// here into a refusal to extract at all. A copy that is already marked is
    /// gated, so nothing was laundered and there is nothing to refuse.
    #[test]
    fn a_copy_the_platform_marked_first_is_not_a_failure() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        xattr::set(&from, QUARANTINE, FROM_SAFARI).expect("marking the source");
        xattr::set(&to, QUARANTINE, FROM_US).expect("marking the copy");
        unwritable(&to);

        assert_eq!(
            carry(&from, &to).expect("a marked copy is not a failure"),
            Mark::AlreadyMarked
        );
    }

    /// The defect this catches is the fallback above swallowing a real one. A
    /// copy that carries no mark at all after the write was refused is exactly
    /// the laundering this module exists to prevent, and it must still be an
    /// error — otherwise the payload is handed to its handler looking like
    /// something this machine made.
    #[test]
    fn a_copy_with_no_mark_at_all_is_still_a_failure() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        xattr::set(&from, QUARANTINE, FROM_SAFARI).expect("marking the source");
        unwritable(&to);

        assert!(
            carry(&from, &to).is_err(),
            "an unmarked copy was accepted, which is the laundering this module exists to prevent"
        );
    }

    /// The mark the platform writes on the calling process's behalf, whose agent
    /// is the running executable's own filename. Built rather than spelled out,
    /// because under `cargo test` the executable is the test binary.
    fn our_own_mark() -> Vec<u8> {
        use std::os::unix::ffi::OsStrExt;
        let us = std::env::current_exe().expect("this process has a path");
        let mut value = b"0082;6a8dc724;".to_vec();
        value.extend_from_slice(us.file_name().expect("and a filename").as_bytes());
        value.push(b';');
        value
    }

    /// The defect this catches is a caller telling somebody that a container
    /// they made here arrived from elsewhere. Under the App Sandbox the
    /// platform marks whatever this process writes, so saving an edit marks the
    /// container — measured 2026-08-25 — and a predicate that only asks whether
    /// a mark exists then reports a local file as downloaded.
    #[test]
    fn a_mark_this_process_wrote_is_not_provenance() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let saved = dir.path().join("saved-here.slpc");
        std::fs::write(&saved, b"container").expect("the container");
        xattr::set(&saved, QUARANTINE, &our_own_mark()).expect("marking it as we would");

        assert!(
            !super::arrived_from_elsewhere(&saved),
            "a container the calling process saved is being reported as downloaded"
        );
    }

    /// The defect this catches is the test above going too far and silencing
    /// real provenance. A mark naming any other agent is what
    /// `arrived_from_elsewhere` exists to report, and disregarding one would be the module lying in the
    /// direction that costs something.
    #[test]
    fn a_mark_anything_else_wrote_still_is() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let downloaded = dir.path().join("downloaded.slpc");
        std::fs::write(&downloaded, b"container").expect("the container");
        xattr::set(&downloaded, QUARANTINE, FROM_SAFARI).expect("marking the source");

        assert!(super::arrived_from_elsewhere(&downloaded));
    }

    /// A value this module cannot read as its own is reported rather than
    /// disregarded. Catches a parser that treats a missing agent field, or any
    /// other shape it did not expect, as evidence the mark is ours — the safe
    /// direction is one line of unnecessary caution, and the other direction is
    /// the laundering this module exists to prevent.
    #[test]
    fn a_mark_that_cannot_be_read_is_reported() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let odd = dir.path().join("odd.slpc");
        std::fs::write(&odd, b"container").expect("the container");
        xattr::set(&odd, QUARANTINE, b"0082").expect("marking it oddly");

        assert!(super::arrived_from_elsewhere(&odd));
    }

    /// The defect this catches is the two questions being one function again.
    /// `carry` needs to know whether the copy is gated, and a copy the platform
    /// marked on the calling process's behalf is gated even though it did not
    /// arrive from anywhere. Making `carry` ask about origin instead breaks
    /// extraction under a sandbox, which is what the fallback was added to fix.
    #[test]
    fn a_copy_this_process_marked_still_counts_as_gated() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        xattr::set(&from, QUARANTINE, FROM_SAFARI).expect("marking the source");
        xattr::set(&to, QUARANTINE, &our_own_mark()).expect("as the platform would");
        unwritable(&to);

        assert_eq!(
            carry(&from, &to).expect("a marked copy is not a failure"),
            Mark::AlreadyMarked
        );
        assert!(
            !super::arrived_from_elsewhere(&to),
            "and the same file does not claim to have come from anywhere"
        );
    }

    /// A container that arrived from nowhere leaves the copy alone, rather than
    /// inventing a mark or reporting one. The macOS counterpart of the Linux
    /// test of the same name, and it catches an arm that treats "no mark on the
    /// source" as something to carry.
    #[test]
    fn a_container_from_nowhere_marks_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("plain.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");

        assert_eq!(carry(&from, &to).expect("carrying"), Mark::Silent);
        assert!(xattr::get(&to, QUARANTINE).expect("reading").is_none());
    }

    /// The defect this catches is the whole point of the module on this
    /// platform: a payload extracted from a downloaded container arriving with
    /// no quarantine attribute, so that Gatekeeper is never consulted about it.
    #[test]
    fn a_downloaded_container_puts_its_quarantine_on_the_payload() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        xattr::set(&from, QUARANTINE, FROM_SAFARI).expect("marking the source");

        assert_eq!(carry(&from, &to).expect("carrying"), Mark::Carried);
        assert_eq!(
            xattr::get(&to, QUARANTINE).expect("reading"),
            Some(FROM_SAFARI.to_vec()),
            "the copy does not carry the value the container carried"
        );
    }

    /// The defect this catches is a save under the App Sandbox laundering the
    /// card. The platform marks the rewrite, refuses to have that replaced, and
    /// the container then reports itself as something this machine made — so a
    /// person watches *arrived from elsewhere* disappear because they edited a
    /// title. Measured by hand on 2026-08-28 before this existed.
    #[test]
    fn a_mark_that_cannot_be_carried_is_recorded_beside_the_one_that_stands() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("rewritten.slpc");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"rewrite").expect("the rewrite");
        xattr::set(&from, QUARANTINE, FROM_SAFARI).expect("marking the source");
        xattr::set(&to, QUARANTINE, FROM_US).expect("the platform marking the copy");

        assert_eq!(
            with_quarantine_denied(|| carry(&from, &to)).expect("not a failure"),
            Mark::Recorded
        );
        assert_eq!(
            xattr::get(&to, ORIGIN_NOTE).expect("reading the note"),
            Some(FROM_SAFARI.to_vec()),
            "the source's own value, verbatim, agent and event identifier included"
        );
        assert!(
            arrived_from_elsewhere(&to),
            "the whole point: the copy still says where it came from"
        );
    }

    /// The defect this catches is the note being treated as a gate. Nothing but
    /// this crate reads it, so a copy carrying only a note must not be reported
    /// as something the platform will stop — that is the confusion that made a
    /// container somebody built here report itself as downloaded, and it cost
    /// an amendment to separate the two questions.
    #[test]
    fn a_note_answers_where_it_came_from_and_never_whether_it_is_gated() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let noted = dir.path().join("noted.slpc");
        std::fs::write(&noted, b"container").expect("the container");
        xattr::set(&noted, ORIGIN_NOTE, FROM_SAFARI).expect("noting the origin");

        assert!(arrived_from_elsewhere(&noted), "the card's question");
        assert!(!carries_a_mark(&noted), "the gate's question");
    }

    /// The defect this catches is `carry` reporting `Recorded` when it wrote
    /// nothing, which would tell a caller provenance survived where it did not.
    /// A source carrying no mark has nothing to record, so the fallback must
    /// stay `AlreadyMarked`.
    #[test]
    fn nothing_is_recorded_when_the_source_said_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("local.slpc");
        let to = dir.path().join("rewritten.slpc");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"rewrite").expect("the rewrite");
        xattr::set(&to, QUARANTINE, FROM_US).expect("the platform marking the copy");

        // `carry` returns Silent before the fallback is reached, because the
        // source has nothing to carry. The note must not appear regardless.
        assert_eq!(
            with_quarantine_denied(|| carry(&from, &to)).expect("not a failure"),
            Mark::Silent
        );
        assert_eq!(xattr::get(&to, ORIGIN_NOTE).expect("reading"), None);
    }

    /// The defect this catches is the note surviving as a stale claim. A
    /// container rewritten from a source that arrived from elsewhere records
    /// that source; one rewritten from a local source must not keep whatever
    /// the destination said before.
    #[test]
    fn a_note_already_on_the_copy_is_not_left_to_go_stale() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("rewritten.slpc");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"rewrite").expect("the rewrite");
        xattr::set(&from, QUARANTINE, FROM_SAFARI).expect("marking the source");
        xattr::set(&to, QUARANTINE, FROM_US).expect("the platform marking the copy");
        xattr::set(&to, ORIGIN_NOTE, b"0083;1;SomethingElse;stale").expect("a stale note");

        with_quarantine_denied(|| carry(&from, &to)).expect("not a failure");
        assert_eq!(
            xattr::get(&to, ORIGIN_NOTE).expect("reading the note"),
            Some(FROM_SAFARI.to_vec()),
            "replaced rather than left saying where some earlier file came from"
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::{carry, Mark};
    use std::path::Path;

    /// What a browser leaves on a container it downloaded. `ZoneId=3` is the
    /// internet zone, and it is the line the shell reads; the rest is detail
    /// this module copies without reading.
    const FROM_THE_INTERNET: &[u8] =
        b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.invalid/a.slpc\r\n";

    /// What a write that failed partway leaves behind. `std::fs::write` creates
    /// the stream and then writes into it, so a failure between the two — a
    /// full disk being the realistic one — leaves a stream that exists and says
    /// nothing the shell acts on.
    const A_WRITE_THAT_FAILED_PARTWAY: &[u8] = b"[ZoneTransfer]\r\n";

    /// The stream is addressed by appending its name to the path, which is why
    /// nothing in this module is FFI. Spelled out again here rather than
    /// reached for in `platform`, so that a test does not pass by agreeing with
    /// the code about where to look.
    fn stream_of(path: &Path) -> std::ffi::OsString {
        let mut named = path.as_os_str().to_os_string();
        named.push(":Zone.Identifier");
        named
    }

    fn mark(path: &Path, zone: &[u8]) {
        std::fs::write(stream_of(path), zone).expect("marking");
    }

    fn zone_on(path: &Path) -> Option<Vec<u8>> {
        std::fs::read(stream_of(path)).ok()
    }

    /// A file whose streams cannot be written. The macOS arm denies the write
    /// with a mode of `0o444` to stand in for a sandbox refusing it; the read
    /// only attribute is this platform's counterpart, and it denies the write
    /// without denying the read the predicate needs.
    fn unwritable(path: &Path) {
        let mut mode = std::fs::metadata(path).expect("the file").permissions();
        mode.set_readonly(true);
        std::fs::set_permissions(path, mode).expect("making it unwritable");
    }

    /// Put back, so that the temporary directory can be removed. Called before
    /// the assertion rather than after it, because a read only file survives
    /// the cleanup a failing test never reaches.
    //
    // Clippy objects because on Unix this sets a mode of `0o666`, which is not
    // what a caller usually means. This arm is Windows only, where the flag is
    // the read only file attribute and setting it false is the whole of what
    // putting the file back means.
    #[allow(clippy::permissions_set_readonly_false)]
    fn writable(path: &Path) {
        let mut mode = std::fs::metadata(path).expect("the file").permissions();
        mode.set_readonly(false);
        std::fs::set_permissions(path, mode).expect("putting it back");
    }

    /// The defect this catches is a payload extracted from a downloaded
    /// container reaching its handler with nothing on it the shell would stop
    /// for. `carries_a_mark` asked whether the stream existed, and the residue
    /// of a write that failed partway is a stream that exists carrying no
    /// `ZoneId` — so `carry` called the copy already marked, returned success,
    /// and the payload opened ungated.
    #[test]
    fn a_stream_that_does_not_gate_is_not_an_excuse_for_a_failed_write() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        mark(&from, FROM_THE_INTERNET);
        mark(&to, A_WRITE_THAT_FAILED_PARTWAY);
        unwritable(&to);

        let outcome = carry(&from, &to);
        writable(&to);
        assert!(
            outcome.is_err(),
            "a copy the shell will not gate was accepted as already marked, \
             which is the laundering this module exists to prevent"
        );
    }

    /// The defect this catches is the repair above going too far and turning
    /// the fallback off. A copy that already carries a zone the shell gates is
    /// gated whoever wrote it, so nothing was laundered and there is nothing to
    /// refuse — the same rule the macOS arm applies to a mark the sandbox
    /// wrote.
    #[test]
    fn a_copy_the_shell_would_gate_is_not_a_failure() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        mark(&from, FROM_THE_INTERNET);
        mark(&to, b"[ZoneTransfer]\r\nZoneId=4\r\n");
        unwritable(&to);

        let outcome = carry(&from, &to);
        writable(&to);
        assert_eq!(
            outcome.expect("a gated copy is not a failure"),
            Mark::AlreadyMarked
        );
    }

    /// The boundary is the measured one rather than the likely one. Measured
    /// 2026-08-26 by running a script under `-ExecutionPolicy RemoteSigned`,
    /// which resolves a zone through this stream: 0, 1 and 2 ran and 3 and 4
    /// were refused. A predicate that took any `ZoneId` at all for a gate would
    /// hand over a payload nothing would stop and call it stopped for.
    #[test]
    fn only_the_zones_the_shell_gates_count_as_a_mark() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for (zone, gates) in [(0, false), (1, false), (2, false), (3, true), (4, true)] {
            let file = dir.path().join(format!("zone-{zone}.pdf"));
            std::fs::write(&file, b"payload").expect("the payload");
            mark(
                &file,
                format!("[ZoneTransfer]\r\nZoneId={zone}\r\n").as_bytes(),
            );
            assert_eq!(
                super::platform::carries_a_mark(&file),
                gates,
                "zone {zone} was not read as the measurement says the shell reads it"
            );
        }
    }

    /// The defect this catches is the two questions becoming one function
    /// again, in the other direction. A caller asks where a container came
    /// from, and a stream this module cannot read as a zone is still evidence
    /// that something wrote one — over-reporting costs a person one line of
    /// caution, and under-reporting is what the module exists to prevent.
    #[test]
    fn a_stream_that_gates_nothing_is_still_reported_as_provenance() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let odd = dir.path().join("odd.slpc");
        std::fs::write(&odd, b"container").expect("the container");
        mark(&odd, A_WRITE_THAT_FAILED_PARTWAY);

        assert!(super::arrived_from_elsewhere(&odd));
    }

    /// A container that arrived from nowhere leaves the copy alone, rather than
    /// inventing a stream or reporting one. The Windows counterpart of the
    /// Linux and macOS tests of the same name.
    #[test]
    fn a_container_from_nowhere_marks_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("plain.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");

        assert_eq!(carry(&from, &to).expect("carrying"), Mark::Silent);
        assert!(zone_on(&to).is_none());
    }

    /// The defect this catches is the whole point of the module on this
    /// platform: a payload extracted from a downloaded container arriving with
    /// no zone stream, so that the shell never asks about it.
    #[test]
    fn a_downloaded_container_puts_its_zone_on_the_payload() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let from = dir.path().join("downloaded.slpc");
        let to = dir.path().join("report.pdf");
        std::fs::write(&from, b"container").expect("the container");
        std::fs::write(&to, b"payload").expect("the payload");
        mark(&from, FROM_THE_INTERNET);

        assert_eq!(carry(&from, &to).expect("carrying"), Mark::Carried);
        assert_eq!(
            zone_on(&to).as_deref(),
            Some(FROM_THE_INTERNET),
            "the copy does not carry the zone the container carried"
        );
    }
}
