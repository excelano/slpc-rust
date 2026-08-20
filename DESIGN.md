# slpc-rust — Design Document

**Status:** design draft. Nothing built.
**Document version:** draft, 2026-08-20
**Implements:** slipcase specification 1.0

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

- **ZIP** — a maintained pure-Rust crate, deflate only.
- **TOML** — a crate implementing TOML 1.1.0, with a document model that preserves the original text on round-trip rather than a serde struct mapping. The specification requires preserving keys an implementation does not recognize, and deserializing into a struct drops them. A document model preserves comments, key order, and whitespace as well, so a person who hand-wrote the metadata gets it back as they wrote it.
- **CLI argument parsing** — clap with the derive feature, which every Rust binary in the fleet uses.
- **Error types** — none taken. The library declares its own error enum with a `Display` impl and a bare `std::error::Error`, which is what the fleet's published crates do: neither `thiserror` nor `anyhow` appears in `xaddr` or `encsniff`. Three families over a handful of variants do not earn a macro, and a crate meant to be linked without deliberation should add nothing to a consumer's tree.

Four things to verify, because each is an assumption about the ZIP crate rather than about the format. The first two gate the read path, the third rewriting, the fourth packing from a reader:

- that member names are decoded per general purpose bit 11 — UTF-8 when set, CP437 otherwise — as the specification requires for matching `payload.file`;
- that the crate exposes enough of a member's external attributes to detect a symlink entry;
- that a member can be copied from one archive to another without being decompressed, so that a container survives a rewrite whether or not this build can read every member in it; and
- that a member can be written from a source whose length is not known in advance, by data descriptor or by spooling.

If any is missing, it is handled here rather than worked around in the format.

**The minimum supported Rust version comes from the dependencies rather than from this code.** The fleet measures the floor and declares it, rather than inferring it from the edition. Here the number is whatever the ZIP and TOML crates demand, which is above the rest of the fleet's and rises whenever either of them raises theirs. The manifest says so where it declares the number, because a floor inherited from a dependency moves without anyone here deciding that it should, and a consumer reading `rust-version` cannot otherwise tell the two cases apart.

---

## 4. The library

### 4.1 Shape

Reading needs `Read + Seek`, because a ZIP's central directory is at the end of the file. The public surface is small:

```rust
let c = Container::open("report.pdf.slpc")?;   // also: Container::read(reader)
c.version();          // the slipcase_version as written
c.payload_name();     // the value of payload.file
c.metadata();         // &Document — the whole TOML document, unknown keys intact
c.metadata_mut();     // &mut Document — changed in place
c.metadata_bytes();   // &[u8] — the metadata member as stored, byte for byte
let mut r = c.payload()?;   // impl Read — streams, never buffered whole

Container::pack_reader(payload_name, reader, metadata, writer)?;   // metadata: Into<Document>
Container::pack_file(&payload_path, metadata, writer)?;            // name taken from the path
Container::rewrite_metadata(reader, &document, writer)?;
Container::rewrite_metadata_bytes(reader, &bytes, writer)?;
slpc::validate(reader)?;
```

**The payload is never read into memory.** Payloads are arbitrary files of arbitrary size, and a library that returns `Vec<u8>` decides for its caller that the file fits in RAM.

**No vocabulary.** The library exposes the metadata document and typed accessors for the two structural keys. It defines no others, validates no others, and has no opinion on what any of them mean.

### 4.2 Metadata, at two levels

The document is the ordinary one, and it behaves as a map: indexable, iterable, and open to insertion and removal. A caller who wants to read the metadata into a table, change a key, and write it back does that on the document and keeps the comments, key order, and whitespace that §3 chose this representation to keep. A second, plain-map representation with a write path attached would offer a convenient way to discard all of that without noticing, so there is not one.

The bytes are for a caller who wants a different parser, a schema validator, or a hash. They are the member as stored, so a container can be re-emitted byte for byte, which no other path promises; the specification defines no canonical serialization, and this is the only way to be sure nothing moved. A signature mechanism, whenever one arrives — §5 of the specification leaves it out of this version — will need those bytes rather than a re-serialization of them. Reading them buffers, which the rule above permits: that rule is about payloads of arbitrary size, not about the metadata member.

**Building metadata is not the same operation as changing it.** A read-modify-write has formatting to preserve and goes through the document. Building metadata from nothing has none, and a caller generating it out of a database or a build system would rather hand over a struct or a map than assemble a document by hand, so both packing forms accept anything convertible into one, serde included. The conversion cannot lose anything, because nothing arriving that way carried anything to lose.

### 4.3 Packing

`pack_reader` takes a name and a `Read`, not a `Read + Seek`. Requiring seek would allow two passes over the payload to compute its length and checksum before the local header goes down, and it would also rule out pipes, sockets, and anything generated as it is written, which are the reason the reader form exists at all. The length is therefore unknown at the moment the header is written, and the member goes out with a data descriptor or is spooled first — the fourth item in §3. Either produces a conformant container; the specification constrains none of this.

`pack_file` has neither problem, because a file on disk can be measured and read twice.

**The two forms fail differently, and the errors say so.** `pack_reader` is handed a name by its caller, and that name has to satisfy the specification's rules for `payload.file`: non-empty, not `.` or `..`, no `/`, `\`, or `:`, and not `slipcase.metadata.toml`. `pack_file` derives the name instead, so its failure is that a file on disk is called something that cannot be a member name. One is a caller passing a bad argument and the other is a payload that cannot be packed as itself, and collapsing them would have the second complain about an argument the caller never supplied.

**The library sets both required keys itself.** `payload.file` is the name the payload is being written under, and `slipcase_version` is the version this build implements. The caller supplies neither and so cannot be inconsistent about either. Metadata arriving with a `payload.file` that disagrees with the name given is an error rather than a silent overwrite, because the caller meant one of the two and the library cannot tell which. Everything else in the document passes through untouched.

There is no bare `pack`. The two forms differ in more than convenience — one validates a name it was handed, the other derives a name and can fail on a file it cannot express — and a call site reads better for saying which one it meant. The read path keeps `open` and `read` rather than matching this, because `Container::open(path)` follows `File::open` and that convention is worth more than symmetry across the two halves of the API.

### 4.4 Rewriting

The specification requires that members an implementation does not recognize survive a rewrite. `rewrite_metadata` copies every member through and substitutes only `slipcase.metadata.toml`, which is why it is an associated function over a reader and a writer rather than a mutable container that gets saved: a rewrite that streams cannot accidentally hold the payload in memory, and a container is never partly in RAM and partly on disk.

Copying a member whose compression method the crate cannot decompress means copying its compressed bytes untouched, which is the third item in §3. It is what allows a container to be rewritten without being fully understood.

**The library validates what it is about to write.** Valid TOML 1.1.0, UTF-8, both required keys present and of the right type, and `payload.file` naming a member the archive actually contains. A key that is present and is a string still describes nothing if it points at a member that is not there, and a caller free to edit the document is free to break it that way. Without these checks, `rewrite_metadata_bytes` is a way to produce a non-conformant container from the reference implementation. The usual argument for the opposite is that malformed containers are needed for tests, and §7 answers it: the conformance corpus is built upstream with `zip` and a shell script, deliberately not with this tool.

### 4.5 Errors

- **I/O** — the file could not be read or written.
- **Malformed** — this is not a conformant container. Each variant names the rule it violates, so the message can point at a specification clause.
- **Unsupported** — this is or may be a conformant container, and this build cannot handle it. An encrypted member, a compression method the crate does not implement, a `slipcase_version` this build does not recognize.

The specification lists compression, encryption, and Zip64 among the properties a container must not be rejected for. "I cannot read this" and "this is invalid" are therefore different answers, and collapsing them would have the implementation reporting conformant containers as broken. Validation reads the central directory and the metadata member: it confirms that a member matching `payload.file` is present and is not a symlink entry, and it never decompresses the payload. A container whose payload uses a compression method this build cannot read therefore still validates.

### 4.6 Unrecognized versions

The specification requires that an implementation not assume it can read a container declaring a version it does not recognize. Parsing the metadata is how the version is discovered, so parsing and reporting are always allowed. Extracting the payload and rewriting the container are not: both refuse with `Unsupported`, naming the version found.

---

## 5. The CLI

Four verbs. Each does one thing the format supports.

- `pack <payload> [--name <n>] [--meta <file.toml>] [-o <out.slpc>]` — writes a container. Default output is the payload's name with `.slpc` appended, per the naming convention. With no `--meta`, generates metadata carrying only the two required keys.
- `unpack <file.slpc> [--dest <dir>] [--metadata]` — writes the payload. `--metadata` also writes `slipcase.metadata.toml`. Nothing else in the archive is written to disk, as the specification requires.
- `info <file.slpc>` — prints the metadata.
- `validate <file.slpc>` — reports conformance.

Behavior that is decided rather than obvious:

- **Both required keys are set for the user**, by the library rather than by the CLI: `payload.file` from the payload's own filename, `slipcase_version` from the build. §4.3. A `--meta` file that sets `payload.file` to something else is an error rather than a silent overwrite.
- **A payload whose filename cannot be a member name is rejected, not renamed.** A local file may contain characters that `payload.file` forbids; packing it would produce a container that cannot name its own payload.
- **Neither `pack` nor `unpack` overwrites an existing file without `--force`.**
- **Exit codes:** 0 for success or conformance, 1 for bad input, 2 for a bad command line. The split is about whose mistake it is: 2 says re-read `--help`, 1 says go and look at the file. A non-conformant container and an unreadable one share 1 rather than splitting, because the distinction is already carried in the message and a wider code space returns less than it costs.
- **`-` names standard input**, per the fleet convention, which is worth honoring because a caller writes `-` by reflex and a tool that reads it as a filename fails in a way that looks like the caller's own bug. `pack -` is the case the library's reader form exists for and streams without buffering, though it needs `--name`, since there is no filename to take `payload.file` from. The reading verbs cannot stream at all: a ZIP's central directory is at its end, so `info -`, `validate -`, and `unpack -` spool standard input to a temporary file and open that. The cost is the CLI's to pay rather than the library's, which keeps its `Read + Seek` bound and never spools for a caller who already has a file.
- `--version`, `-V`, `--help`, `-h`, per the fleet convention.

---

## 6. Testing

Two layers.

**Fixtures generated by the tests themselves**, covering the read and write paths, so the suite is self-contained and runs on a fresh clone with no network.

**The conformance corpus from `excelano/slipcase`**, consumed rather than vendored so that every language implementation tests against one source. It arrives as a pinned git submodule under `tests/`. This is the layer that matters: passing a corpus written against the prose, by someone reading the prose, is what makes the specification's claim to be implementable something other than an assertion.

---

## 7. Build order

1. **Read path** — open, parse, validate, expose the payload as a stream. Includes the first two ZIP-crate assumptions in §3.
2. **Write path** — packing from a reader and from a path, and rewriting a container's metadata with unknown keys and unknown members preserved. Includes the other two.
3. **CLI** over both.
4. **Wire in the conformance corpus** once the specification repository has one, keeping the generated fixtures for cases it does not cover.

The corpus is built upstream with `zip` and a shell script rather than with this implementation. A corpus produced by the tool it validates proves nothing, and building it with ordinary tools tests the format's own claim that ordinary tools suffice.

---

## 8. Non-goals

**A ZIP or TOML implementation.** Both are dependencies. Writing either would be the largest part of the codebase and the least interesting.

**An index verb, a corpus scanner, or anything that walks a directory tree.** Searching across many containers is a real need and not this format's problem to solve.

**Desktop integration** — an opener, a file association, an icon, a shell extension. Separate work, separate platforms, separate repository if ever.

**Metadata editing from the CLI.** The library exposes the document; a caller can change anything. A `set key=value` verb needs a convention for whether `3` is an integer or a string, and inventing one here would be defining a vocabulary the format deliberately does not have.

**Bindings for other languages.** Every other language gets a native implementation reading the same specification.

**Schema validation of metadata beyond the two structural keys.** There is nothing to validate against.
