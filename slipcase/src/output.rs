// Writing files that appear only once they are complete.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::fs::{File, Permissions};
use std::io::{IsTerminal, Seek, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::fail::{Context, Failure, Result};

/// Somewhere a verb writes: a file that becomes visible under its real name
/// only when it is finished, or standard output.
///
/// Everything is written to a temporary file first and put where it belongs at
/// the end. A pack that runs out of disk halfway therefore leaves nothing behind
/// rather than a truncated container that looks like one, `--force` over an
/// existing file cannot destroy the old one and then fail to produce the new,
/// and a repack that replaces a container with itself cannot leave the only copy
/// half written. Standard output goes through a temporary file too, so a
/// pipeline never receives the first half of a container that then failed — and
/// because repacking writes to a stream it can seek in, which is what stops the
/// members it copies through from claiming a length nothing supplies.
pub struct Destination {
    to: To,
}

enum To {
    File {
        tmp: NamedTempFile,
        path: PathBuf,
        force: bool,
        /// Taken from the file being replaced, where there is one. A
        /// temporary file is created private to its owner, and renaming one
        /// over a container would otherwise narrow it to 0600.
        mode: Option<Permissions>,
    },
    /// Spooled, then copied out at the end. Unlinked as soon as it is created,
    /// so it leaves nothing behind however this process ends.
    Stdout(File),
}

impl Destination {
    /// Reserve a destination, refusing to overwrite unless told to.
    ///
    /// The existence check here is for the message; the guarantee is the
    /// no-clobber rename in [`Destination::commit`], which is atomic and cannot
    /// be raced. Checking twice costs a `stat` and buys a sentence that names
    /// the file and the flag.
    pub fn new(path: &Path, force: bool) -> Result<Self> {
        Self::at(path, force, None)
    }

    /// Reserve a file to be written back over itself.
    ///
    /// The path is resolved first, so a container reached through a symbolic
    /// link is replaced rather than the link being replaced by a file, and the
    /// container's own permissions are what the replacement gets rather than
    /// the ones a new file would. What a rename cannot carry across is
    /// ownership, which is the standing cost of replacing a file rather than
    /// writing into it, and it is shared with every editor that writes this way.
    pub fn in_place(path: &Path) -> Result<Self> {
        let real =
            std::fs::canonicalize(path).context(format!("cannot read {}", path.display()))?;
        let mode = std::fs::metadata(&real)
            .context(format!("cannot read {}", real.display()))?
            .permissions();
        Self::at(&real, true, Some(mode))
    }

    fn at(path: &Path, force: bool, mode: Option<Permissions>) -> Result<Self> {
        if !force && path.exists() {
            return Err(Failure::new(format!(
                "{} exists. Pass --force to overwrite it.",
                path.display()
            )));
        }
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if !dir.is_dir() {
            return Err(Failure::new(format!(
                "{} is not a directory that exists, so nothing can be written into it.",
                dir.display()
            )));
        }
        let tmp =
            NamedTempFile::new_in(dir).context(format!("cannot write into {}", dir.display()))?;
        Ok(Self {
            to: To::File {
                tmp,
                path: path.to_owned(),
                force,
                mode,
            },
        })
    }

    /// Standard output.
    ///
    /// Refused when that is a terminal. A container is a ZIP archive, and a
    /// screenful of it helps nobody and can leave the terminal in a state the
    /// caller then has to fix.
    pub fn stdout() -> Result<Self> {
        if std::io::stdout().is_terminal() {
            return Err(Failure::new(
                "standard output is a terminal, and a container is not text. Redirect it, or name a file with -o.",
            ));
        }
        let spool =
            tempfile::tempfile().context("cannot create a temporary file for standard output")?;
        Ok(Self {
            to: To::Stdout(spool),
        })
    }

    /// Where to write.
    ///
    /// A file rather than a `Write`, because repacking needs to seek in what it
    /// is writing.
    pub fn writer(&mut self) -> &mut File {
        match &mut self.to {
            To::File { tmp, .. } => tmp.as_file_mut(),
            To::Stdout(spool) => spool,
        }
    }

    /// What has been written so far, rewound, for a caller that wants to read
    /// back its own output before anything is replaced by it.
    pub fn written(&mut self) -> Result<&mut File> {
        let f = self.writer();
        f.flush().context("cannot finish writing")?;
        f.rewind().context("cannot re-read what was written")?;
        Ok(f)
    }

    /// Put the finished file where it belongs.
    pub fn commit(self) -> Result<()> {
        let (tmp, path, force, mode) = match self.to {
            To::Stdout(mut spool) => {
                spool.rewind().context("cannot rewind the spooled output")?;
                let mut out = std::io::stdout().lock();
                std::io::copy(&mut spool, &mut out)
                    .and_then(|_| out.flush())
                    .context("cannot write to standard output")?;
                return Ok(());
            }
            To::File {
                tmp,
                path,
                force,
                mode,
            } => (tmp, path, force, mode),
        };

        if let Some(mode) = mode {
            tmp.as_file()
                .set_permissions(mode)
                .context(format!("cannot set the permissions of {}", path.display()))?;
        }

        let outcome = if force {
            tmp.persist(&path).map_err(|e| e.error)
        } else {
            tmp.persist_noclobber(&path).map_err(|e| e.error)
        };
        match outcome {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(Failure::new(format!(
                "{} exists. Pass --force to overwrite it.",
                path.display()
            ))),
            Err(e) => Err(Failure::new(format!(
                "cannot write {}: {e}",
                path.display()
            ))),
        }
    }
}
