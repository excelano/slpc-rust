// Where a verb writes: a file, through the library, or standard output.
//
// The file half is `slpc::Destination`, which is where the temporary file, the
// permissions, and the rename live. What stays here is what is shaped like a
// command-line tool rather than like a container: `-` for standard output,
// refusing to write a ZIP at a terminal, and the wording of the messages,
// which mention flags the library has never heard of.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::fs::File;
use std::io::{IsTerminal, Seek, Write};
use std::path::Path;

use crate::fail::{Context, Failure, Result};

/// Somewhere a verb writes: a file that becomes visible under its real name
/// only when it is finished, or standard output.
///
/// Standard output goes through a temporary file too, so a pipeline never
/// receives the first half of a container that then failed — and because
/// repacking writes to a stream it can seek in, which is what stops the members
/// it copies through from claiming a length nothing supplies.
pub struct Destination {
    to: To,
}

enum To {
    File(slpc::Destination),
    /// Spooled, then copied out at the end. Unlinked as soon as it is created,
    /// so it leaves nothing behind however this process ends.
    Stdout(File),
}

impl Destination {
    /// Reserve a destination, refusing to overwrite unless told to.
    ///
    /// The two checks before the library is asked are for the message. The
    /// guarantee is the library's no-clobber rename, which is atomic and cannot
    /// be raced; these buy a sentence that names the file and the flag.
    pub fn new(path: &Path, force: bool) -> Result<Self> {
        if !force && path.exists() {
            return Err(exists(path));
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
        Ok(Self {
            to: To::File(slpc::Destination::new(path, force)?),
        })
    }

    /// Reserve a file to be written back over itself.
    pub fn in_place(path: &Path) -> Result<Self> {
        Ok(Self {
            to: To::File(slpc::Destination::in_place(path)?),
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
            To::File(d) => d.writer(),
            To::Stdout(spool) => spool,
        }
    }

    /// What has been written so far, rewound, for a caller that wants to read
    /// back its own output before anything is replaced by it.
    pub fn written(&mut self) -> Result<&mut File> {
        match &mut self.to {
            To::File(d) => Ok(d.written()?),
            To::Stdout(spool) => {
                spool.flush().context("cannot finish writing")?;
                spool.rewind().context("cannot re-read what was written")?;
                Ok(spool)
            }
        }
    }

    /// Put the finished file where it belongs.
    pub fn commit(self) -> Result<()> {
        match self.to {
            To::File(d) => d.commit().map_err(placement),
            To::Stdout(mut spool) => {
                spool.rewind().context("cannot rewind the spooled output")?;
                let mut out = std::io::stdout().lock();
                std::io::copy(&mut spool, &mut out)
                    .and_then(|_| out.flush())
                    .context("cannot write to standard output")
            }
        }
    }
}

/// The refusal to overwrite, in this tool's vocabulary.
///
/// The library reports `AlreadyExists` and says nothing about flags, because it
/// has no flags. Naming `--force` is this tool's job.
fn exists(path: &Path) -> Failure {
    Failure::new(format!(
        "{} exists. Pass --force to overwrite it.",
        path.display()
    ))
}

/// A library failure from putting a file somewhere.
///
/// The one case worth recognizing is a destination that appeared between being
/// reserved and being committed, which is the same refusal as the check up
/// front and deserves the same sentence.
fn placement(e: slpc::Error) -> Failure {
    match &e {
        slpc::Error::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists => {
            Failure::new(format!("{io}. Pass --force to overwrite it."))
        }
        _ => e.into(),
    }
}
