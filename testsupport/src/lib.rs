//! Helpers both test suites need, in one copy.
//!
//! `slpc`'s tests and `slipcase`'s tests both have to mark a file the way this
//! platform's downloaders mark one. They are separate crates, so the helper was
//! written twice — and the two copies disagreed about the Windows arm within an
//! hour of being written, one carrying a `HostUrl` line and the other not. That
//! is the failure `check-shared-docs.py` exists to prevent in prose, happening
//! in code, and this crate is the same answer for it.
//!
//! Never published: `publish = false`, the way `corpus` is.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::path::Path;

/// Mark `path` as having come from the internet, the way this platform's
/// downloaders do.
///
/// **Written directly rather than through `slpc::provenance`**, deliberately. A
/// test that marked a file with the code under test would be asking the library
/// whether it agrees with itself, and would pass just as happily if both halves
/// were wrong together.
///
/// Returns false where the filesystem will not hold the mark — `tmpfs` mounted
/// `nouser_xattr`, a FAT volume. That is a fact about the machine rather than
/// something the code can answer for, and a caller is expected to announce a
/// skip rather than pass quietly, so that a run which proved nothing does not
/// read like one that did.
#[must_use]
pub fn mark_as_downloaded(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        // `flags;timestamp;agent;event-uuid`. 0083 is what Safari writes for a
        // download it has not yet had assessed.
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
        // The shape a browser really writes, checked against the downloads
        // folder of a Windows machine: `ZoneId` first, then the URLs. 3 is the
        // internet zone and is the line the shell reads.
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
