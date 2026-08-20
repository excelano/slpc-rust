# slpc-rust — Design Document

**Status:** built and released. Which version is current, and what shipped with it, live in the git tags and on crates.io, which is where they stay right without anyone editing this file.
**Document version:** 2026-08-20
**Implements:** slipcase specification 1.0
**Section references:** `SPEC §2.3` is the specification in `excelano/slipcase`; a bare `§4.3` is this document. The two number their sections independently and both have a §3 and a §5, so neither is safe to read from context.

---

## 1. What this repository is

The Rust implementation of the slipcase container format: a library and a command-line tool, in one Cargo workspace.

- `slpc` — the library crate. Reads, writes, and validates containers.
- `slipcase` — the binary crate. A CLI over the library.

The specification lives in `excelano/slipcase` and is the authority. This repository is a reference implementation, which means it exists to demonstrate that the specification is implementable and to be checked against it — not to define anything.

**When the two disagree, the specification wins.** When the specification is silent or ambiguous, the fix goes upstream into the specification and comes back here. An implementation detail settled in this repository must never quietly become a format rule; if it turns out to be one, it belongs in `SPEC.md`.

---

## 2. Why one repository holds both crates

Splitting the library and the CLI into separate repositories means publishing to crates.io on every library change just to iterate the CLI against it. A workspace with a path dependency removes that loop entirely.

Repository names and published crate names are independent, so the two identities cost nothing: the repository is `slpc-rust`, the crates are `slpc` and `slipcase`. The rule across the project is that `slipcase` is what a person says and `slpc` is what a machine indexes — flat registries where names are scarce get `slpc`, path-namespaced things get `slipcase`.

crates.io publishes packages, not repositories, and knows nothing about where a package came from — the `repository` field is a URL and nothing more. A workspace publishing several crates is the ordinary Rust shape: tokio, serde, clap, and ripgrep all do it. Two consequences for the release path: the CLI's dependency on the library carries both a path and a version (`slpc = { path = "../slpc", version = "…" }`), since crates.io rejects a path-only dependency; and `slpc` publishes before `slipcase`, because the registry must already hold the version the binary asks for.

The two crates version in lockstep. The CLI is a thin shell over the library, and independent versions would mean per-crate tags and a release tool to manage them, against the fleet's one-version-per-repository convention. The workflow that publishes them is this repository's own rather than the fleet's shared one, which runs a bare `cargo publish` that a workspace root with two members rejects; this one names each crate and waits for the registry between them.

---

## 3. Dependencies

The format is ZIP plus TOML, and both have mature pure-Rust libraries. The implementation writes neither.

**Pure Rust, no C dependencies.** This applies to the whole tree, including transitive dependencies pulled in by optional features. The zip and compression crates ship optional backends that link C libraries; those features stay off, and the resulting build must cross-compile with nothing installed but a Rust toolchain.

- **ZIP** — `zip`, with `default-features = false` and `features = ["deflate-flate2-zlib-rs"]`. Its default set links libbz2 and libzstd through `bzip2-sys` and `zstd-sys`, so switching the defaults off is what the rule above demands rather than a preference, and asking for `deflate` instead of naming the flate2 backend pulls `zopfli` and three more crates to reach a compression level nothing here uses. What remains is fifteen crates with no C among them, and `cargo check` succeeds for `x86_64-pc-windows-msvc` on a machine carrying no MSVC toolchain.
- **TOML** — `toml_edit`, which implements TOML 1.1.0 as the specification requires, with a document model that preserves the original text on round-trip rather than a serde struct mapping. The specification requires preserving keys an implementation does not recognize, and deserializing into a struct drops them. A document model preserves comments, key order, and whitespace as well, so a person who hand-wrote the metadata gets it back as they wrote it.
- **CLI argument parsing** — clap with the derive feature, which every Rust binary in the fleet uses.
- **Temporary files** — `tempfile`, for spooling standard input and for writing a file that appears only once it is complete. Its tree is pure Rust: `libc` and `linux-raw-sys` are declarations rather than a C library to compile. It is the binary's dependency alone, so the library's tree stays at fifteen crates.
- **Error types** — none taken. The library declares its own error enum with a `Display` impl and a bare `std::error::Error`, which is what the fleet's published crates do: neither `thiserror` nor `anyhow` appears in `xaddr` or `encsniff`. Three families over a handful of variants do not earn a macro, and a crate meant to be linked without deliberation should add nothing to a consumer's tree.

Four things here were assumptions about the ZIP crate rather than about the format, and each was checked against `zip` 8.6 before any of the design leaned on it. All four hold.

**Member names are decoded per general purpose bit 11 by the crate itself**, UTF-8 when the flag is set and CP437 otherwise, which is what the specification requires for matching `payload.file`. The CP437 table is total over all 256 bytes. This is work the Go implementation has to do by hand and this one does not.

**An entry's type is readable.** `unix_mode` reads the high half of the external attributes, which is where every archiver that can express something other than an ordinary file puts it, whatever platform wrote the archive. That is all the crate can tell us; which types SPEC §2.3 excludes is read from the specification. An entry made on FAT carries no such bits and the crate synthesizes an ordinary file mode, so the answer there is that it is a regular file, which is both true and the safe direction to be wrong in.

**A member can be copied without being decompressed**, but only through `by_index_raw`. The obvious `by_index` refuses a member whose compression method this build does not carry, which is exactly the member the rewrite path exists to preserve. Encrypted members behave the same way and copy the same way, so SPEC §2.5 is satisfiable rather than aspirational.

**A member can be written from a source of unknown length**, and `ZipWriter::new_stream` does it over a writer that implements only `Write`, emitting a data descriptor. The writing half of the library therefore needs no `Seek` bound at all, which is a smaller demand on a caller than the design assumed.

Two further assumptions sat inside those four, unstated, and neither holds.

**The crate cannot count members.** `ZipArchive` keys its directory by name, so two members sharing one arrive as a single entry and `len()` counts them once. SPEC §2.1 requires exactly one member named `slipcase.metadata.toml` and exactly one matching `payload.file`, which is a question the crate cannot be asked. The central directory is therefore read here, in `central.rs`, for names alone: it counts, and members are still located and read through the crate. That module carries its own CP437 table, transcribed from the crate's, so the two cannot disagree about what a name decodes to.

The second is that a name the crate hands back can be trusted to be the name. When bit 11 is set but the name bytes are not valid UTF-8, the crate substitutes U+FFFD rather than reporting the problem, and nothing in the public API reports which decoding it chose — `ZipArchive` hands back no reference to its reader, so the flag word cannot be re-read either. Two members can then decode to the same name, and a `payload.file` carrying U+FFFD can match a member whose real name is something else, which puts the result at the mercy of member order that the specification forbids depending on. The rule that closes it needs no flag: CP437 decoding never produces U+FFFD, so a decoded name carrying one over raw bytes that are not valid UTF-8 came from the lossy branch, and that member's true name is not a Rust string and equals no `payload.file`, which always is one. Such a member never matches.

**The minimum supported Rust version is 1.88, and it comes from the dependencies rather than from this code.** The fleet measures the floor and declares it, rather than inferring it from the edition; 1.88 is what `zip` asks for, above `toml_edit`'s 1.85 and everything below them, and it was built and run rather than read off a manifest. The number is whatever the ZIP and TOML crates demand, which is above the rest of the fleet's and rises whenever either of them raises theirs. The manifest says so where it declares the number, because a floor inherited from a dependency moves without anyone here deciding that it should, and a consumer reading `rust-version` cannot otherwise tell the two cases apart.

---

## 4. The library

### 4.1 Shape

Reading needs `Read + Seek`, because a ZIP's central directory is at the end of the file. The public surface is small:

```rust
let mut c = Container::open("report.pdf.slpc")?;   // also: Container::read(reader)
c.version();          // the slipcase_version as written
c.payload_name();     // the value of payload.file
c.metadata();         // &DocumentMut — the whole TOML document, unknown keys intact
c.metadata_mut();     // &mut DocumentMut — changed in place
c.metadata_bytes();   // &[u8] — the metadata member as stored, byte for byte
let mut r = c.payload()?;   // impl Read — streams, never buffered whole

slpc::pack_reader(payload_name, reader, metadata, writer)?;   // metadata: Into<DocumentMut>
slpc::pack_file(&payload_path, metadata, writer)?;            // name taken from the path
slpc::rewrite_metadata(reader, &document, writer)?;
slpc::rewrite_metadata_bytes(reader, &bytes, writer)?;
slpc::validate(reader)?;   // -> Verdict

slpc::Repack::new(reader)          // change a container that already exists
    .metadata(&document)           // also: .metadata_bytes(&bytes)
    .payload(name, reader)         // also: .payload_file(&path)
    .write(writer)?;
```

The container is `mut` because the archive lends out one member at a time, which is the ZIP crate's shape rather than a choice made here.

**The payload is never read into memory.** Payloads are arbitrary files of arbitrary size, and a library that returns `Vec<u8>` decides for its caller that the file fits in RAM.

**Nothing on the write side takes or returns a container.** Each operation reads a stream and writes a stream, while `Container` means one thing, a container open for reading. Hanging the write path off it would be choosing a namespace rather than describing a relationship, which is the difference between `File::create` and `fs::copy`. That is why packing and rewriting metadata are free functions beside `validate`.

**`Repack` is a type because its arguments are optional and independent.** Metadata, payload, or both, and three functions each taking the source and one replacement do not compose into the fourth: running two of them in sequence would mean buffering a whole container between them. A builder says in one call what the container is to become and then makes one pass. `rewrite_metadata` and `rewrite_metadata_bytes` stay as they are, because the case with one thing to change should not need a builder to express, and because they were published before this existed.

**No vocabulary.** The library exposes the metadata document and typed accessors for the two structural keys. It defines no others, validates no others, and has no opinion on what any of them mean.

### 4.2 Metadata, at two levels

The document is the ordinary one, and it behaves as a map: indexable, iterable, and open to insertion and removal. A caller who wants to read the metadata into a table, change a key, and write it back does that on the document and keeps the comments, key order, and whitespace that §3 chose this representation to keep. A second, plain-map representation with a write path attached would offer a convenient way to discard all of that without noticing, so there is not one.

The bytes are for a caller who wants a different parser, a schema validator, or a hash. They are the member as stored, so a container can be re-emitted byte for byte, which no other path promises; the specification defines no canonical serialization, and this is the only way to be sure nothing moved. A signature mechanism, whenever one arrives — SPEC §5 leaves it out of this version — will need those bytes rather than a re-serialization of them. Reading them buffers, which the rule above permits: that rule is about payloads of arbitrary size, not about the metadata member.

**Building metadata is not the same operation as changing it.** A read-modify-write has formatting to preserve and goes through the document. Building metadata from nothing has none, and a caller generating it out of a database or a build system would rather hand over a struct or a map than assemble a document by hand, so both packing forms accept anything convertible into one, serde included. The conversion cannot lose anything, because nothing arriving that way carried anything to lose.

### 4.3 Packing

`pack_reader` takes a name and a `Read`, not a `Read + Seek`. Requiring seek would allow two passes over the payload to compute its length and checksum before the local header goes down, and it would also rule out pipes, sockets, and anything generated as it is written, which are the reason the reader form exists at all. The length is therefore unknown at the moment the header is written, and the member goes out with a data descriptor, which §3 confirmed the crate emits over a writer that implements only `Write`. That settles the other bound too: **packing** asks a caller for no `Seek` at either end, so a container can be packed from a pipe straight into a socket. The specification constrains none of this. Repacking is the other case and asks for both, for the reason in §4.4.

`pack_file` has neither problem, because a file on disk can be measured and read twice. It goes through the same streaming core anyway, and its member goes out with a data descriptor like the other's. Both are conformant, and one path is easier to keep right than two.

**The two forms fail differently, and the errors say so.** `pack_reader` is handed a name by its caller, and that name has to satisfy the specification's rules for `payload.file`: non-empty, not `.` or `..`, no `/`, `\`, or `:`, and not `slipcase.metadata.toml`. `pack_file` derives the name instead, so its failure is that a file on disk is called something that cannot be a member name. One is a caller passing a bad argument and the other is a payload that cannot be packed as itself, and collapsing them would have the second complain about an argument the caller never supplied.

**The library sets both required keys itself.** `payload.file` is the name the payload is being written under, and `slipcase_version` is the version this build implements. The caller supplies neither and so cannot be inconsistent about either. Metadata arriving with a `payload.file` that disagrees with the name given is an error rather than a silent overwrite, because the caller meant one of the two and the library cannot tell which. Everything else in the document passes through untouched.

There is no bare `pack`. The two forms differ in more than convenience — one validates a name it was handed, the other derives a name and can fail on a file it cannot express — and a call site reads better for saying which one it meant. The read path keeps `open` and `read` rather than matching this, because `Container::open(path)` follows `File::open` and that convention is worth more than symmetry across the two halves of the API.

### 4.4 Repacking

The specification requires that members an implementation does not recognize survive a rewrite. `Repack` copies every member through and substitutes only the ones being replaced, which is why it is a reader and a writer rather than a mutable container that gets saved: a rewrite that streams cannot accidentally hold the payload in memory, and a container is never partly in RAM and partly on disk.

Copying a member whose compression method the crate cannot decompress means copying its compressed bytes untouched, which §3 confirmed the crate will do. It is what allows a container to be rewritten without being fully understood. A member nothing is replacing comes out byte for byte, and that includes the metadata member when nothing about it changed — replacing a payload under the name the container already used leaves the document exactly where it was, rather than parsing and re-serializing it for no reason.

**`payload.file` is set by the library exactly when the library is writing the payload member.** Both packing forms set it, and so does repacking a payload. A metadata-only rewrite does not, because there the caller may be repointing the key at a member already in the archive and only they know which — the key is checked against the archive instead.

Where a payload does arrive with a name of its own, a document handed in has that key set rather than checked. This is the one place the library overwrites a value a caller supplied, and it is not the silent overwrite §4.3 refuses: the value the document carried named the member being replaced, so it is not a second opinion about what is being written, and the natural source of that document is the container itself, which necessarily still names the old payload. Bytes handed in are refused rather than corrected, because correcting them would mean they were no longer the bytes handed in, and storing them exactly is the only reason that form exists.

**A payload cannot be written under a name another member already carries.** SPEC §2.1 allows a container exactly one member under `payload.file`, and which of two was the payload would depend on the order they sat in. The name is therefore checked against the archive the payload is going into rather than the one it came from — the same archive here, and not the same elsewhere.

**Repacking writes to a stream it can seek in, and packing does not.** A member copied through already knows its compressed size. A writer that cannot seek has nowhere to put that size except a data descriptor after the data, which is a promise to a reader walking forward that a length is coming; writing the size into the local header is simpler and is the only form such a reader can use without decompressing. The bound costs a caller nothing they were not already paying, because repacking's source has to seek regardless — a ZIP's central directory is at the end of the file, so a pipe was never a possible source. Packing keeps its `Write`-only destination, since a payload arriving from a pipe genuinely has no size to write down.

The bug that made this urgent rather than merely tidy is worth recording, because it is invisible from inside: `zip` 8.6 sets the data descriptor flag on a raw-copied member in a stream writer and then writes no descriptor, so the local header claims a length of zero and nothing supplies the real one. Every reader that walks the central directory is unaffected, which is this library's own reader and every test it had; Info-ZIP walks forward, calls the result overlapping components, and exits 12. A test now asserts that no member comes out promising a descriptor, and it was watched failing against the old writer before it was kept.

**The library validates what it is about to write, against the rules it reads by.** Metadata is parsed and the payload located by the same code the read path uses, so what this writes is what it would accept back, and neither half can drift from the other or from a specification that grows a rule. A key that is present and is a string still describes nothing if it points at a member that is not there, and a caller free to edit the document is free to break it that way. Without these checks, `rewrite_metadata_bytes` is a way to produce a non-conformant container from the reference implementation. The usual argument for the opposite is that malformed containers are needed for tests, and §7 answers it: the conformance corpus is built upstream, deliberately not with this tool.

### 4.5 Errors and verdicts

- **I/O** — the file could not be read or written.
- **Malformed** — this is not a conformant container. Each variant names the rule it violates, so the message can point at a specification clause.
- **Unsupported** — this is or may be a conformant container, and this build cannot handle it. An encrypted member, a compression method the crate does not implement, a `slipcase_version` this build does not recognize.

**Validation returns a verdict rather than a yes or no.** Four answers, because two will not do: conformant, non-conformant with the rule it breaks, undetermined when the metadata member cannot be read at all, and out of scope when the container declares a version this build does not implement. SPEC §3 forbids reporting a container as conformant *or* as non-conformant when its metadata cannot be read, and SPEC §2.4 puts another version outside the question rather than failing it. A `Result<()>` can say neither thing, and the first version of this library discarded at its signature a distinction the error families below already drew.

The specification lists compression, encryption, and Zip64 among the properties a container must not be rejected for. "I cannot read this" and "this is invalid" are therefore different answers, and collapsing them would have the implementation reporting conformant containers as broken. Validation reads the central directory and the metadata member: it confirms that exactly one member matches `payload.file` and that the member is a regular file entry, and it never decompresses the payload. A container whose payload uses a compression method this build cannot read therefore still validates.

### 4.6 Unrecognized versions

The specification requires that an implementation not assume it can read a container declaring a version it does not recognize. Parsing the metadata is how the version is discovered, so parsing and reporting are always allowed. Extracting the payload and rewriting the container are not: both refuse with `Unsupported`, naming the version found.

---

## 5. The CLI

Five verbs. Each does one thing the format supports.

- `pack <payload> [--name <n>] [--meta <file.toml>] [-o <out.slpc>]` — writes a container. Default output is the payload's name with `.slpc` appended, per the naming convention. With no `--meta`, generates metadata carrying only the two required keys.
- `unpack <file.slpc> [--dest <dir>] [--metadata]` — writes the payload. `--metadata` also writes `slipcase.metadata.toml`. Nothing else in the archive is written to disk, as the specification requires.
- `repack <file.slpc> [--meta <file.toml>] [--payload <file>] [--name <n>] [-o <out.slpc>]` — changes the metadata, the payload, or both, and copies every other member through. At least one of the two, since a repack with nothing to change would read as a command that did something.
- `info <file.slpc>` — prints the metadata.
- `validate <file.slpc>` — reports conformance. A container declaring a version this build does not implement is not reported conformant, because everything past the two required keys is a rule this version's text states and none of it was checked. That is exit 3: a refusal to answer, not a verdict on the file.

Behavior that is decided rather than obvious:

- **Both required keys are set for the user**, by the library rather than by the CLI: `payload.file` from the payload's own filename, `slipcase_version` from the build. §4.3. A `--meta` file that sets `payload.file` to something else is an error rather than a silent overwrite.
- **A payload whose filename cannot be a member name is rejected, not renamed.** A local file may contain characters that `payload.file` forbids; packing it would produce a container that cannot name its own payload.
- **`repack` exists because unpacking and packing again is not the same operation.** SPEC §3 requires that members an implementation does not recognize survive a rewrite, and unpack-then-pack discards every one of them. Without this verb the tool has no way to change a container that keeps what it does not understand, and the workflow it teaches instead is the one the specification forbids.
- **`repack` writes back over the container it was given**, unless `-o` names somewhere else. A verb that named its target and then refused to touch it would send every caller through `repack -o tmp && mv tmp target`, which is the same operation with the atomicity taken out and no cleanup on failure. `--force` is not the gate here, because `--force` means "there is an unrelated file in your way": naming the container is the consent. What is at risk is bounded to the metadata document the caller replaced, since the payload and every other member are copied through untouched.
- **`repack` reads back what it wrote before replacing anything.** The library validates the metadata it is about to store, so this checks the archive around it. It costs a central-directory read of a file already in the page cache, and it is the difference between replacing the only copy of a container on faith and doing it on evidence.
- **Writing in place resolves the path first**, so a container reached through a symbolic link is replaced rather than the link being replaced by a file, and the container's permissions are carried onto the temporary file before the rename. What a rename cannot carry is ownership, which is the standing cost of replacing a file rather than writing into it, and it is shared with `sed -i` and with every editor's default.
- **A file this tool creates comes out with the permissions any other new file would have.** Writing through a temporary file has a cost its atomicity does not advertise: a temporary file is created private to its owner, and a rename carries that mode onto the destination, so every container packed and every payload unpacked came out 0600 rather than what the umask decided. There is no portable way to read a umask without setting it, which needs a C call and the `unsafe` this codebase forbids, so the mode is measured instead — a file is created the ordinary way beside the temporary one, asked what it got, and removed. Three system calls, once per run. Replacing a file rather than creating one is the other case, and takes that file's own mode; §4.4's verb is the only one that does.
- **Neither `pack` nor `unpack` overwrites an existing file without `--force`.** Both write to a temporary file beside the destination and rename it into place, so a run that fails partway leaves nothing behind rather than a truncated container that looks like one, and `--force` cannot destroy the old file and then fail to produce the new. `unpack --metadata` reserves both destinations before writing either.
- **Exit codes:** 0 for success or conformance, 1 for bad input, 2 for a bad command line, 3 for no verdict. The first three split on whose mistake it is: 2 says re-read `--help`, 1 says go and look at the file, and a non-conformant container shares 1 with an unreadable one because that distinction is carried in the message. The fourth is against the fleet's convention, which keeps the space to three, and it is earned because this distinction is normative rather than convenient: a container whose metadata cannot be read, or which declares another version, is one SPEC §3 forbids calling non-conformant, and with one code for both a caller branching on the status reads it as exactly that.
- **`-` names standard input where a file is read, and standard output where one is written.** The reading half is the fleet convention, which is worth honoring because a caller writes `-` by reflex and a tool that reads it as a filename fails in a way that looks like the caller's own bug. The writing half is this tool's own extension of it, and it is what lets a container move through a pipeline: `info | edit | repack --meta -` is the shape the verb is for. Writing a container to a terminal is refused rather than done, since a screenful of ZIP helps nobody. Only one argument may be `-`, because there is one standard input. Standard output is spooled to a temporary file and copied out at the end, the mirror of what the reading verbs already do with standard input: it is what gives repacking the seekable destination §4.4 wants, and it means a pipeline never receives the first half of a container that then failed. `pack -` is the case the library's reader form exists for and streams without buffering, though it needs `--name`, since there is no filename to take `payload.file` from. The reading verbs cannot stream at all: a ZIP's central directory is at its end, so `info -`, `validate -`, and `unpack -` spool standard input to a temporary file and open that. The cost is the CLI's to pay rather than the library's, which keeps its `Read + Seek` bound and never spools for a caller who already has a file.
- `--version`, `-V`, `--help`, `-h`, per the fleet convention.

---

## 6. Testing

Two layers.

**Fixtures the tests build themselves.** Every archive the suite reads is stamped byte by byte in `tests/support`, including the ones no ordinary writer will produce: a CP437 member name, a name flagged UTF-8 that is not one, two members sharing a name, a payload declaring a compression method this build lacks. Nothing binary is committed, so nothing in the history is opaque to review; what a fixture is testing is in the code that builds it rather than in a file that can only be hexdumped; and a fixture cannot go stale against a constant it shares with the crate.

None of that is self-containment and it should not be sold as such. `cargo test` fetches this workspace's dependencies before it runs anything, so the suite has never been able to run on a machine with no network, and generating fixtures buys nothing toward that.

**The conformance corpus from `excelano/slipcase`**, run against a version being released rather than against every commit. There is one corpus and every implementation answers to it, so it is consumed where it lives rather than copied or pinned here. A pinned copy would freeze the arbiter: the corpus changes when the specification is clarified, and a frozen one cannot notice. It would also put a clone of the specification repository into every checkout of this one, including the five cargo-dist makes on each release.

Running it needs that repository checked out and Python 3.11 or later to generate the cases, neither of which `cargo test` implies. That is why it is a command rather than a test: a test would have to choose between skipping quietly, which reports green having proved nothing, and failing on a machine that was never going to have those things. `corpus/` in this workspace is that command. It refuses to report success on a corpus it could not find, on one whose cases have not been generated, and on one holding containers the manifest does not describe, because a check that passes having done nothing is worse than no check.

It checks two things per case, not one. The verdict the library reaches, and the exit code the tool returns. The second is not a restatement of the first: `slipcase --help` states a four-code contract, three of whose codes exist because SPEC §3 forbids reporting a container this build cannot judge as one it has judged, and nothing else runs that mapping across a corpus of containers built to break it.

This is the layer that matters. Passing a corpus written against the prose, by someone reading the prose, is what makes the specification's claim to be implementable something other than an assertion — and that is a claim about a release, which is when it is made.

---

## 7. Build order

1. **Read path** — open, parse, validate, expose the payload as a stream. Settles how names are decoded and compared, which everything else then reuses.
2. **Write path** — packing from a reader and from a path, and rewriting a container's metadata with unknown keys and unknown members preserved.
3. **CLI** over both.
4. **The corpus runner**, keeping the generated fixtures for what the corpus does not cover.

The corpus is built upstream rather than with this implementation, and a corpus produced by the tool it validates proves nothing. How it is built is the specification repository's business to describe.

---

## 8. Non-goals

**A ZIP or TOML implementation.** Both are dependencies. Writing either would be the largest part of the codebase and the least interesting.

**An index verb, a corpus scanner, or anything that walks a directory tree.** Searching across many containers is a real need and not this format's problem to solve.

**Desktop integration** — an opener, a file association, an icon, a shell extension. Separate work, separate platforms, separate repository if ever.

**Key-level metadata editing from the CLI.** `repack --meta` replaces the document wholesale, which needs no syntax of its own: a TOML file goes in as a TOML file. A `set key=value` verb would need a convention for whether `3` is an integer or a string, and inventing one here would be defining a vocabulary the format deliberately does not have. SPEC §5 leaves a vocabulary out of this version rather than out of every version, so the name stays free for one that has something to operate on.

**Bindings for other languages.** Every other language gets a native implementation reading the same specification.

**Schema validation of metadata beyond the two structural keys.** There is nothing to validate against.
