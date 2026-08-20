// One failure type, because every failure this tool produces exits the same way.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::fmt;

/// Something went wrong with the input or with what was asked for.
///
/// Two exit codes reach here. Most failures are exit 1, bad input. The
/// exception is a container this build can say nothing about: SPEC 3 forbids
/// reporting one as conformant or as non-conformant when its metadata member
/// cannot be read, and SPEC 2.4 puts a container declaring another version
/// outside the question rather than failing it. Both of those are exit 3.
///
/// Widening the code space is against the fleet's own convention, which keeps
/// it to three and says the distinction a caller needs is carried in the text.
/// The exception is earned here because the distinction is normative rather
/// than convenient: with one code for both, a caller branching on the status
/// reads "no verdict" as "non-conformant", which is the reading the
/// specification forbids.
///
/// A malformed command line never reaches here at all: clap reports it and
/// exits 2 itself.
pub struct Failure {
    message: String,
    code: u8,
}

impl Failure {
    /// Bad input: exit 1.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 1,
        }
    }

    /// No verdict is available: exit 3.
    pub fn no_verdict(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 3,
        }
    }

    #[must_use]
    pub fn code(&self) -> u8 {
        self.code
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<slpc::Error> for Failure {
    fn from(e: slpc::Error) -> Self {
        // An Unsupported error means this build cannot speak to the container,
        // which is the exit 3 case wherever it surfaces, not just under
        // `validate`. Refusing to unpack a payload compressed beyond this build
        // is the same kind of answer.
        match e {
            slpc::Error::Unsupported(_) => Self::no_verdict(e.to_string()),
            _ => Self::new(e.to_string()),
        }
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
        self.map_err(|e| Failure::new(format!("{what}: {e}")))
    }
}

pub type Result<T> = std::result::Result<T, Failure>;
