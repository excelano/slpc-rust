// Writing files that appear only once they are complete.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::fs::File;
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

use crate::fail::{Context, Failure, Result};

/// A file that becomes visible under its real name only when it is finished.
///
/// Everything is written to a temporary file beside the destination and renamed
/// into place at the end. A pack that runs out of disk halfway therefore leaves
/// nothing behind rather than a truncated container that looks like one, and
/// `--force` over an existing file cannot destroy the old one and then fail to
/// produce the new.
pub struct Destination {
    tmp: NamedTempFile,
    path: PathBuf,
    force: bool,
}

impl Destination {
    /// Reserve a destination, refusing to overwrite unless told to.
    ///
    /// The existence check here is for the message; the guarantee is the
    /// no-clobber rename in [`Destination::commit`], which is atomic and cannot
    /// be raced. Checking twice costs a `stat` and buys a sentence that names
    /// the file and the flag.
    pub fn new(path: &Path, force: bool) -> Result<Self> {
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
            tmp,
            path: path.to_owned(),
            force,
        })
    }

    /// The file to write into.
    pub fn file(&mut self) -> &mut File {
        self.tmp.as_file_mut()
    }

    /// Put the finished file where it belongs.
    pub fn commit(self) -> Result<()> {
        let outcome = if self.force {
            self.tmp.persist(&self.path).map_err(|e| e.error)
        } else {
            self.tmp.persist_noclobber(&self.path).map_err(|e| e.error)
        };
        match outcome {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(Failure::new(format!(
                "{} exists. Pass --force to overwrite it.",
                self.path.display()
            ))),
            Err(e) => Err(Failure::new(format!(
                "cannot write {}: {e}",
                self.path.display()
            ))),
        }
    }
}
