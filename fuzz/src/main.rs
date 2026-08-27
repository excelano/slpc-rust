//! Throw malformed bytes at the reader and watch what it does.
//!
//! Every defect the 2026-08-27 security review found came from somebody
//! *reasoning* about the code. This is the other half: bytes the code was never
//! shown, in volume, looking for the classes reading misses — a panic on
//! malformed input, a loop with no bound, arithmetic on a length an attacker
//! chose.
//!
//! Seeded from the conformance corpus, because a container is a structure and
//! random bytes are almost never one. Mutations that only flip bits spend their
//! whole run being refused as "not a ZIP archive"; the ones that matter here
//! rewrite the little-endian fields beside a ZIP signature, which is exactly
//! where today's three worst findings lived.
//!
//! Deterministic: the same `--seed` produces the same run. A case that fails is
//! reproducible from its number, and is written out besides.
//!
//! Usage:
//!
//!     cargo run --release -p fuzz -- /path/to/slipcase/conformance
//!     cargo run --release -p fuzz -- CONF --cases 200000 --seed 7
//!
//! Run it under `timeout`. The harness writes the input it is about to try to
//! `<out>/current.bin` before every case, so a run that hangs and is killed
//! leaves the container that hung it.
//!
//! Author: David M. Anderson
//! Built with AI assistance (Claude, Anthropic)

use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;

#[derive(Parser)]
#[command(about = "Mutate conformance containers and read the results")]
struct Args {
    /// The `conformance` directory of a checkout of `excelano/slipcase`.
    conformance: PathBuf,
    /// How many mutated containers to try.
    #[arg(long, default_value_t = 50_000)]
    cases: u64,
    /// The run is a function of this.
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Where to leave anything worth keeping.
    #[arg(long, default_value = "fuzz-findings")]
    out: PathBuf,
    /// A case taking longer than this is reported.
    #[arg(long, default_value_t = 2000)]
    slow_ms: u64,
}

/// xorshift64*, written out rather than depended on.
///
/// Reproducibility is the whole point and a dependency that changes its
/// algorithm between versions would take that away silently.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// The four ZIP signatures worth landing a mutation next to.
const SIGNATURES: [u32; 4] = [0x0403_4B50, 0x0201_4B50, 0x0605_4B50, 0x0606_4B50];

/// Values a length or an offset is interesting at.
const EDGES: [u64; 10] = [
    0,
    1,
    2,
    0xFFFF - 1,
    0xFFFF,
    0x1_0000,
    0xFFFF_FFFE,
    0xFFFF_FFFF,
    0x1_0000_0000,
    u64::MAX,
];

fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut out = seed.to_vec();
    if out.is_empty() {
        return out;
    }
    for _ in 0..1 + rng.below(4) {
        // An earlier round may have truncated it to nothing, and every arm
        // below indexes. Emptiness is a case worth handing to the reader, so it
        // is returned rather than avoided.
        if out.is_empty() {
            break;
        }
        match rng.below(6) {
            // A field beside a signature. The mutation that finds structural
            // defects, and the one a bit-flipper almost never stumbles into.
            0 | 1 => {
                let want = SIGNATURES[rng.below(SIGNATURES.len())].to_le_bytes();
                let sites: Vec<usize> = out
                    .windows(4)
                    .enumerate()
                    .filter(|(_, w)| *w == want)
                    .map(|(i, _)| i)
                    .collect();
                if sites.is_empty() {
                    continue;
                }
                let at = sites[rng.below(sites.len())];
                let field = at + 4 + rng.below(44);
                let value = EDGES[rng.below(EDGES.len())];
                let width = [2usize, 4, 8][rng.below(3)];
                if field + width <= out.len() {
                    out[field..field + width].copy_from_slice(&value.to_le_bytes()[..width]);
                }
            }
            // Ordinary bit and byte damage.
            2 => {
                let at = rng.below(out.len());
                out[at] ^= 1 << rng.below(8);
            }
            3 => {
                let at = rng.below(out.len());
                out[at] = rng.next() as u8;
            }
            // Truncation, which is how a reader meets a partial download.
            4 => {
                let keep = rng.below(out.len());
                out.truncate(keep);
            }
            // Splice a run of the file over itself.
            _ => {
                let len = 1 + rng.below(out.len().min(64));
                let from = rng.below(out.len() - len + 1);
                let to = rng.below(out.len() - len + 1);
                let chunk = out[from..from + len].to_vec();
                out[to..to + len].copy_from_slice(&chunk);
            }
        }
    }
    out
}

/// How far a mutated container got, so that a run can say whether it was
/// testing anything.
///
/// **A fuzzer that never gets past the front door finds nothing, and looks
/// exactly like one that found nothing.** Most mutations of a ZIP archive stop
/// at "not an archive", which exercises about four lines. These counters are
/// what says whether the run reached the metadata parser, the name rules, and
/// the payload — and they are printed at the end for that reason rather than
/// for interest.
#[derive(Default)]
struct Reach {
    not_an_archive: u64,
    other_refusal: u64,
    undetermined: u64,
    out_of_scope: u64,
    conformant: u64,
    metadata_parsed: u64,
    payload_read: u64,
    repacked: u64,
}

/// Everything a caller can ask of a byte stream, asked of one.
///
/// The payload is read too, and to a bounded sink: a conformant container may
/// legitimately hold a large payload and the point here is a defect in this
/// crate, not how fast this machine can copy.
fn exercise(bytes: &[u8], reach: &mut Reach) {
    match slpc::validate(Cursor::new(bytes)) {
        Ok(slpc::Verdict::Conformant) => reach.conformant += 1,
        Ok(slpc::Verdict::Undetermined(_)) => reach.undetermined += 1,
        Ok(slpc::Verdict::OutOfScope(_)) => reach.out_of_scope += 1,
        Ok(slpc::Verdict::NonConformant(m)) => {
            if m.to_string().contains("not a readable ZIP archive") {
                reach.not_an_archive += 1;
            } else {
                reach.other_refusal += 1;
            }
        }
        _ => reach.other_refusal += 1,
    }
    if slpc::metadata_of(Cursor::new(bytes)).is_ok() {
        reach.metadata_parsed += 1;
    }
    if let Ok(mut c) = slpc::Container::read(Cursor::new(bytes)) {
        let _ = c.version().len();
        let _ = c.payload_name().len();
        let _ = c.payload_size();
        let _ = c.payload_mode();
        let _ = c.check_payload_readable();
        if let Ok(p) = c.payload() {
            // Bounded on the read, so a decompression bomb in a *payload* is
            // not mistaken for a hang in the reader. SPEC 6 bounds the metadata
            // member and deliberately does not bound this one: nothing inflates
            // a payload until a caller asks, and this caller is asking.
            if std::io::copy(&mut p.take(4 << 20), &mut std::io::sink()).is_ok() {
                reach.payload_read += 1;
            }
        }
    }

    // The rewrite path, which reading alone never touches. SPEC 3 requires that
    // members an implementation does not recognise survive a rewrite, so this
    // is the code that walks every member of a container somebody else wrote —
    // including the ones a mutation has made strange — and copies them through.
    let mut out = Cursor::new(Vec::new());
    if slpc::Repack::new(Cursor::new(bytes))
        .write(&mut out)
        .is_ok()
    {
        reach.repacked += 1;
        // And what came out is read back, because a rewrite that produces
        // something this crate cannot read is a defect the write path can only
        // be caught in by looking.
        let _ = slpc::validate(Cursor::new(out.into_inner()));
    }
}

fn seeds(conformance: &Path) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    let cases = conformance.join("cases");
    let mut stack = vec![cases.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "slpc") {
                if let Ok(b) = std::fs::read(&p) {
                    let name = p
                        .strip_prefix(&cases)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .into_owned();
                    found.push((name, b));
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn main() -> std::process::ExitCode {
    let args = Args::parse();

    let seeds = seeds(&args.conformance);
    if seeds.is_empty() {
        eprintln!(
            "fuzz: no containers under {}/cases — run `python3 generate.py` there first",
            args.conformance.display()
        );
        return std::process::ExitCode::FAILURE;
    }
    std::fs::create_dir_all(&args.out).expect("a directory to leave findings in");
    println!(
        "{} seeds, {} cases, seed {}",
        seeds.len(),
        args.cases,
        args.seed
    );

    // A panic in the reader is a finding rather than a reason to stop, so the
    // hook is silenced around the call that makes it — and only around that
    // call. Silencing it for the whole run hides a panic in this harness, which
    // is how the first version of it exited 101 with nothing to say.
    let default = std::panic::take_hook();

    let mut rng = Rng(args.seed | 1);
    let started = Instant::now();
    let mut panics = 0u64;
    let mut slow = 0u64;
    let mut reach = Reach::default();
    let current = args.out.join("current.bin");

    for case in 0..args.cases {
        let (name, seed) = &seeds[rng.below(seeds.len())];
        let bytes = mutate(seed, &mut rng);

        // Written before the run, so a case that hangs and is killed from
        // outside leaves the container that hung it.
        let _ = std::fs::write(&current, &bytes);

        let at = Instant::now();
        let mut this = Reach::default();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            exercise(&bytes, &mut this);
        }));
        std::panic::set_hook(Box::new(|info| eprintln!("fuzz harness panicked: {info}")));
        let took = at.elapsed();
        reach.not_an_archive += this.not_an_archive;
        reach.other_refusal += this.other_refusal;
        reach.undetermined += this.undetermined;
        reach.out_of_scope += this.out_of_scope;
        reach.conformant += this.conformant;
        reach.metadata_parsed += this.metadata_parsed;
        reach.payload_read += this.payload_read;
        reach.repacked += this.repacked;

        if outcome.is_err() {
            panics += 1;
            let path = args.out.join(format!("panic-{case:08}.slpc"));
            let _ = std::fs::write(&path, &bytes);
            println!("  PANIC  case {case} from {name} -> {}", path.display());
        } else if took > Duration::from_millis(args.slow_ms) {
            slow += 1;
            let path = args.out.join(format!("slow-{case:08}.slpc"));
            let _ = std::fs::write(&path, &bytes);
            println!(
                "  SLOW   case {case} took {:?} from {name} -> {}",
                took,
                path.display()
            );
        }

        if case > 0 && case % 10_000 == 0 {
            println!(
                "  {case} cases, {panics} panics, {slow} slow, {:?}",
                started.elapsed()
            );
        }
    }

    std::panic::set_hook(default);
    let _ = std::fs::remove_file(&current);
    println!(
        "{} cases in {:?}: {panics} panics, {slow} slow",
        args.cases,
        started.elapsed()
    );
    // What the run actually touched. A campaign whose cases are almost all
    // `not an archive` has tested the first four lines of the reader and
    // nothing else, and should say so rather than report a clean bill.
    println!(
        "  reached: {} not-an-archive, {} other refusal, {} undetermined, {} out-of-scope, {} conformant",
        reach.not_an_archive, reach.other_refusal, reach.undetermined, reach.out_of_scope, reach.conformant
    );
    println!(
        "           {} parsed the metadata, {} read a payload, {} rewrote and read back",
        reach.metadata_parsed, reach.payload_read, reach.repacked
    );
    if panics == 0 && slow == 0 {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}
