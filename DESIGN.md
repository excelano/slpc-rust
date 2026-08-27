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

Separate repositories would mean publishing to crates.io on every library change just to iterate the CLI against it. A workspace with a path dependency removes that loop.

Repository and crate names are independent: the repository is `slpc-rust`, the crates are `slpc` and `slipcase`. The rule across the project is that `slipcase` is what a person says and `slpc` is what a machine indexes — flat registries where names are scarce get `slpc`, path-namespaced things get `slipcase`.

Two consequences for the release path. The CLI's dependency on the library carries both a path and a version, since crates.io rejects a path-only dependency; and `slpc` publishes before `slipcase`, because the registry must already hold the version the binary asks for.

The two crates version in lockstep, the CLI being a thin shell over the library. Independent versions would mean per-crate tags against the fleet's one-version-per-repository convention. The workflow that publishes them is this repository's own rather than the fleet's shared one, which runs a bare `cargo publish` that a workspace root with two members rejects.

---

## 3. Dependencies

The format is ZIP plus TOML, and both have mature pure-Rust libraries. The implementation writes neither.

**Pure Rust, no C dependencies**, including transitive ones pulled in by optional features. The zip and compression crates ship optional backends that link C libraries; those stay off, and the build must cross-compile with nothing installed but a Rust toolchain.

- **ZIP** — `zip`, with `default-features = false` and `features = ["deflate-flate2-zlib-rs"]`. Its defaults link libbz2 and libzstd through `bzip2-sys` and `zstd-sys`, and the plain `deflate` feature pulls `zopfli` and three more crates for a compression level nothing here uses. What remains is fifteen crates with no C among them, and `cargo check` succeeds for `x86_64-pc-windows-msvc` on a machine carrying no MSVC toolchain.
- **TOML** — `toml_edit`, which implements TOML 1.1.0 as the specification requires. The specification requires preserving keys an implementation does not recognize, and deserializing into a struct drops them. A document model preserves comments, key order, and whitespace as well.
- **CLI argument parsing** — clap with the derive feature, which every Rust binary in the fleet uses.
- **Temporary files** — `tempfile`, for writing a file that appears only once it is complete and for spooling standard input. Its tree is pure Rust: `libc` and `linux-raw-sys` are declarations rather than a C library to compile. It is optional in the library and reached only through the `fs` feature of §4.7, so the default tree stays at fifteen crates.
- **Error types** — none taken. The library declares its own error enum with a `Display` impl and a bare `std::error::Error`, which is what the fleet's published crates do. Three families over a handful of variants do not earn a macro, and a crate meant to be linked without deliberation should add nothing to a consumer's tree.

Four assumptions about the ZIP crate were checked against `zip` 8.6 before the design leaned on them. All four hold.

**Member names are decoded per general purpose bit 11 by the crate itself**, UTF-8 when the flag is set and CP437 otherwise, which is what the specification requires for matching `payload.file`. The CP437 table is total over all 256 bytes.

**An entry's type is readable.** `unix_mode` reads the high half of the external attributes, which is where any archiver that can express something other than an ordinary file puts it. Which types SPEC §2.3 excludes is read from the specification. An entry made on FAT carries no such bits and the crate synthesizes an ordinary file mode, which is both true and the safe direction to be wrong in.

**A member can be copied without being decompressed**, but only through `by_index_raw`. The obvious `by_index` refuses a member whose compression method this build does not carry, which is exactly the member the rewrite path exists to preserve. Encrypted members behave and copy the same way, so SPEC §2.5 is satisfiable rather than aspirational.

**A member can be written from a source of unknown length**, and `ZipWriter::new_stream` does it over a `Write`-only writer, emitting a data descriptor.

Two assumptions sat inside those, unstated, and neither holds.

**The crate cannot count members.** `ZipArchive` keys its directory by name, so two members sharing one arrive as a single entry. SPEC §2.1 requires exactly one member named `slipcase.metadata.toml` and exactly one matching `payload.file`, which is a question the crate cannot be asked. The central directory is therefore read here, in `central.rs`, for names alone; members are still located and read through the crate. That module carries its own CP437 table, transcribed from the crate's, so the two cannot disagree about what a name decodes to.

**A name the crate hands back cannot be trusted to be the name.** When bit 11 is set but the name bytes are not valid UTF-8, the crate substitutes U+FFFD rather than reporting it, and nothing in the public API says which decoding it chose. A `payload.file` carrying U+FFFD could then match a member whose real name is something else, putting the result at the mercy of member order the specification forbids depending on. The rule that closes it needs no flag: CP437 decoding never produces U+FFFD, so a decoded name carrying one over bytes that are not valid UTF-8 came from the lossy branch, and that member's true name is not a Rust string and equals no `payload.file`, which always is one. Such a member never matches.

**The minimum supported Rust version is 1.88, and it comes from the dependencies rather than from this code.** The fleet measures the floor and declares it; 1.88 is what `zip` asks for, above `toml_edit`'s 1.85 and everything below them, and it was built and run rather than read off a manifest. It rises whenever the ZIP or TOML crates raise theirs, which is why the manifest says so where it declares the number: a consumer reading `rust-version` cannot otherwise tell an inherited floor from a chosen one.

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
c.payload_size()?;    // u64 — uncompressed, read off the central directory
c.check_payload_readable()?;  // -> Result<(), Unsupported> — can this build decode it
let mut r = c.payload()?;   // impl Read — streams, never buffered whole

slpc::metadata_of(reader)?;   // -> DocumentMut — the document, no verdict attached

slpc::pack_reader(payload_name, reader, metadata, writer)?;   // metadata: Into<DocumentMut>
slpc::pack_file(&payload_path, metadata, writer)?;            // name taken from the path
slpc::rewrite_metadata(reader, &document, writer)?;
slpc::rewrite_metadata_bytes(reader, &bytes, writer)?;
slpc::validate(reader)?;   // -> Verdict

slpc::Repack::new(reader)          // change a container that already exists
    .metadata(&document)           // also: .metadata_bytes(&bytes)
    .payload(name, reader)         // also: .payload_file(&path)
    .write(writer)?;

// With the `fs` feature. §4.7.
let mut out = slpc::Destination::new(&path, force)?;   // also: ::in_place(&path)
slpc::pack_file(&payload_path, metadata, out.writer())?;
slpc::validate(out.written()?)?;   // read back before anything is replaced
out.commit()?;
```

The container is `mut` because the archive lends out one member at a time, which is the ZIP crate's shape rather than a choice made here.

**The payload is never read into memory.** A library that returns `Vec<u8>` decides for its caller that the file fits in RAM.

**Nothing on the write side takes or returns a container.** Each operation reads a stream and writes a stream. Hanging the write path off `Container` would be choosing a namespace rather than describing a relationship — the difference between `File::create` and `fs::copy`.

**`Repack` is a type because its arguments are optional and independent.** Three functions each taking the source and one replacement do not compose into the fourth: running two in sequence would mean buffering a whole container between them. `rewrite_metadata` and `rewrite_metadata_bytes` stay as they are, since the case with one thing to change should not need a builder, and they were published before this existed.

**No vocabulary.** The library exposes the metadata document and typed accessors for the two structural keys. It defines no others, validates no others, and has no opinion on what any of them mean.

### 4.2 Metadata, at two levels

The document behaves as a map: indexable, iterable, open to insertion and removal. Read-modify-write goes through it and keeps the comments, key order, and whitespace §3 chose this representation for. A second plain-map representation with a write path attached would be a convenient way to discard all of that without noticing, so there is not one.

The bytes are for a caller who wants a different parser, a schema validator, or a hash. They are the member as stored, so a container can be re-emitted byte for byte, which no other path promises: the specification defines no canonical serialization. A signature mechanism, whenever one arrives, will need those bytes rather than a re-serialization of them. Reading them buffers, which the rule above permits — that rule is about payloads of arbitrary size, not about the metadata member.

**Building metadata is not the same operation as changing it.** Building from nothing has no formatting to preserve, and a caller generating metadata out of a database or a build system would rather hand over a struct or a map than assemble a document by hand. Both packing forms therefore accept anything convertible into one, serde included.

### 4.3 Packing

`pack_reader` takes a name and a `Read`, not a `Read + Seek`: requiring seek would rule out pipes, sockets, and anything generated as it is written, which is why the reader form exists. The payload's length is unknown when the local header goes down, so the member carries a data descriptor, which §3 confirmed the crate emits over a `Write`-only writer. Packing therefore asks for no `Seek` at either end, and a container can be packed from a pipe straight into a socket. Repacking is the other case, for the reason in §4.4.

`pack_file` could measure a file and read it twice, and does not: it goes through the same streaming core, and one path is easier to keep right than two.

**The two forms fail differently, and the errors say so.** `pack_reader` is handed a name and checks it against SPEC §2.3; `pack_file` derives one, so its failure is that a file on disk is called something no member can be called. Collapsing them would have the second complain about an argument the caller never supplied.

**The library sets both required keys itself** — `payload.file` from the name being written, `slipcase_version` from the build — so a caller cannot be inconsistent about either. Metadata arriving with a `payload.file` that disagrees is an error rather than a silent overwrite, since the library cannot tell which of the two was meant. Everything else in the document passes through untouched.

**There is no bare `pack`.** The two forms differ in more than convenience, and a call site reads better for saying which it meant. The read path keeps `open` and `read` rather than matching this, because `Container::open` follows `File::open`.

### 4.4 Repacking

The specification requires that members an implementation does not recognize survive a rewrite. `Repack` copies every member through and substitutes only the ones being replaced, streaming rather than holding a container in memory. Copying a member whose compression method the crate cannot decompress means copying its compressed bytes untouched, which §3 confirmed the crate will do, and it is what allows a container to be rewritten without being fully understood. A member nothing replaces comes out byte for byte, the metadata member included when nothing about it changed.

**`payload.file` is set by the library exactly when the library is writing the payload member.** Both packing forms set it, and so does repacking a payload. A metadata-only rewrite does not, because there the caller may be repointing the key at a member already in the archive and only they know which; the key is checked against the archive instead.

Where a payload does arrive with a name of its own, a document handed in has that key set rather than checked. This is the one place the library overwrites a value a caller supplied, and it is not the silent overwrite §4.3 refuses: the value the document carried named the member being replaced. Bytes handed in are refused rather than corrected, since correcting them would mean they were no longer the bytes handed in.

**A payload cannot be written under a name another member already carries.** SPEC §2.1 allows exactly one member under `payload.file`, and which of two was the payload would depend on the order they sat in. The name is therefore checked against the archive the payload is going into rather than the one it came from.

**Repacking writes to a stream it can seek in, and packing does not.** A member copied through already knows its compressed size, and a writer that cannot seek has nowhere to put it but a data descriptor after the data — a promise to a reader walking forward that a length is coming. The bound costs a caller nothing, since repacking's source has to seek regardless: a ZIP's central directory is at the end of the file, so a pipe was never a possible source.

The defect behind it is invisible from inside: `zip` 8.6 sets the data descriptor flag on a raw-copied member in a stream writer and then writes no descriptor, so the local header claims a length of zero. Readers that walk the central directory are unaffected, which is this library's own reader and every test it had; Info-ZIP walks forward and exits 12. A test now asserts that no member comes out promising a descriptor.

**The library validates what it is about to write, against the rules it reads by.** Metadata is parsed and the payload located by the same code the read path uses, so what this writes is what it would accept back and neither half can drift from the other. Without these checks, `rewrite_metadata_bytes` is a way to produce a non-conformant container from the reference implementation. Malformed containers for tests come from the conformance corpus, which §7 builds upstream and deliberately not with this tool.

### 4.5 Errors and verdicts

- **I/O** — the file could not be read or written.
- **Malformed** — this is not a conformant container. Each variant names the rule it violates, so the message can point at a specification clause.
- **Unsupported** — this is or may be a conformant container, and this build cannot handle it. An encrypted member, a compression method the crate does not implement, a `slipcase_version` this build does not recognize.

**Validation returns a verdict rather than a yes or no.** Four answers, because two will not do: conformant, non-conformant with the rule it breaks, undetermined when the metadata member cannot be read at all, and out of scope when the container declares a version this build does not implement. SPEC §3 forbids reporting a container as conformant *or* as non-conformant when its metadata cannot be read, and SPEC §2.4 puts another version outside the question rather than failing it. A `Result<()>` can say neither thing.

SPEC §2.5 lists compression, encryption, and Zip64 among the properties a container must not be rejected for, so "I cannot read this" and "this is invalid" are different answers. Validation reads the central directory and the metadata member, confirms that exactly one member matches `payload.file` and that the member is a regular file entry, and never decompresses the payload — so a container whose payload uses a compression method this build lacks still validates.

### 4.6 Unrecognized versions

The specification requires that an implementation not assume it can read a container declaring a version it does not recognize. Parsing the metadata is how the version is discovered, so parsing and reporting are always allowed. Extracting the payload and rewriting the container are not: both refuse with `Unsupported`, naming the version found. `payload_size` refuses with the rest, since the payload was never located.

### 4.7 Putting a container on disk

Everything in §4.1 writes into a stream the caller supplies, which is the right shape for a library: a sink can be a file, a socket, a buffer, or a pipe. What it left open is that the reading side has taken a path since 0.1.0 through `Container::open` and the writing side never had the equivalent, so every caller putting a container on disk supplied that half itself.

`Destination`, behind the `fs` feature, is that half. It writes through a temporary file beside the destination and renames it into place, so a write that fails partway leaves nothing behind rather than a truncated container that looks like one. It lends out a `File`, which is the `Write + Seek` §4.4 asks for and lets a caller read back its own output before anything is replaced by it.

**It is a feature rather than part of the default surface.** A caller who only reads containers should not acquire a temporary-file dependency to do it, and §3's fifteen-crate tree is worth keeping true for them. Both binaries in this workspace turn it on, and both were already carrying those crates.

**Why the library rather than each binary.** That the code has no format knowledge is not a reason to exclude it: `Container::open` has none either. What decides it is what the code contains — files came out `0600` from 0.1.0 to 0.3.0 because a rename carries a private mode, and a defect held once is fixed where a defect copied is rediscovered.

**Two mode policies, deliberately.** `Destination::new` gives a new file what the umask decided, whether or not `force` replaced something in the way — a file that happened to be there does not get to decide who can read what replaces it. `Destination::in_place` resolves symbolic links first and carries the replaced file's own permissions across. What a rename cannot carry is ownership, which is the standing cost of replacing a file rather than writing into it, shared with `sed -i` and with every editor's default.

**The umask is measured rather than read.** A new file gets `0666` with the umask taken out of it, and there is no portable way to read a umask without setting it, which needs a C call and the `unsafe` this crate forbids. So a file is created the ordinary way beside the temporary one, asked what it got, and removed: three system calls, and only where there is no file to take a mode from.

**Errors stay in the three families.** A destination that already exists reports `Error::Io` carrying `ErrorKind::AlreadyExists`. The library says nothing about how a caller lets someone override that, because it has no flags; naming `--force` is the CLI's job.

### 4.8 A document without a verdict

`metadata_of` reads the metadata member and parses it, requiring of that member what SPEC §2.2 requires — one of it, valid TOML, UTF-8 — and asking nothing else. It looks for neither required key and never locates a payload.

It exists because `Container::read` cannot say two things at once. A container whose `payload.file` names no member, names several, or names something SPEC §2.3 forbids has a metadata document that parsed cleanly, and the read path returns an error over the payload before a caller can reach it. So does one missing a required key. A program showing a person what is in a file wants to show them that document alongside the reason the container is not conformant, and had no way to get it.

**It is not a verdict and must not become one.** A document coming back says nothing about conformance; `validate` remains the only function here that answers the question SPEC §3 constrains, and the separation is what keeps a caller from reading "the metadata parsed" as "the container is fine". `slipcase info` gains from the same function: it refused a container whose metadata it could read perfectly well.

### 4.9 Whether the payload can be read

`check_payload_readable` answers, before anything is extracted, whether this build can decode the payload member. It refuses with the same three `Unsupported` variants `payload` does — an unrecognized version, an encrypted member, a compression method this build lacks — and it meets them in the order the ZIP crate does, so a member that is both encrypted and compressed by a method this build lacks is reported encrypted.

It exists because a program has to commit to an operation before performing it. A window putting an Open button on a payload card, or a plan stating what it is about to do, had no way to learn the answer except by attempting the extraction and reading it off the failure.

**It borrows shared and reads nothing.** Both facts are already in the central directory entry collected when the container was opened, which is where §4.1's `payload_size` gets its answer for the same reason. The probe it replaces — construct a payload reader and drop it — needed `&mut`, and it seeks and reads a local header on the way, so it could also fail for reasons that are not about capability at all.

**The library answers rather than the caller.** Exposing the compression method and the encryption flag would ask a caller to judge a method against a build, which it cannot do: cargo unifies features across the whole dependency graph, so another crate depending on `zip` with `zstd` widens what this library can decode without anything here changing. Which methods a build carries is a fact only that build holds.

**`Ok` is not a promise that extraction succeeds.** It says the decoder exists. Truncated data, a failed checksum, and an i/o error are still ahead, and `payload` reports them when they arrive.

**It is not a verdict and must not become one.** SPEC §2.5 puts compression and encryption outside the conformance question, so `validate` reports an encrypted payload conformant and this says the bytes are out of reach. Folding capability into conformance would make the verdict depend on which features the build was compiled with, which is the thing §4.5 keeps `Unsupported` separate from `Malformed` to prevent.

The risk it carries is drift: the two conditions are mirrored from inside the ZIP crate, and a version of it that added a third would have the check say yes where extraction says no. A test asserts the two answers agree across every fixture that reaches one, which is the pairing §6's corpus runner makes between verdict and exit code, applied to a smaller question.

---

### 4.10 A legal name that is not a file

`check_payload_name` answers whether a name is legal under SPEC §2.3. `payload_path` answers a question the specification does not ask: whether that legal name, joined to a directory, names a file on the machine doing the joining.

**The argument that `dir.join(name)` suffices was written independently in two codebases, and it is wrong in the same way in both.** It goes: `payload.file` is a plain filename, checked when the container was read against a rule that rejects every separator and every traversal, so joining it to a directory cannot leave that directory. That is true and it is not the question. Win32 resolves `CON`, `COM1`, `AUX`, `LPT1`, `PRN` and `NUL` to devices wherever the name appears, with or without an extension and in any case. `CON` does not leave the directory. It is not in it.

**Measured rather than reasoned about**, in `excelano/slipcase-desktop` on 2026-08-26, and one name at a time because they do not agree with one another. Writing `CON` returned `Ok` at every step and left no file, the bytes having gone to the console; `metadata` then failed with code 87, and `std::fs::read` never returned at all, because it opens the console for reading and waits for input a windowed application will never supply. `LPT1` and `PRN` failed cleanly with `NotFound`. `NUL` succeeded and discarded. There is no single failure to code against, which is why the conformance corpus did not disagree over the case that carries such a name — it hung, twice, for ten minutes, before the case had a name.

**The repair is not a list of reserved names.** Windows looks for those names while it parses a path, and a path in the `\\?\` verbatim form is not parsed that way, so `canonicalize` is asked of the *directory* and the name is joined onto the answer. The name stops being a device without anything here calling it a bad name, and which names are devices stays Windows's business as that list changes. Three alternatives were rejected: refusing these names in `check_payload_name` makes a conformant container unopenable and contradicts SPEC §2.3; renaming the payload substitutes an implementation's judgement for the name a person chose; and refusing extraction with a sentence is the smallest of the three and still turns a conformant container into one this implementation will not open.

**Nowhere but Windows does anything.** `canonicalize` on Unix would also resolve symbolic links, which would quietly move where a caller's payload lands in order to fix a problem that platform does not have, so the other arm joins and returns. This is the first `cfg` in the library's source, and §3's rule is untouched by it: no dependency is added and nothing is compiled that was not compiled before.

**It reports where the file is, not how the caller spelled it.** `canonicalize` expands 8.3 short names and resolves junctions as well as adding the prefix, so a caller passing `C:\\Users\\RUNNER~1\\…` gets `C:\\Users\\runneradmin\\…` back. That is the canonical name of the same file and the one a person can paste into Explorer, so it is left as it comes rather than folded back to the caller's spelling — but a caller comparing the result against a path of their own has to compare files rather than strings. Measured on a Windows runner by a test that asserted the spelling and was wrong to.

**It costs a display rule, paid separately.** The path handed back is the verbatim one, because that is what addresses the file, and the prefix is how a path is addressed rather than part of its name. `display_path` takes it off and nothing else does, so *Extracted to* does not show somebody a spelling they have never seen and could not type. `already_exists` goes through it too, that message being read by a person deciding what to do about the file.

**What is not fixed is the handover.** A payload named for a device now extracts as an ordinary file and is read back byte for byte; asking the shell to open it still fails, with *the specified device name is invalid*. That is the truth about that container on that platform rather than a defect left standing.


### 4.11 Where a container came from

A container downloaded from the internet is marked as such by the platform that downloaded it: `com.apple.quarantine` on macOS, a `Zone.Identifier` alternate data stream on Windows, `user.xdg.origin.url` on Linux by freedesktop convention. All three are properties of the file rather than of its contents, so a payload written out of that container carries none of them unless something puts them there.

**Without that, unpacking is laundering.** Somebody downloads a container, takes the payload out, and it reaches its handler as something this machine made; the warning the platform would have raised never appears. That is the shape of defect that made disk images and archives the delivery vehicle of choice, and it is why refusing a payload name with a separator is not the end of what unpacking owes. `slipcase unpack` did exactly this from 0.1.0 until 0.3.5.

**Why the library rather than the caller**, which is §4.7's question and gets §4.7's answer. The code has no format knowledge, and neither has `Container::open`; what decides it is that a defect held once is fixed where a defect copied is rediscovered. The proof is on the record rather than hypothetical: `excelano/slipcase-desktop` wrote this module first, and the reasoning error §4.10 describes was written independently in both repositories, in nearly the same words. Two callers reasoned out the same half-question and both got it wrong.

**The policy is here and not split with the caller.** `carry` fails only when the platform gates opening on a mark, the source carries one, and the copy ends up carrying none. Everything else succeeds — no mark, no such mark on this platform, a note nothing enforces, a mark the platform wrote itself. So the whole of the rule for a caller about to hand a payload to the system is that an error means do not open it. It is deliberately a test of the copy rather than of the write's own success: under the macOS App Sandbox the platform marks whatever the calling process writes and then refuses to have that mark replaced, so the write fails while the gate it exists for is in place, and `Mark::AlreadyMarked` says so. Asking the file rather than the environment is also why nothing here asks whether it is sandboxed.

**Linux is a note and says so.** Nothing there consults `user.xdg.origin.url` before opening a file, so carrying it is hygiene rather than a control. `Mark::Noted` is a separate answer from `Mark::Carried` precisely so that nothing reads one as the other, and the Linux arm never fails: refusing to hand over a payload because a note nothing reads could not be written would be theatre.

**Off by default, and once separate from `fs`.** A caller who writes containers has no use for it, and one who only unpacks containers they made themselves does not need it either. It adds `xattr` on Unix and nothing at all on Windows, where a stream is addressed by appending `:name` to a path and `std::fs` reaches it. What that costs on Unix depends on what else is turned on, and the first draft of this paragraph gave only the flattering half: measured on 2026-08-26, one crate on top of `fs`, which already brings `xattr`'s tree of `rustix`, `bitflags` and `linux-raw-sys` in through `tempfile` — and four on its own, `provenance` not implying `fs` deliberately. Coupling the two to make the smaller number true would be answering a documentation problem with a dependency, so the number is stated both ways instead. §3's rule against compiling C is untouched either way: every one of the four is declarations rather than C to build.

**Amended 2026-08-27: `fs` implies `provenance` now, and the paragraph above had the reasoning half-right.** It was true that a caller who only writes containers has no use for carrying a mark onto a payload. It was false that such a caller has no use for the feature, because `Destination::in_place` replaces a file by renaming a fresh one over it and a fresh file carries no mark — so every in-place rewrite stripped whatever the platform had recorded about the original, from the day `in_place` existed until this was measured. A caller who enabled `fs` in order to replace containers was getting that silently, which is the laundering §4.11 exists to prevent arriving through the door §4.11 was not watching. The implication costs one crate on Unix, `xattr` itself, whose tree `tempfile` already brings in here, and nothing on Windows. The other direction is unchanged and still deliberate: `provenance` does not imply `fs`, because carrying a mark between two paths needs no temporary file.

**What the CLI does with a failure is the CLI's, and it removes the payload.** The file is on disk, the platform would have stopped somebody opening the container it came out of, and leaving it is leaving precisely the artifact this exists to prevent. `Destination` takes the same line about a container it could not finish writing. A container read from standard input is skipped rather than failed: there is no source to read a mark from, and what dropped it down the pipe is not something this can see.

**What it does not reach.** A payload extracted somewhere with no room for the mark is the failure above and is reported. A filesystem that will not hold an extended attribute at all — `tmpfs` mounted `nouser_xattr`, a FAT volume — makes the tests announce a skip rather than pass quietly, because a run that proved nothing should not read like one that did.


### 4.12 What a reader spends before it knows what it holds

SPEC §6 requires a bound on the metadata member and names no number. This is where the number is chosen and where the reason a bound belongs to the library at all is written down.

**Why it is not the ZIP library's problem, which is what this said until it was measured.** A general ZIP consumer chooses what to inflate, so a limit is between it and its own caller. A reader of this format does not choose: inflating the metadata member and parsing it as TOML *is* how it discovers whether the file was a container, so the memory is spent before anything about the file is known. Measured on 2026-08-27, before the fix: a 204,151-byte container whose metadata member deflates at a little over a thousand to one cost 620 MB resident here, 1,020 MB in `slipcase-desktop`, took 0.61 seconds, and came back conformant. At 1,019,488 bytes it was 5,019 MB and 5.1 seconds, and the desktop parses on the thread drawing the window.

**16 MiB, and why not less.** The two keys the format defines occupy a few dozen bytes, so any bound at all is generous by that measure — and SPEC §2.2 permits any keys at any depth, which is the whole forward-compatibility story, so a document carrying an extracted text layer or an embedded thumbnail can reach a megabyte or two without doing anything wrong. A bound near that would refuse containers nobody would call hostile. At the other end, 16 MiB of TOML costs a parsed document several times that, which is survivable anywhere this crate is likely to run. It is a default rather than a recommendation, which is why `Limits` exists: a reader invoked automatically over a directory should set it far lower, and one handed a single file by somebody who chose it can afford more.

**Undetermined and not non-conformant**, which SPEC §6 requires and is worth restating as a design constraint rather than a rule. The bound is this reader's, so exceeding it is a fact about the reader. `Verdict::NonConformant` would publish a configuration value as a property of somebody else's file, and two callers holding different `Limits` would disagree about conformance — the disagreement SPEC §3 exists to prevent. `Unsupported` already maps to `Undetermined`, so the new variant needed no plumbing.

**The recorded size is not the bound.** The obvious implementation reads the uncompressed size out of the central directory and refuses on that alone. Measured against `zip` 8.6: a directory rewritten to declare 100 bytes for a member inflating to 209,715,259 is not checked by anything, and the member still arrived in full at 621 MB. So it is read first, because it refuses the ordinary hostile case without spending a byte, and the guarantee is `take(limit + 1)` on the read. One past the limit rather than at it, so that reaching the bound is a refusal and never a silent truncation — a document cut off at the limit would parse to something the container does not say.

**The depth half is `toml_edit`'s and is already met**, which is worth recording so that nobody implements a second one. Measured across nested arrays, nested inline tables, dotted keys and table headers: it accepts nesting to depth 80 and refuses 81, and refuses with an error naming a recursion limit rather than by overflowing the stack. A stack overflow aborts the process and cannot be caught, so this was measured rather than assumed.

### 4.13 A name is not safe because it is legal

SPEC §2.3 excludes the C0 controls and U+007F from `payload.file` and lets everything else through, including the Unicode bidirectional formatting characters. That is deliberate on both sides: they are legal on every filesystem, a writer may hold a file that genuinely has one in its name, and putting them in the name rules would turn a rule about paths into a table of special cases. So the container is conformant and the problem is the display, which is why SPEC §3 states it as a display rule and why `display_name` is a rendering function rather than a check.

**Escaping rather than isolating.** Wrapping the name in U+2066 and U+2069 confines the reordering to one field, and a name that reads as `report.pdf` inside its own field is still a name that reads as `report.pdf` — the field exists to say what somebody is about to open. An override with no terminator also runs to the end of the paragraph rather than the end of the string, so anything relying on containment has to emit the terminator itself and cannot trust the name to carry one.

**Why `info` does not escape when it is redirected.** The verb has two jobs and they want opposite things. Into a pipe or a file it reproduces the metadata member byte for byte, which is what a caller redirecting it asked for and what escaping would ruin: they would get a document the container does not contain. Onto a terminal it is a display, and a terminal is the one place these characters are applied. Splitting on `IsTerminal` is where `ls` and `git` split for the same reason, and it costs no dependency.


## 5. The CLI

Five verbs. Each does one thing the format supports.

- `pack <payload> [--name <n>] [--meta <file.toml>] [-o <out.slpc>]` — writes a container. Default output is the payload's name with `.slpc` appended, per the naming convention. With no `--meta`, generates metadata carrying only the two required keys.
- `unpack <file.slpc> [--dest <dir>] [--metadata]` — writes the payload. `--metadata` also writes `slipcase.metadata.toml`. Nothing else in the archive is written to disk, as the specification requires.
- `repack <file.slpc> [--meta <file.toml>] [--payload <file>] [--name <n>] [-o <out.slpc>]` — changes the metadata, the payload, or both, and copies every other member through. At least one of the two, since a repack with nothing to change would read as a command that did something.
- `info <file.slpc>` — prints the metadata. Redirected, it reproduces the member byte for byte; onto a terminal it escapes the bidirectional formatting characters, for the reason in §4.13.
- `validate <file.slpc>` — reports conformance. A container declaring a version this build does not implement is not reported conformant, because everything past the two required keys is a rule this version's text states and none of it was checked. That is exit 3: a refusal to answer, not a verdict on the file.

Behavior that is decided rather than obvious:

- **Both required keys are set for the user**, by the library rather than by the CLI: `payload.file` from the payload's own filename, `slipcase_version` from the build. §4.3. A `--meta` file that sets `payload.file` to something else is an error rather than a silent overwrite.
- **A payload whose filename cannot be a member name is rejected, not renamed.** Packing it would produce a container that cannot name its own payload.
- **`repack` exists because unpacking and packing again is not the same operation.** SPEC §3 requires that members an implementation does not recognize survive a rewrite, and unpack-then-pack discards every one of them. Without this verb the workflow the tool teaches is the one the specification forbids.
- **`repack` writes back over the container it was given**, unless `-o` names somewhere else. A verb that named its target and then refused to touch it would send every caller through `repack -o tmp && mv tmp target`, the same operation with the atomicity taken out. `--force` is not the gate, because `--force` means "there is an unrelated file in your way": naming the container is the consent.
- **`repack` reads back what it wrote before replacing anything.** The library validates the metadata it is about to store, so this checks the archive around it, at the cost of a central-directory read of a file already in the page cache.
- **Writing in place resolves the path first**, so a container reached through a symbolic link is replaced rather than the link. That and the permissions carried across are §4.7's: the tool chooses an in-place destination over a new one, and the library knows what each means.
- **A file this tool creates comes out with the permissions any other new file would have**, which is also §4.7's doing. What stays in the tool is what is shaped like a command line: `-` for standard output, refusing to write a ZIP at a terminal, and the wording of the messages — the library reports that a destination exists and says nothing about `--force`, having no flags of its own.
- **Neither `pack` nor `unpack` overwrites an existing file without `--force`.** `unpack --metadata` reserves both destinations before writing either.
- **Exit codes:** 0 for success or conformance, 1 for bad input, 2 for a bad command line, 3 for no verdict. The first three split on whose mistake it is: 2 says re-read `--help`, 1 says go and look at the file. The fourth is against the fleet's three-code convention and is earned because the distinction is normative: a container whose metadata cannot be read, or which declares another version, is one SPEC §3 forbids calling non-conformant, and with one code for both a caller branching on the status reads it as exactly that.
- **`-` names standard input where a file is read, and standard output where one is written.** The reading half is the fleet convention. The writing half is this tool's own, and it is what lets a container move through a pipeline: `info | edit | repack --meta -` is the shape the verb is for. Writing a container to a terminal is refused. Only one argument may be `-`, there being one standard input.
- **Standard output is spooled to a temporary file and copied out at the end**, the mirror of what the reading verbs do with standard input. It gives repacking the seekable destination §4.4 wants, and a pipeline never receives the first half of a container that then failed. `pack -` streams without buffering but needs `--name`, there being no filename to take `payload.file` from. The reading verbs cannot stream at all — a ZIP's central directory is at its end — so `info -`, `validate -`, and `unpack -` spool and open the spool. That cost is the CLI's rather than the library's, which keeps its `Read + Seek` bound and never spools for a caller who already has a file.
- `--version`, `-V`, `--help`, `-h`, per the fleet convention.

---

## 6. Testing

Two layers.

**Fixtures the tests build themselves.** Every archive the suite reads is stamped byte by byte in `tests/support`, including the ones no ordinary writer will produce: a CP437 member name, a name flagged UTF-8 that is not one, two members sharing a name, a payload declaring a compression method this build lacks. Nothing binary is committed, so nothing in the history is opaque to review, what a fixture tests is in the code that builds it, and a fixture cannot go stale against a constant it shares with the crate.

None of that is self-containment and it should not be sold as such. `cargo test` fetches this workspace's dependencies before it runs anything.

**The conformance corpus from `excelano/slipcase`**, run against a version being released rather than against every commit. There is one corpus and every implementation answers to it, so it is consumed where it lives rather than copied or pinned here: a pinned copy would freeze the arbiter, and the corpus changes when the specification is clarified.

Running it needs that repository checked out and Python 3.11 or later to generate the cases, neither of which `cargo test` implies. That is why it is a command rather than a test — a test would have to choose between skipping quietly, which reports green having proved nothing, and failing on a machine that was never going to have those things. `corpus/` is that command. It refuses to report success on a corpus it could not find, on one whose cases have not been generated, or on one holding containers the manifest does not describe.

It checks two things per case. The verdict the library reaches, and the exit code the tool returns; the second is not a restatement of the first, since three of the four codes exist because SPEC §3 forbids reporting a container this build cannot judge as one it has judged.

This is the layer that matters. Passing a corpus written against the prose, by someone reading the prose, is what makes the specification's claim to be implementable something other than an assertion — and that is a claim about a release.

---

### 6.1 Throwing bytes at it

`fuzz/` is a mutational harness over the conformance corpus. It exists because
every defect the 2026-08-27 security review found came from somebody *reasoning*
about the code, and reasoning has a blind spot that volume does not: a panic on
input nobody imagined, a loop with no bound, arithmetic on a length an attacker
chose.

**Seeded from the corpus, not from noise.** A container is a structure and
random bytes are almost never one; a run seeded on nothing spends itself being
refused at the first four lines. Two of the six mutations rewrite a
little-endian field *beside a ZIP signature*, which is where the three worst
findings of that review lived, and the rest are ordinary bit damage,
truncation and splicing.

**It counts how deep it got, and prints that.** A fuzzer that never passes the
front door finds nothing and looks exactly like one that found nothing. Of two
million cases from one seed: 1.19 M are refused as not an archive, 564 K are
refused for something else, 202 K come out *conformant*, 386 K parse the
metadata member, 129 K read a payload through, and 202 K survive a rewrite and
are read back. Those numbers are the argument that a clean run means anything.

**Stable, not `cargo-fuzz`.** libFuzzer wants a nightly toolchain, which is a
1.5 GB install and a second compiler to keep in step with the one everything
else is built by. This finds the classes above on the toolchain already here,
and if coverage-guided fuzzing is wanted later it is this file that it replaces.
What is given up is real: the mutations explore by luck rather than by feedback,
so depth comes from volume.

**It detects a crash and a hang, and not a wrong answer.** A container that
validates as conformant when it should not is invisible here; that is the
corpus's job, and the two are complements. It also writes the case it is about
to try to `current.bin` first, so a run that hangs and is killed from outside
leaves the container that hung it.

    cargo run --release -p fuzz -- /path/to/slipcase/conformance
    cargo run --release -p fuzz -- CONF --cases 2000000 --seed 7

Sixteen million cases across eight seeds on 2026-08-27, then sixteen million
more once the rewrite path was added: no panic, no case over two seconds.

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

**Desktop integration** — an opener, a file association, an icon, a shell extension. Separate work, separate platforms, and a separate repository: `excelano/slipcase-desktop` is a graphical application over this library. It is not in this workspace because the fleet's shared CI and this repository's release pipeline are both scoped to a workspace holding one dependency-free command-line binary, and because a window is two hundred crates the library's contributors should not have to build. What it needs from the library goes into the library — §4.7 and §4.8 both arrived that way.

**Key-level metadata editing from the CLI.** `repack --meta` replaces the document wholesale, which needs no syntax of its own. A `set key=value` verb would need a convention for whether `3` is an integer or a string, and inventing one here would be defining a vocabulary the format deliberately does not have. SPEC §5 leaves a vocabulary out of this version rather than out of every version, so the name stays free for one that has something to operate on.

**Bindings for other languages.** Every other language gets a native implementation reading the same specification.

**Schema validation of metadata beyond the two structural keys.** There is nothing to validate against.
