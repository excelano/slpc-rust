// Putting a container on disk: a file that appears only once it is complete.
//
// The reading side has taken a path since 0.1.0, through `Container::open`.
// This is the other half of that symmetry, and it is a feature rather than
// part of the default surface because a caller who only reads containers
// should not acquire a temporary-file dependency to do it.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::fs::{File, Permissions};
use std::io::{Seek, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::error::Result;

/// A file that appears under its real name only once it has been written.
///
/// Requires the `fs` feature, which is off by default:
/// `slpc = { version = "0.3.2", features = ["fs"] }`.
///
/// Everything is written to a temporary file beside the destination and renamed
/// into place at the end, so a write that fails partway leaves nothing behind
/// rather than a truncated container that looks like one, and replacing a file
/// cannot destroy the old one and then fail to produce the new.
///
/// The handle it lends out is a [`File`], so it satisfies the `Write + Seek`
/// that [`Repack::write`](crate::Repack::write) asks for.
///
/// ```no_run
/// # fn main() -> slpc::Result<()> {
/// let mut out = slpc::Destination::new("report.pdf.slpc", false)?;
/// slpc::pack_file("report.pdf", slpc::toml_edit::DocumentMut::new(), out.writer())?;
/// out.commit()?;
/// # Ok(())
/// # }
/// ```
///
/// Nothing is written to the destination until [`commit`](Destination::commit)
/// is called, and dropping one without committing removes the temporary file.
#[derive(Debug)]
pub struct Destination {
    tmp: NamedTempFile,
    path: PathBuf,
    force: bool,
    /// What the finished file should be readable by. A temporary file is
    /// created private to its owner, which is right while it is a temporary
    /// file and wrong the moment it is renamed into place, so this is applied
    /// before the rename. It comes from the file being replaced where there is
    /// one, and from [`new_file_mode`] where there is not.
    mode: Permissions,
}

impl Destination {
    /// Reserve a path to write to.
    ///
    /// With `force` false, an existing file is refused. The check here is for
    /// the caller's message; the guarantee is the no-clobber rename in
    /// [`commit`](Destination::commit), which is atomic and cannot be raced.
    /// Both report [`ErrorKind::AlreadyExists`](std::io::ErrorKind::AlreadyExists),
    /// so a caller can match on it and say whatever its own vocabulary calls
    /// the override.
    ///
    /// The finished file gets the permissions any other new file would get
    /// under the process umask, whether or not `force` replaced something.
    /// Carrying a file's own permissions across is what
    /// [`in_place`](Destination::in_place) is for.
    pub fn new<P: AsRef<Path>>(path: P, force: bool) -> Result<Self> {
        let path = path.as_ref();
        if !force && path.exists() {
            return Err(already_exists(path).into());
        }
        // Always the umask's answer, even where `force` is replacing something.
        // Carrying a mode is what `in_place` is for, and it is deliberate that
        // the two differ: a caller naming an output file is creating a file
        // there, and one that happened to be in the way should not decide who
        // can read what replaces it.
        Self::at(path, force, None)
    }

    /// Reserve a file to be written back over itself.
    ///
    /// The path is resolved first, so a container reached through a symbolic
    /// link is replaced rather than the link being replaced by a file, and the
    /// container's own permissions are what the replacement gets rather than
    /// the ones a new file would.
    ///
    /// What a rename cannot carry across is ownership, which is the standing
    /// cost of replacing a file rather than writing into it, and it is shared
    /// with every editor that writes this way.
    pub fn in_place<P: AsRef<Path>>(path: P) -> Result<Self> {
        let real = std::fs::canonicalize(path)?;
        let mode = std::fs::metadata(&real)?.permissions();
        Self::at(&real, true, Some(mode))
    }

    fn at(path: &Path, force: bool, mode: Option<Permissions>) -> Result<Self> {
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        // The directory is named in the error: a bare `No such file or
        // directory` from a temporary file whose name the caller never chose
        // tells them nothing about what to fix.
        let tmp = NamedTempFile::new_in(dir).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("cannot write into {}: {e}", dir.display()),
            )
        })?;
        let mode = match mode {
            Some(m) => m,
            None => new_file_mode(tmp.path())?,
        };
        Ok(Self {
            tmp,
            path: path.to_owned(),
            force,
            mode,
        })
    }

    /// Where to write.
    ///
    /// A [`File`] rather than an opaque `impl Write`, because repacking needs
    /// to seek in what it is writing and a caller may want to read it back.
    pub fn writer(&mut self) -> &mut File {
        self.tmp.as_file_mut()
    }

    /// What has been written so far, flushed and rewound.
    ///
    /// For a caller that wants to read back its own output before anything is
    /// replaced by it. `slipcase repack` validates through this, which is the
    /// difference between replacing the only copy of a container on faith and
    /// doing it on evidence.
    pub fn written(&mut self) -> Result<&mut File> {
        let f = self.writer();
        f.flush()?;
        f.rewind()?;
        Ok(f)
    }

    /// Put the finished file where it belongs.
    ///
    /// Reports [`ErrorKind::AlreadyExists`](std::io::ErrorKind::AlreadyExists)
    /// where the destination appeared after it was reserved and `force` is off.
    pub fn commit(self) -> Result<()> {
        let Self {
            tmp,
            path,
            force,
            mode,
        } = self;

        tmp.as_file().set_permissions(mode)?;

        let outcome = if force {
            tmp.persist(&path).map_err(|e| e.error)
        } else {
            tmp.persist_noclobber(&path).map_err(|e| e.error)
        };
        match outcome {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(already_exists(&path).into())
            }
            Err(e) => Err(e.into()),
        }
    }
}

/// The refusal to overwrite, carrying the path that was refused.
fn already_exists(path: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        // Through `display_path`, because since `payload_path` exists a caller
        // can hand this function the `\\?\` verbatim form, and this string is
        // read by a person deciding what to do about the file. Identical for
        // every path that never carried the prefix, which is all of them
        // everywhere but Windows.
        format!("{} exists", display_path(path)),
    )
}

/// The permissions a file created the ordinary way beside `near` would have.
///
/// Measured rather than asked for. What a new file gets is 0666 with the
/// process umask taken out of it, and there is no way to read the umask without
/// setting it, which needs a C call and the `unsafe` this crate forbids. So a
/// file is created the ordinary way, asked what it got, and removed. It costs
/// three system calls once per destination, and it is the difference between
/// handing back a container the umask decided who can read and one only its
/// author can.
///
/// The probe sits beside the temporary file and borrows its name, which is
/// already unique to this process, so nothing else can be creating it.
fn new_file_mode(near: &Path) -> Result<Permissions> {
    let mut name = near.as_os_str().to_owned();
    name.push(".mode");
    let probe = PathBuf::from(name);

    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    let mode = f.metadata().map(|m| m.permissions());
    drop(f);

    std::fs::remove_file(&probe)?;
    Ok(mode?)
}

/// Where a payload named `name` belongs inside `dir`, spelled the way this
/// platform can address it.
///
/// [`check_payload_name`](crate::check_payload_name) answers whether a name is
/// legal under SPEC 2.3. This answers a question the specification does not
/// ask: whether that legal name, joined to a directory, names a file on the
/// machine doing the joining. On Windows it may not, and the reason is not
/// traversal.
///
/// **`dir.join(name)` is not enough, and the argument that it is has now been
/// written twice.** It goes: `payload.file` is a plain filename checked against
/// SPEC 2.3, which rejects every separator and every traversal, so joining it
/// to a directory cannot leave that directory. That is true, and it is not the
/// question. Win32 resolves a handful of names — `CON`, `CON.txt`, `con`,
/// `COM1`, `AUX`, `LPT1`, `PRN`, `NUL` — to devices wherever the name appears.
/// `CON` does not leave the directory. It is not in it.
///
/// Measured in `excelano/slipcase-desktop` on 2026-08-26, one name at a time,
/// because they do not agree with one another. Writing `CON` returned `Ok` at
/// every step and left no file, the bytes having gone to the console; `metadata`
/// then failed with code 87, and `std::fs::read` **never returned**, because it
/// opens the console for reading and waits for input a window will never
/// supply. `LPT1` and `PRN` failed cleanly with `NotFound`. `NUL` succeeded and
/// discarded the bytes. So there is no one failure to code against, and the
/// conformance corpus did not disagree — it hung.
///
/// Windows looks for those names while it *parses* a path, and a path in the
/// `\\?\` verbatim form is not parsed that way. `canonicalize` answers in that
/// form, so this asks it of the directory and joins the name onto the answer.
/// Every name above then wrote, read back byte for byte, and was removable,
/// exactly as an ordinary name does.
///
/// **Nothing here holds a list of reserved names.** The prefix is asked of the
/// directory rather than spelled onto the path, so which names are devices
/// stays Windows's to know, and it goes on knowing it as the list changes.
///
/// Everywhere else the directory is the directory and this joins and returns.
/// The `\\?\` form is deliberately not produced on Unix: `canonicalize` there
/// would also resolve symbolic links, which would quietly change where a
/// caller's payload lands to fix a problem that platform does not have.
///
/// What this does not fix is opening the result. A file named for a device
/// extracts and is a real file; handing it to the shell still fails, with *the
/// specified device name is invalid*. That is a truth about the container on
/// that platform rather than a defect left behind.
///
/// # Errors
///
/// On Windows, whatever `canonicalize` says about `dir` — so a directory that
/// is not there is an error here rather than at the first write. Nowhere else.
pub fn payload_path(dir: &Path, name: &str) -> Result<PathBuf> {
    #[cfg(windows)]
    let dir = std::fs::canonicalize(dir)?;

    Ok(dir.join(name))
}

/// A path as it should be shown to a person.
///
/// [`payload_path`] hands back the `\\?\` verbatim form on Windows, because
/// that is what addresses a file whose name Windows would otherwise read as a
/// device. The prefix is how a path is addressed and not part of its name, so
/// printing it would tell somebody their payload went to a place spelled in a
/// way they have never seen and could not type. This crate introduced the
/// prefix, so this crate owes a caller the way to take it off.
///
/// Only the display form changes: every filesystem call keeps the spelling that
/// works. A no-op on a path that never carried the prefix, so a caller does not
/// have to know which kind it is holding.
#[must_use]
pub fn display_path(path: &Path) -> String {
    let text = path.display().to_string();
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return text;
    };
    // `\\?\UNC\server\share` is the same trick applied to a network path, and
    // putting the two leading separators back is what makes it the name a
    // person knows again. Stripping the prefix whole would leave
    // `server\share`, which is a relative path and not anywhere.
    match rest.strip_prefix(r"UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => rest.to_owned(),
    }
}
