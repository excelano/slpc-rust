//! Run the conformance corpus from `excelano/slipcase` against this workspace.
//!
//! Two things are checked against every case: the verdict the library reaches,
//! and the exit code the tool returns for it. The second is not a restatement
//! of the first. `slipcase --help` states a four-code contract, three of whose
//! codes exist because SPEC 3 forbids reporting a container this build cannot
//! judge as one it has judged, and nothing else runs that mapping over a corpus
//! of containers built to break it.
//!
//! Not a test. It needs the specification repository checked out and its cases
//! generated, neither of which `cargo test` implies, and a test that has to
//! choose between skipping quietly and failing on a machine that was never
//! going to have those things is worse than a command run on purpose. It is a
//! step in RELEASING.md, because passing the corpus is a claim about a version
//! that shipped rather than about a commit.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::Parser;
use slpc::toml_edit::DocumentMut;

#[derive(Parser)]
#[command(name = "corpus", about, version)]
struct Args {
    /// The `conformance` directory of a checkout of excelano/slipcase.
    conformance: PathBuf,
    /// The slipcase binary whose exit codes to check. Found beside this one otherwise.
    #[arg(long, value_name = "FILE")]
    slipcase: Option<PathBuf>,
}

/// An `s` where one is owed. Counts land in these messages, and `1 cases` is
/// the tell of a program that was never run on the failing path.
fn s(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// One disagreement, for the report: what it was, and whatever either side had
/// to say about it. A verdict that agrees says nothing, so an empty line is not
/// printed for it.
fn detail(id: &str, why: &str, note: &str) -> String {
    let mut out = format!("  {id}");
    for line in [why, note] {
        if !line.is_empty() {
            out.push_str("\n      ");
            out.push_str(line);
        }
    }
    out
}

/// One case, as the manifest describes it.
struct Case {
    id: String,
    expect: String,
    note: String,
    file: PathBuf,
}

/// The exit code `slipcase validate` owes a container with this verdict.
///
/// Not derived from the library's answer: it is read off the contract in
/// `--help`, so that a change to either side shows up here as a disagreement
/// rather than as two things quietly agreeing to be wrong together.
fn owed_exit_code(expect: &str) -> Option<i32> {
    match expect {
        "accept" => Some(0),
        "reject" => Some(1),
        // Conformance cannot be established, or the container declares another
        // version. Both are answers this build cannot give, and neither may be
        // reported as non-conformant.
        "undetermined" | "out-of-scope" => Some(3),
        _ => None,
    }
}

/// This library's answer, in the corpus's vocabulary.
fn verdict(path: &Path) -> (&'static str, String) {
    let f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return ("unreadable", e.to_string()),
    };
    match slpc::validate(f) {
        Ok(slpc::Verdict::Conformant) => ("accept", String::new()),
        Ok(slpc::Verdict::NonConformant(m)) => ("reject", m.to_string()),
        Ok(slpc::Verdict::Undetermined(u)) => ("undetermined", u.to_string()),
        Ok(slpc::Verdict::OutOfScope(v)) => ("out-of-scope", v),
        // A verdict a later version of the library added and this did not learn
        // about. Named rather than folded into one of the four.
        Ok(v) => ("unknown-verdict", v.to_string()),
        Err(e) => ("unreadable", e.to_string()),
    }
}

fn main() -> ExitCode {
    match run(&Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("corpus: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(a: &Args) -> Result<(), String> {
    let cases = read_manifest(&a.conformance)?;
    let tool = find_slipcase(a.slipcase.clone())?;

    // A corpus that could not be found must never come out as agreement. Every
    // count below is only worth reading because these three refused first.
    if cases.is_empty() {
        return Err(format!(
            "{} describes no cases",
            a.conformance.join("manifest.toml").display()
        ));
    }
    missing_files(&cases)?;
    ungoverned_files(&a.conformance, &cases)?;

    let mut agreed = 0usize;
    let mut disagreements: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for c in &cases {
        let mut ok = true;

        let (got, why) = verdict(&c.file);
        if got != c.expect {
            ok = false;
            disagreements
                .entry(format!("the library: expected {}, got {got}", c.expect))
                .or_default()
                .push(detail(&c.id, &why, &c.note));
        }

        let owed = owed_exit_code(&c.expect)
            .ok_or_else(|| format!("{}: the manifest expects {:?}, which is not one of the four verdicts this build knows", c.id, c.expect))?;
        let (code, said) = exit_code(&tool, &c.file)?;
        if code != owed {
            ok = false;
            disagreements
                .entry(format!("the tool: expected exit {owed}, got {code}"))
                .or_default()
                .push(detail(&c.id, &said, &c.note));
        }

        if ok {
            agreed += 1;
        }
    }

    let total = cases.len();
    if disagreements.is_empty() {
        println!("{total} cases, all agree.");
        return Ok(());
    }

    println!(
        "{total} cases: {agreed} agree, {} did not.\n",
        total - agreed
    );
    for (what, which) in &disagreements {
        println!("=== {what}  ({} case{})", which.len(), s(which.len()));
        for line in which {
            println!("{line}");
        }
        println!();
    }
    Err(format!("{} of {total} cases did not agree", total - agreed))
}

/// The cases the manifest describes.
fn read_manifest(conformance: &Path) -> Result<Vec<Case>, String> {
    let path = conformance.join("manifest.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}. Point this at the `conformance` directory of a checkout of excelano/slipcase.", path.display()))?;
    let doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("{} is not valid TOML: {e}", path.display()))?;

    let listed = doc
        .get("case")
        .and_then(|c| c.as_array_of_tables())
        .ok_or_else(|| format!("{} has no [[case]] tables", path.display()))?;

    let mut cases = Vec::with_capacity(listed.len());
    for (n, t) in listed.iter().enumerate() {
        let string = |key: &str| t.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        let id = string("id").ok_or_else(|| format!("case {n} has no id"))?;
        let expect = string("expect").ok_or_else(|| format!("{id} has no expected verdict"))?;
        // A case whose subject is the container's name on disk says so.
        let file = conformance
            .join("cases")
            .join(string("filename").unwrap_or_else(|| format!("{id}.slpc")));

        cases.push(Case {
            id,
            expect,
            note: string("note").unwrap_or_default(),
            file,
        });
    }
    Ok(cases)
}

/// Refuse a corpus whose cases have not been generated.
fn missing_files(cases: &[Case]) -> Result<(), String> {
    let absent: Vec<&str> = cases
        .iter()
        .filter(|c| !c.file.exists())
        .map(|c| c.id.as_str())
        .collect();
    match absent.len() {
        0 => Ok(()),
        // All of them, which is what an ungenerated corpus looks like.
        n if n == cases.len() => Err(
            "no case files are there. Run `python3 generate.py` in that directory first; `cases/` is generated and is not committed."
                .to_owned(),
        ),
        n => Err(format!(
            "the manifest describes {n} case{} with no file: {}",
            s(n),
            absent.join(", ")
        )),
    }
}

/// Refuse a corpus holding containers the manifest says nothing about.
///
/// Reporting agreement on the cases that were described, while files sat beside
/// them unread, is the same false pass as reporting it on no cases at all.
fn ungoverned_files(conformance: &Path, cases: &[Case]) -> Result<(), String> {
    let described: std::collections::BTreeSet<&Path> =
        cases.iter().map(|c| c.file.as_path()).collect();

    let mut loose = Vec::new();
    let mut dirs = vec![conformance.join("cases")];
    while let Some(d) = dirs.pop() {
        let entries =
            std::fs::read_dir(&d).map_err(|e| format!("cannot read {}: {e}", d.display()))?;
        for e in entries {
            let e = e.map_err(|e| format!("cannot read {}: {e}", d.display()))?;
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "slpc") && !described.contains(p.as_path())
            {
                loose.push(p.display().to_string());
            }
        }
    }

    if loose.is_empty() {
        Ok(())
    } else {
        loose.sort();
        Err(format!(
            "cases/ holds {} container{} the manifest does not describe and nothing would have checked: {}",
            loose.len(),
            s(loose.len()),
            loose.join(", ")
        ))
    }
}

/// What `slipcase validate` returns for a container, and what it said.
fn exit_code(tool: &Path, case: &Path) -> Result<(i32, String), String> {
    let out = Command::new(tool)
        .arg("validate")
        .arg(case)
        .output()
        .map_err(|e| format!("cannot run {}: {e}", tool.display()))?;

    let said = if out.stderr.is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    } else {
        String::from_utf8_lossy(&out.stderr).trim().to_owned()
    };
    let code = out.status.code().ok_or_else(|| {
        format!(
            "{} was killed by a signal on {}",
            tool.display(),
            case.display()
        )
    })?;
    Ok((code, said))
}

/// The tool to run: the one named, or the one built beside this program.
fn find_slipcase(named: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(p) = named {
        return if p.exists() {
            Ok(p)
        } else {
            Err(format!("{} is not there", p.display()))
        };
    }

    let here = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
    let beside = here
        .parent()
        .ok_or("this program has no directory")?
        .join(format!("slipcase{}", std::env::consts::EXE_SUFFIX));

    if beside.exists() {
        Ok(beside)
    } else {
        Err(format!(
            "no slipcase binary beside this one at {}. Build the whole workspace — `cargo build --workspace` — or name one with --slipcase.",
            beside.display()
        ))
    }
}
