// Getting a seekable reader out of a path or out of standard input.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::fs::File;
use std::io::Seek;
use std::path::Path;

use crate::fail::{Context, Result};

/// Whether this argument is `-`: standard input where a file is read, and
/// standard output where one is written.
///
/// Every tool in the fleet takes `-` for standard input, so a caller writes it
/// by reflex. One that reads it as a filename fails with `-: No such file or
/// directory`, which looks like a bug in the caller's own command rather than a
/// missing feature. The writing half is this tool's own extension of that, and
/// it is what makes a container a thing a pipeline can carry.
pub fn is_dash(path: &Path) -> bool {
    path.as_os_str() == "-"
}

/// What to call this argument in a message.
pub fn name_of(path: &Path) -> String {
    if is_dash(path) {
        "standard input".to_owned()
    } else {
        path.display().to_string()
    }
}

/// Open a container for reading, spooling standard input if that is the source.
///
/// A ZIP cannot be read from a pipe: the central directory is at the end of the
/// file, so there is no finding a member without first seeking to it. The cost
/// of `-` on the reading verbs is therefore a copy through a temporary file,
/// and it is the tool's to pay rather than the library's, which keeps its
/// `Read + Seek` bound and never spools for a caller who already has a file.
///
/// The temporary file is unlinked as soon as it is created, so it leaves
/// nothing behind however this process ends.
pub fn container(path: &Path) -> Result<File> {
    if !is_dash(path) {
        return File::open(path).context(format!("cannot read {}", path.display()));
    }

    let mut spool =
        tempfile::tempfile().context("cannot create a temporary file for standard input")?;
    std::io::copy(&mut std::io::stdin().lock(), &mut spool)
        .context("cannot read standard input")?;
    spool.rewind().context("cannot rewind the spooled input")?;
    Ok(spool)
}
