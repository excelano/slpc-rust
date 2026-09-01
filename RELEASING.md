# Releasing slipcase

The release loop lives in `~/notes/releasing.md` — the ordered steps, the apt
step, crates.io, the spent-tag rule, and the standing facts about tokens and
secrets. Failure recipes are in `~/notes/build_release_gotchas.md`. This file
carries what is true of slpc-rust and not of its siblings.

| | |
|---|---|
| Loop | cargo-dist |
| Version lives in | two places in the root `Cargo.toml` — see below |
| `apt-ship` argument | `slpc-rust` |
| crates | `slpc`, then `slipcase`, in that order |
| winget package | `Excelano.slipcase-cli` |
| Windows asset | `slipcase-x86_64-pc-windows-msvc.zip` |

The crates.io publish runs from this repository's own `publish-crate.yml` rather
than the fleet's shared workflow; that file states why, and why the order in the
table is not arbitrary.

**The winget identifier does not match the command.** The command is `slipcase`
and the identifier is `Excelano.slipcase-cli`, which is the one coordinate in
the table that carries a suffix. The plain name belongs to Slipcase Desktop,
which reaches Windows through the Microsoft Store and is therefore already
findable as `slipcase`; a CLI submitted under the same name would compete with
it in the one place a user cannot tell them apart. Everywhere else — the
command, the apt package, the Homebrew formula, both crates — stays unsuffixed.
This is the mismatch to get right when running komac: the asset URL carries
`slipcase-`, and only the identifier carries `-cli`.

**The moniker is `slipcase-cli` too, and that is the deliberate part.** Every
sibling uses its bare command name, so this one looks like an oversight and is
not. The moniker is what `winget install <name>` resolves against, and pointing
`slipcase` at this package would rebuild the ambiguity the identifier exists to
prevent. `winget install slipcase` should reach the desktop app.

## The version is written twice, and the second one is quiet

The two crates version in lockstep: one number, one tag, one release. Bump it in
`version` under `[workspace.package]` and in the `slpc` requirement under
`[workspace.dependencies]`, which sit adjacent for exactly this reason.

Getting the second wrong fails silently. A caret requirement on the old number
still resolves, so nothing errors — the published binary simply asks crates.io
for a library older than the one it shipped with.

Rename `CHANGELOG.md`'s `[Unreleased]` heading to `## [0.1.0] - <today>` in the
same commit. A version with no section in that file publishes empty release
notes rather than failing.

## The conformance corpus is not part of `cargo test`

Run it before tagging, against the corpus as it is rather than a pinned copy:

```sh
cargo build --workspace
(cd ../slipcase/conformance && python3 generate.py)
cargo run -p corpus -- ../slipcase/conformance
```

A disagreement stops the release without settling it: the corpus is not
normative and the specification wins over both. The runner refuses to report
success on a corpus it could not find or whose cases were never generated.

## Check what consumers will see

`slpc` is published as a crate in its own right rather than as the binary's
internals, so it has an audience that none of the binary's channels exercise:
someone who reads `docs.rs/slpc` and compiles against the packaged tarball.
Those three commands are what that audience actually receives.

```sh
cargo package -p slpc --list
cargo publish -p slpc --dry-run
cargo doc --no-deps --all-features --open
```

That dry run compiles the packaged copy with **default features only**, and
`cargo doc` without `--all-features` hides every item behind one. Build the
packaged tree again to cover the rest:

```sh
cargo test --manifest-path <target-dir>/package/slpc-0.1.0/Cargo.toml --all-features
```

Then use the library as a caller would, before the version is spent:

```sh
cargo new /tmp/consumer && cd /tmp/consumer
cargo add slpc --path ~/slipcase/slpc-rust/slpc --features fs
```

Write the few lines a caller writes — open a container, report what is in it,
write one to a path — and compile them. Reading the signatures is not the same
check: how two of them compose is where the defects are.

`cargo publish -p slipcase --dry-run` **cannot run until the library is
published**, since it resolves `slpc` from crates.io. That ordering is what the
rest of the loop depends on.

## Two checks the workspace layout needs

`cargo deb -p slipcase` builds the package; `dpkg-deb -c` and `dpkg-deb -I` read
it back. The `-p` is load-bearing — the workspace default is not a package.

`dist plan` should list five platform archives, the shell and PowerShell
installers, the Homebrew formula, and the checksums, naming `slipcase` and never
`slpc`. The library has no binary to ship and must not appear.

## When a format change is a breaking change

The Rust signatures are the smaller half of this library's interface. The larger
half is what counts as a conformant container, and that is `SPEC.md` in
`excelano/slipcase`. A change here that accepts or rejects a container
differently from before is a bug or a specification change, and goes upstream
either way.

Within that, widening what the library accepts is a minor bump and narrowing it
is breaking, even when no type changed. `slpc/tests/` is the record of what has
been promised; a case removed or changed there is the signal to think about the
version number.

**The two halves of that sentence can disagree, and the second one decides.**
0.3.6 narrowed what the library accepts: a container whose metadata member
exceeds the bound now reports undetermined where it reported conformant. By the
first half that is breaking. By the second nothing moved — `slpc/tests/` gained
nine cases and lost none, because no test had ever promised to accept a
container the new bound refuses. It shipped as a patch. The rule to take from it
is that the promise is what the suite records, not what the implementation
happened to do beside it, and that a narrowing which breaks no recorded promise
is a fix rather than a break. A narrowing that changes an existing case is the
other thing, and that is what the version number is for.
