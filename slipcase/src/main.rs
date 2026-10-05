//! The slipcase command-line tool: five verbs over the `slpc` library.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]

mod fail;
mod input;
mod output;

use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use fail::{Context, Failure, Result};
use output::Destination;
use slpc::toml_edit::DocumentMut;
use slpc::{Container, Verdict};

/// Stated in `--help` because an undocumented convention is one a caller has to
/// discover by experiment.
const EXIT_CODES: &str = "\
Exit codes:
  0  success, or the container is conformant
  1  bad input: a file that is missing, unreadable, or not a conformant container
  2  bad command line: an unknown flag, a missing argument, a verb that is not one of these
  3  no verdict: the container may well be conformant and this build cannot say

3 is separate from 1 because the specification forbids calling a container
non-conformant when its flyleaf member cannot be read, or when it declares a
version this build does not implement. Both are answers, not failures.

Wherever a file is read, `-` names standard input. Wherever one is written,
`-` names standard output.";

#[derive(Parser)]
#[command(name = "slipcase", version, about, after_help = EXIT_CODES)]
struct Cli {
    #[command(subcommand)]
    verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
    /// Write a container holding a content file and its flyleaf.
    Pack(Pack),
    /// Write a container's content file to disk, and other members when asked.
    Unpack(Unpack),
    /// Change a container's flyleaf or content file, keeping everything else.
    Repack(Repack),
    /// Print a container's flyleaf.
    Info(Info),
    /// Report whether a container is conformant.
    Validate(Validate),
}

#[derive(Args)]
struct Pack {
    /// The file to pack. `-` reads standard input, which needs --name.
    content: PathBuf,
    /// The name to record in content.file. Taken from the content file's own filename otherwise.
    #[arg(long, value_name = "NAME")]
    name: Option<String>,
    /// A TOML file whose keys go into the container's flyleaf.
    #[arg(long, value_name = "FILE")]
    flyleaf: Option<PathBuf>,
    /// Where to write. Defaults to the content file's name with .slpc appended.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Overwrite an existing file.
    #[arg(long)]
    force: bool,
}

/// At least one of `--flyleaf` and `--content`: a repack with nothing to change
/// would read as a command that did something.
#[derive(Args)]
#[command(group(clap::ArgGroup::new("change").args(["flyleaf", "content"]).required(true).multiple(true)))]
struct Repack {
    /// The container to change. `-` reads standard input, which needs -o.
    container: PathBuf,
    /// A TOML file to become the container's flyleaf. `-` reads standard input.
    #[arg(long, value_name = "FILE")]
    flyleaf: Option<PathBuf>,
    /// A file to become the container's content file. `-` reads standard input, which needs --name.
    #[arg(long, value_name = "FILE")]
    content: Option<PathBuf>,
    /// The name to record in content.file. Taken from the content file's own filename otherwise.
    #[arg(long, value_name = "NAME", requires = "content")]
    name: Option<String>,
    /// Where to write. Rewrites the container in place otherwise.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Overwrite an existing --output file.
    #[arg(long)]
    force: bool,
}

#[derive(Args)]
struct Unpack {
    /// The container to unpack.
    container: PathBuf,
    /// Where to write the content file. Defaults to the current directory.
    #[arg(long, value_name = "DIR")]
    dest: Option<PathBuf>,
    /// Also write slipcase.flyleaf.toml.
    #[arg(long)]
    flyleaf: bool,
    /// Also write the additional member NAME. Repeatable.
    #[arg(long, value_name = "NAME", conflicts_with = "all_members")]
    member: Vec<String>,
    /// Also write every additional member.
    #[arg(long)]
    all_members: bool,
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
            ExitCode::from(e.code())
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.verb {
        Verb::Pack(a) => pack(a),
        Verb::Unpack(a) => unpack(a),
        Verb::Repack(a) => repack(&a),
        Verb::Info(a) => info(&a.container),
        Verb::Validate(a) => validate(&a.container),
    }
}

fn pack(a: Pack) -> Result<()> {
    let from_stdin = input::is_dash(&a.content);
    if from_stdin && a.name.is_none() {
        return Err(Failure::new(
            "packing from standard input needs --name: there is no filename to record in content.file.",
        ));
    }

    let flyleaf = match &a.flyleaf {
        None => DocumentMut::new(),
        Some(p) => read_flyleaf(p)?,
    };

    let out_path = match a.output {
        Some(p) => p,
        None => default_output(&a)?,
    };
    let mut out = destination(&out_path, a.force)?;

    // The library sets content.file and slipcase_version itself, so a --flyleaf
    // file that sets either to something else is refused rather than quietly
    // overwritten. Nothing here has to check for that.
    match (from_stdin, a.name) {
        (true, Some(name)) => {
            slpc::pack_reader(&name, std::io::stdin().lock(), flyleaf, out.writer())?;
        }
        (false, Some(name)) => {
            let f = std::fs::File::open(&a.content)
                .context(format!("cannot read {}", a.content.display()))?;
            slpc::pack_reader(&name, f, flyleaf, out.writer())?;
        }
        (false, None) => slpc::pack_file(&a.content, flyleaf, out.writer())?,
        (true, None) => unreachable!("checked above"),
    }
    out.commit()
}

/// The content file's name with `.slpc` appended, per the naming convention.
///
/// A convention and nothing more: `content.file` is the only authority on the
/// content file's name, and nothing here reads a container's name to find out what
/// is inside it.
fn default_output(a: &Pack) -> Result<PathBuf> {
    let stem = match &a.name {
        Some(n) => n.clone(),
        None => a
            .content
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| {
                Failure::new(format!(
                    "{} has no filename to build an output name from. Pass -o.",
                    a.content.display()
                ))
            })?
            .to_owned(),
    };
    Ok(PathBuf::from(format!("{stem}.slpc")))
}

fn read_flyleaf(path: &Path) -> Result<DocumentMut> {
    let what = input::name_of(path);
    let text = if input::is_dash(path) {
        std::io::read_to_string(std::io::stdin().lock())
    } else {
        std::fs::read_to_string(path)
    }
    .context(format!("cannot read {what}"))?;

    text.parse()
        .map_err(|e| Failure::new(format!("{what} is not valid TOML: {e}")))
}

/// Where a verb writes: a named file, or standard output for `-`.
fn destination(path: &Path, force: bool) -> Result<Destination> {
    if input::is_dash(path) {
        Destination::stdout()
    } else {
        Destination::new(path, force)
    }
}

/// Change a container, keeping everything that is not being changed.
///
/// With no `-o`, the container is written back over itself: a repack that named
/// its target and then refused to touch it would send every caller through
/// `repack -o tmp && mv tmp target`, which is the same operation with the
/// atomicity taken out.
fn repack(a: &Repack) -> Result<()> {
    // Three arguments can name standard input and there is only one of it.
    let container_piped = input::is_dash(&a.container);
    let sources = [Some(&a.container), a.flyleaf.as_ref(), a.content.as_ref()];
    if sources
        .into_iter()
        .flatten()
        .filter(|p| input::is_dash(p))
        .count()
        > 1
    {
        return Err(Failure::new(
            "only one of the container, --flyleaf, and --content can read standard input.",
        ));
    }
    if container_piped && a.output.is_none() {
        return Err(Failure::new(
            "a container read from standard input has no file to write back over. Pass -o.",
        ));
    }
    if a.content.as_deref().is_some_and(input::is_dash) && a.name.is_none() {
        return Err(Failure::new(
            "a content read from standard input needs --name: there is no filename to record in content.file.",
        ));
    }

    // Everything that has to be read is opened before the destination is
    // reserved, so a bad argument fails before a temporary file exists beside
    // the container.
    let source = input::container(&a.container)?;
    let flyleaf = a.flyleaf.as_deref().map(read_flyleaf).transpose()?;

    let mut out = match &a.output {
        Some(p) => destination(p, a.force)?,
        None => Destination::in_place(&a.container)?,
    };

    let mut r = slpc::Repack::new(source);
    if let Some(d) = &flyleaf {
        r = r.flyleaf(d);
    }
    r = match (a.content.as_deref(), &a.name) {
        (None, _) => r,
        (Some(p), Some(name)) if input::is_dash(p) => r.content(name, std::io::stdin().lock()),
        (Some(p), Some(name)) => {
            let f = std::fs::File::open(p).context(format!("cannot read {}", p.display()))?;
            r.content(name, f)
        }
        (Some(p), None) => r.content_file(p)?,
    };
    r.write(out.writer())?;

    // Read back what was written before it replaces anything. The library
    // validates the flyleaf it is about to store, so this is checking the
    // archive around it, and it is the difference between replacing the only
    // copy of a container on faith and doing it on evidence.
    let verdict = slpc::validate(out.written()?)?;
    if !verdict.is_conformant() {
        return Err(Failure::new(format!(
            "the container this would have written is {verdict}. Nothing was changed."
        )));
    }
    out.commit()?;

    // A repack in place keeps its own provenance: `Destination::in_place`
    // carries it onto the replacement, which is where that rule belongs because
    // every caller replacing a file wants it. `-o` is the other shape and the
    // library cannot help — a caller naming an output file is creating one, and
    // nothing in `Destination::new` knows which container the bytes came out
    // of. This does, and a container repacked from a downloaded one is as much
    // a thing that arrived from elsewhere as the content file `unpack` writes.
    //
    // Not a failure when it cannot be done, which is where this differs from
    // `unpack` above. What that guards is a content file about to be handed to the
    // system; this is a container, and nothing opens a container but this tool,
    // which reports provenance rather than acting on it. So the copy is left
    // and the loss is said out loud.
    if let (Some(out_path), false, false) = (
        &a.output,
        input::is_dash(&a.container),
        a.output.as_deref().is_some_and(input::is_dash),
    ) {
        if let Err(e) = slpc::provenance::carry(&a.container, out_path) {
            eprintln!(
                "slipcase: {} was written, and where {} came from could not be carried onto it: {e}",
                out_path.display(),
                input::name_of(&a.container),
            );
        }
    }
    Ok(())
}

fn unpack(a: Unpack) -> Result<()> {
    let mut c = Container::read(input::container(&a.container)?)?;
    let dest = a.dest.unwrap_or_else(|| PathBuf::from("."));

    let mut u = slpc::Unpack::new(&dest).force(a.force);
    if a.flyleaf {
        u = u.flyleaf();
    }
    for name in &a.member {
        u = u.member(name);
    }
    if a.all_members {
        u = u.all_members();
    }
    // A container read from standard input carries no provenance to carry.
    if !input::is_dash(&a.container) {
        u = u.carry_from(&a.container);
    }
    u.write(&mut c).map_err(output::placement)?;
    Ok(())
}

/// Print the flyleaf member as stored, byte for byte.
///
/// Not a re-serialization of it: this way the output is what the container
/// actually holds, comments and key order included, and it goes into another
/// TOML tool unchanged.
fn info(path: &Path) -> Result<()> {
    use std::io::IsTerminal as _;

    let c = Container::read(input::container(path)?)?;
    let bytes = c.flyleaf_bytes();
    let mut out = std::io::stdout();

    // One verb, two jobs, split where `ls` and `git` split them. Redirected
    // into a file or a pipe this reproduces the member byte for byte, which is
    // what a caller redirecting it asked for and what escaping would ruin.
    // Onto a terminal it is a display, and SPEC 3 requires the bidirectional
    // formatting characters be escaped rather than applied there — a terminal
    // is the one place they are applied, an override running to the end of the
    // paragraph rather than the end of the value it sat in.
    //
    // `from_utf8` cannot fail here: SPEC 2.2 requires the member be UTF-8 and
    // `Container::read` parsed it as TOML before this was reached. The fallback
    // is the raw bytes rather than an error, because refusing to print a
    // document over its encoding would be this verb inventing a verdict.
    if out.is_terminal() {
        if let Ok(text) = std::str::from_utf8(bytes) {
            return out
                .write_all(slpc::display_name(text).as_bytes())
                .context("cannot write to standard output");
        }
    }
    out.write_all(bytes)
        .context("cannot write to standard output")
}

fn validate(path: &Path) -> Result<()> {
    // Read the source once. `-` spools standard input to a file, and standard
    // input cannot be read a second time, so the rewind below is what lets the
    // conformant case name the content file without asking for the bytes again.
    let mut source = input::container(path)?;
    let verdict = slpc::validate(&mut source)?;

    // Each of the four verdicts gets the exit code that says what it is.
    // Reporting undetermined or out-of-scope with the code a rejected container
    // gets is the conflation SPEC 3 forbids.
    match verdict {
        Verdict::Conformant => {
            source.rewind().context("cannot re-read the container")?;
            let c = Container::read(source)?;
            // Through `display_name`, because this line is read by somebody
            // deciding what a container holds and a content file called
            // `report<U+202E>fdp.exe` reads as `report.pdf` in every terminal
            // that applies the override (SPEC 3).
            println!(
                "conformant — slipcase {}, content file {}",
                c.version(),
                slpc::display_name(c.content_name())
            );
            Ok(())
        }
        v @ Verdict::NonConformant(_) => Err(Failure::new(v.to_string())),
        // Undetermined, out of scope, and anything a later version of the
        // library adds. Defaulting an unfamiliar verdict to "no verdict" is the
        // safe direction: it never claims a check this build did not run.
        v => Err(Failure::no_verdict(v.to_string())),
    }
}
