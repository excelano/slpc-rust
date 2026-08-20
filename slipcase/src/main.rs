//! The slipcase command-line tool: four verbs over the `slpc` library.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]

mod fail;
mod input;
mod output;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use fail::{Context, Failure, Result};
use output::Destination;
use slpc::toml_edit::DocumentMut;
use slpc::Container;

/// Stated in `--help` because an undocumented convention is one a caller has to
/// discover by experiment.
const EXIT_CODES: &str = "\
Exit codes:
  0  success, or the container is conformant
  1  bad input: a file that is missing, unreadable, non-conformant, or beyond this build
  2  bad command line: an unknown flag, a missing argument, a verb that is not one of these

Wherever a file is read, `-` names standard input.";

#[derive(Parser)]
#[command(name = "slipcase", version, about, after_help = EXIT_CODES)]
struct Cli {
    #[command(subcommand)]
    verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
    /// Write a container holding a payload and its metadata.
    Pack(Pack),
    /// Write a container's payload to disk.
    Unpack(Unpack),
    /// Print a container's metadata.
    Info(Info),
    /// Report whether a container is conformant.
    Validate(Validate),
}

#[derive(Args)]
struct Pack {
    /// The file to pack. `-` reads standard input, which needs --name.
    payload: PathBuf,
    /// The name to record in payload.file. Taken from the payload's own filename otherwise.
    #[arg(long, value_name = "NAME")]
    name: Option<String>,
    /// A TOML file whose keys go into the container's metadata.
    #[arg(long, value_name = "FILE")]
    meta: Option<PathBuf>,
    /// Where to write. Defaults to the payload's name with .slpc appended.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Overwrite an existing file.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct Unpack {
    /// The container to unpack.
    container: PathBuf,
    /// Where to write the payload. Defaults to the current directory.
    #[arg(long, value_name = "DIR")]
    dest: Option<PathBuf>,
    /// Also write slipcase.metadata.toml.
    #[arg(long)]
    metadata: bool,
    /// Overwrite an existing file.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct Info {
    /// The container to read.
    container: PathBuf,
}

#[derive(Args)]
struct Validate {
    /// The container to check.
    container: PathBuf,
}

fn main() -> ExitCode {
    // clap reports a malformed command line itself and exits 2. Everything
    // reaching the match below is about the input, which is exit 1.
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("slipcase: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.verb {
        Verb::Pack(a) => pack(a),
        Verb::Unpack(a) => unpack(a),
        Verb::Info(a) => info(&a.container),
        Verb::Validate(a) => validate(&a.container),
    }
}

fn pack(a: Pack) -> Result<()> {
    let from_stdin = input::is_stdin(&a.payload);
    if from_stdin && a.name.is_none() {
        return Err(Failure::new(
            "packing from standard input needs --name: there is no filename to record in payload.file.",
        ));
    }

    let metadata = match &a.meta {
        None => DocumentMut::new(),
        Some(p) => read_metadata(p)?,
    };

    let out_path = match a.output {
        Some(p) => p,
        None => default_output(&a)?,
    };
    let mut out = Destination::new(&out_path, a.force)?;

    // The library sets payload.file and slipcase_version itself, so a --meta
    // file that sets either to something else is refused rather than quietly
    // overwritten. Nothing here has to check for that.
    match (from_stdin, a.name) {
        (true, Some(name)) => {
            slpc::pack_reader(&name, std::io::stdin().lock(), metadata, out.file())?;
        }
        (false, Some(name)) => {
            let f = std::fs::File::open(&a.payload)
                .context(format!("cannot read {}", a.payload.display()))?;
            slpc::pack_reader(&name, f, metadata, out.file())?;
        }
        (false, None) => slpc::pack_file(&a.payload, metadata, out.file())?,
        (true, None) => unreachable!("checked above"),
    }
    out.commit()
}

/// The payload's name with `.slpc` appended, per the naming convention.
///
/// A convention and nothing more: `payload.file` is the only authority on the
/// payload's name, and nothing here reads a container's name to find out what
/// is inside it.
fn default_output(a: &Pack) -> Result<PathBuf> {
    let stem = match &a.name {
        Some(n) => n.clone(),
        None => a
            .payload
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| {
                Failure::new(format!(
                    "{} has no filename to build an output name from. Pass -o.",
                    a.payload.display()
                ))
            })?
            .to_owned(),
    };
    Ok(PathBuf::from(format!("{stem}.slpc")))
}

fn read_metadata(path: &Path) -> Result<DocumentMut> {
    let text = std::fs::read_to_string(path).context(format!("cannot read {}", path.display()))?;
    text.parse()
        .map_err(|e| Failure::new(format!("{} is not valid TOML: {e}", path.display())))
}

fn unpack(a: Unpack) -> Result<()> {
    let mut c = Container::read(input::container(&a.container)?)?;
    let dest = a.dest.unwrap_or_else(|| PathBuf::from("."));

    // payload.file is a plain filename, checked against SPEC 2.3 when the
    // container was read, so joining it to a destination cannot leave that
    // destination. That is what the rule is for.
    let mut payload_out = Destination::new(&dest.join(c.payload_name()), a.force)?;

    // Both destinations are reserved before either is written, so --metadata
    // over an existing file fails before the payload has already landed.
    let mut metadata_out = if a.metadata {
        let bytes = c.metadata_bytes().to_vec();
        let mut d = Destination::new(&dest.join(slpc::METADATA_MEMBER), a.force)?;
        d.file()
            .write_all(&bytes)
            .context("cannot write the metadata")?;
        Some(d)
    } else {
        None
    };

    std::io::copy(&mut c.payload()?, payload_out.file()).context("cannot write the payload")?;
    payload_out.commit()?;
    if let Some(d) = metadata_out.take() {
        d.commit()?;
    }
    Ok(())
}

/// Print the metadata member as stored, byte for byte.
///
/// Not a re-serialization of it: this way the output is what the container
/// actually holds, comments and key order included, and it goes into another
/// TOML tool unchanged.
fn info(path: &Path) -> Result<()> {
    let c = Container::read(input::container(path)?)?;
    std::io::stdout()
        .write_all(c.metadata_bytes())
        .context("cannot write to standard output")
}

fn validate(path: &Path) -> Result<()> {
    let c = Container::read(input::container(path)?)?;

    // A version this build does not implement is not a non-conformant
    // container, and saying "conformant" would claim a check that never ran:
    // everything past the two required keys is a rule this version's text
    // states. Exit 1 is the refusal, not a verdict on the file.
    if c.version() != slpc::VERSION {
        return Err(Failure::new(format!(
            "declares slipcase {}, which this build does not implement. The metadata is well formed; nothing beyond it was checked.",
            c.version()
        )));
    }

    println!(
        "conformant — slipcase {}, payload {}",
        c.version(),
        c.payload_name()
    );
    Ok(())
}
