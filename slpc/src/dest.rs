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
/// `slpc = { version = "0.3", features = ["fs"] }`.
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
    /// The file this one is replacing, when it is replacing one in place.
    ///
    /// Kept so that [`commit`](Destination::commit) can carry the platform's
    /// record of where that file came from onto the replacement. A rename puts
    /// a *new* file at the path and a new file carries no mark, so without this
    /// every in-place rewrite launders the original — which is what it did
    /// until 2026-08-27.
    carry_from: Option<PathBuf>,
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
    /// What a rename cannot carry across is ownership and any other name the
    /// file had. A hard link to the original keeps pointing at the original,
    /// which now holds the old contents and has a link count of one — so a
    /// container reachable under two names is rewritten under only the one it
    /// was opened by. Both are the standing cost of replacing a file rather
    /// than writing into it, and both are shared with every editor that writes
    /// this way; they are named here because the alternative is somebody
    /// discovering the second one from a container that did not change.
    pub fn in_place<P: AsRef<Path>>(path: P) -> Result<Self> {
        let real = std::fs::canonicalize(path)?;
        let mode = std::fs::metadata(&real)?.permissions();
        let mut this = Self::at(&real, true, Some(mode))?;
        this.carry_from = Some(real);
        Ok(this)
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
            carry_from: None,
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
            carry_from,
        } = self;

        tmp.as_file().set_permissions(mode)?;

        // Before the rename rather than after, for the reason the permissions
        // are: the file that appears at the path should be complete at the
        // instant it appears, and a window in which the replacement exists
        // unmarked is the window this exists to close.
        //
        // Only where something is being replaced. A caller naming an output
        // file is creating a file there, and there is no original whose
        // provenance the new one inherits — the same line `new` takes about
        // permissions, and for the same reason.
        //
        // An error here stops the commit, so the original stays. That is the
        // right way round: `carry` fails only when this platform gates opening
        // on a mark, the original carries one, and the replacement would not,
        // and replacing a gated container with an ungated one is precisely the
        // laundering it reports.
        if let Some(original) = &carry_from {
            crate::provenance::carry(original, tmp.path())?;
        }

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
/// **Use this rather than `dir.join(name)`.**
/// [`check_payload_name`](crate::check_payload_name) answers whether a name is
/// legal under SPEC 2.3, and a legal name is not always a file. Win32 resolves
/// `CON`, `CON.txt`, `con`, `COM1`, `AUX`, `LPT1`, `PRN` and `NUL` to devices
/// wherever the name appears, so `dir.join("CON")` is the console rather than a
/// path in `dir`. It is not a traversal, and the check against SPEC 2.3 does not
/// catch it — writing there can silently discard the payload, and reading it
/// back can block forever.
///
/// On Windows this answers in the `\\?\` verbatim form, which reaches the
/// filesystem without those names being looked for. Nothing here keeps a list of
/// reserved names: the form is asked of the *directory*, so which names are
/// devices stays Windows's to know as that list changes.
///
/// **Two things a caller has to know about the result.** It is where the file
/// *is* rather than how the caller spelled it — `canonicalize` expands 8.3 short
/// names and resolves junctions — so compare files rather than strings if you
/// hold a path of your own. And [`display_path`] is what takes the prefix off
/// before a person reads it, since the prefix is how a path is addressed and not
/// part of its name.
///
/// Everywhere but Windows the directory is the directory and this joins and
/// returns. The verbatim form is deliberately not produced on Unix, where
/// `canonicalize` would also resolve symbolic links and so move where a payload
/// lands to fix a problem that platform does not have.
///
/// Opening the result is a separate question and still fails: the shell answers
/// *the specified device name is invalid* for a file named after one. That is
/// the truth about such a container on that platform rather than something left
/// undone here.
///
/// # Errors
///
/// On Windows, whatever `canonicalize` says about `dir`, so a directory that is
/// not there is an error here rather than at the first write. Nowhere else.
pub fn payload_path(dir: &Path, name: &str) -> Result<PathBuf> {
    // Naming the directory, because the bare `canonicalize` error does not.
    // `slipcase unpack --dest nowhere` reported *The system cannot find the
    // file specified. (os error 2)* and left the person to work out which file
    // — where before this function existed the failure came later, from a
    // write into `nowhere`, and carried the name. A repair that costs a
    // sentence somebody reads is not a repair. Caught by an existing CLI test
    // on the Windows runner added one commit earlier, which is the whole
    // argument for that job in one line.
    #[cfg(windows)]
    let dir = std::fs::canonicalize(dir)
        .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", display_path(dir))))?;

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
