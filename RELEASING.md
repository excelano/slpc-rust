# Releasing

The loop for a new version. Run it from a clean `main` with the working tree committed. Examples cut `v0.1.0`.

**The two crates version in lockstep**: one number in `[workspace.package]`, one tag, one release.

## The loop

1. **Bump the version, in two adjacent places** in the root `Cargo.toml`: `version` under `[workspace.package]`, and the `slpc` requirement under `[workspace.dependencies]`. Getting the second wrong is quiet — a caret requirement on the old number still resolves, and the published binary asks crates.io for a library older than the one it shipped with.

   Then update `Cargo.lock` with a build, run `cargo test --workspace --all-features`, rename `CHANGELOG.md`'s `[Unreleased]` heading to `## [0.1.0] - <today>`, and commit. A version with no section in that file publishes empty release notes rather than failing.

2. **Run the conformance corpus.** It is not part of `cargo test`:

   ```sh
   cargo build --workspace
   (cd ../slipcase/conformance && python3 generate.py)
   cargo run -p corpus -- ../slipcase/conformance
   ```

   Against the corpus as it is, not a pinned copy. A disagreement stops the release without settling it: the corpus is not normative and the specification wins over both. The runner refuses to report success on a corpus it could not find or whose cases were never generated.

3. **Check what consumers will see.**

   ```sh
   cargo package -p slpc --list
   cargo publish -p slpc --dry-run
   cargo doc --no-deps --all-features --open
   ```

   That dry run compiles the packaged copy with **default features only**, and `cargo doc` without `--all-features` hides every item behind one. Build the packaged tree again to cover the rest:

   ```sh
   cargo test --manifest-path <target-dir>/package/slpc-0.1.0/Cargo.toml --all-features
   ```

   Then use the library as a consumer would, before the version is spent:

   ```sh
   cargo new /tmp/consumer && cd /tmp/consumer
   cargo add slpc --path ~/slipcase/slpc-rust/slpc --features fs
   ```

   Write the few lines a caller writes — open a container, report what is in it, write one to a path — and compile them. Reading the signatures is not the same check: how two of them compose is where the defects are.

   `cargo publish -p slipcase --dry-run` **cannot run until the library is published**, since it resolves `slpc` from crates.io. That ordering is what the rest of the loop depends on.

4. **Verify the Debian package builds.** `cargo deb -p slipcase`, then `dpkg-deb -c` and `dpkg-deb -I` on the result.

5. **Check the release plan.** `dist plan` should list five platform archives, the shell and PowerShell installers, the Homebrew formula, and the checksums, naming `slipcase` and never `slpc`.

6. **Tag and push.** `git tag v0.1.0 && git push origin main --tags`, which triggers `release.yml` for the archives and the GitHub release, and `publish-crate.yml` for `slpc` and then `slipcase` — that order, because crates.io must already hold the version the binary asks for. A crate already at that version is skipped, so a re-run finishes a half-done publish. Do not run `cargo publish` by hand.

   **Versions are immutable.** `cargo yank` hides a bad release from new resolution; a number is never republished. A fix is a fresh version.

7. **Build the .debs.** They do not fire on their own: GitHub does not raise `release: published` for a release cargo-dist created with the default token.

   ```sh
   gh workflow run deb.yml -f tag=v0.1.0
   gh run list --limit 5
   gh release view v0.1.0
   ```

8. **Ship the .debs to the Excelano apt repository.** The argument is the *repository* name rather than the binary's:

   ```sh
   apt-ship slpc-rust v0.1.0
   ```

   It pools each `.deb`, prunes to the retention policy, re-signs the indices, previews the rsync, refuses to deploy any deletion the prune did not make, pushes, and verifies against the live index on both architectures.

   **Do not run `apt-ship -n` first.** Its dry run prunes for real, so the deploy after it aborts on deletions that run already made. `anderix/bin#4`. Run it once, with `-y` if unattended.

   **This is the step a release loses**, since nothing downstream depends on apt. `fleet -r` catches it, as an `APT` column reading `behind`; run it afterwards. Nothing to commit in the apt repository — `dists/` and `pool/` are gitignored.

## What is not automated

**Nothing publishes without a tag you pushed.** Both workflows fire on `v*`.

**The crates.io step is token-gated** and skips cleanly without `CRATES_IO_TOKEN`.

## When a format change is a breaking change

The Rust signatures are the smaller half of this library's interface. The larger half is what counts as a conformant container, and that is `SPEC.md` in `excelano/slipcase`. A change here that accepts or rejects a container differently from before is a bug or a specification change, and goes upstream either way.

Within that, widening what the library accepts is a minor bump and narrowing it is breaking, even when no type changed. `slpc/tests/` is the record of what has been promised; a case removed or changed there is the signal to think about the version number.
