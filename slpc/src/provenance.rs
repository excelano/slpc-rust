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
//! That is deliberately a test of the copy rather than of this module's own
//! success. What is called laundering above is a payload reaching its handler
//! looking like something this machine made, and the warning that then never
//! appears; it is not the absence of one particular value. A copy the platform
//! marked is gated, so the harm does not arise, and the source's own value —
//! which agent, which download — is detail this module loses rather than a
//! control it gives up. Testing the file rather than the environment is also
//! why nothing here asks whether it is sandboxed.
//!
//! Requires the `provenance` feature, which is off by default. It adds one
//! crate to the tree on Unix and none on Windows, where a stream is reached
//! through `std::fs`.

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
        Err(_) if platform::carries_a_mark(to) => Ok(Mark::AlreadyMarked),
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

    pub fn carry(from: &Path, to: &Path) -> io::Result<Mark> {
        match xattr::get(from, QUARANTINE)? {
            Some(value) => {
                xattr::set(to, QUARANTINE, &value)?;
                Ok(Mark::Carried)
            }
            None => Ok(Mark::Silent),
        }
    }

    /// Whether anything at all will consult a mark before opening this file.
    pub fn carries_a_mark(path: &Path) -> bool {
        value_of(path).is_some()
    }

    pub fn arrived_from_elsewhere(path: &Path) -> bool {
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

    pub fn arrived_from_elsewhere(_path: &Path) -> bool {
        false
    }
}
