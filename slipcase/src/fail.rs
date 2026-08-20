// One failure type, because every failure this tool produces exits the same way.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::fmt;

/// Something went wrong with the input or with what was asked for.
///
/// There is one variant because there is one exit code behind it. A malformed
/// command line never reaches here: clap reports it and exits 2 itself, which
/// is the split the fleet convention asks for — 2 says re-read `--help`, 1 says
/// go and look at the file.
pub struct Failure(String);

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<slpc::Error> for Failure {
    fn from(e: slpc::Error) -> Self {
        Self(e.to_string())
    }
}

/// Attach the thing being acted on to an I/O error.
///
/// `No such file or directory` on its own tells a caller nothing about which
/// file, and the first line of the message is the one that survives truncation.
pub trait Context<T> {
    fn context(self, what: impl fmt::Display) -> Result<T>;
}

impl<T> Context<T> for std::io::Result<T> {
    fn context(self, what: impl fmt::Display) -> Result<T> {
        self.map_err(|e| Failure(format!("{what}: {e}")))
    }
}

pub type Result<T> = std::result::Result<T, Failure>;
