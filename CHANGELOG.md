# Changelog

Notable changes to `slpc` and `slipcase`, which version in lockstep and are
released together. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
follows [semantic versioning](https://semver.org/spec/v2.0.0.html) — where a
minor bump below 1.0 is how a breaking change ships.

This file begins at 0.3.0. Earlier releases carried no notes, and the tags and
the commit history are the record for those.

## [Unreleased]

### Added

- **`Container::payload_crc`**, the CRC-32 the ZIP central directory already
  records for the payload member. Read in the same pass as the name, the size
  and the kind, so it decompresses nothing and needs no `&mut`, and it refuses
  the way `payload_size` does for a container declaring a version this build
  does not implement.

  It answers one question: whether a file on disk is the one that came out of
  this container. A caller that extracted a payload earlier and wants to know
  whether it has been edited since had no way to ask without keeping a record of
  what it wrote — a second copy of a fact, which can drift from the fact, and
  which is least trustworthy at the moment such a record is usually consulted:
  after something went wrong. The container's own value needs nothing
  maintaining it, because repacking recomputes it.

  **Documented as the ZIP field it is.** SPEC 5 defines no checksum or fixity
  key, and this is not one arriving by another road. A format library exposing a
  checksum invites the reading the specification declined to license, so the doc
  comment says what it does not answer as plainly as what it does.

## [0.3.10] - 2026-08-28

### Added

- **`Mark::Recorded`, and an origin note that survives what the App Sandbox
  destroys.** Under the sandbox the platform marks whatever the calling process
  writes and refuses to have that mark replaced, so `carry` reported
  `AlreadyMarked` and the source's value was lost. The gate was never the
  problem — the copy is gated whoever marked it — but the answer to *where did
  it come from* was, and a caller that reports provenance could no longer tell a
  container that arrived from elsewhere from one made on the machine. Found in
  `slipcase-desktop`, where saving an edit to a downloaded container made the
  card's *arrived from elsewhere* line disappear.

  `carry` now keeps the source's quarantine value verbatim under
  `com.excelano.slipcase.origin` when the platform refuses to carry it, and
  answers `Mark::Recorded` rather than `Mark::AlreadyMarked` when it did.
  `arrived_from_elsewhere` consults the note. `carries_a_mark` deliberately does
  **not**: the note gates nothing, nothing outside this crate reads it, and a
  caller deciding whether to hand a payload to the system must not read our own
  writing as the platform's word.

  Three things measured inside a bundle signed with the sandbox entitlement
  rather than assumed: the refusal is specific to `com.apple.quarantine` and an
  attribute of our own goes on without complaint; the note survives
  `-[NSFileManager replaceItemAtURL:]`, which is the operation that destroys the
  attribution; and no supported API preserves the original attribution, since
  `NSURL`'s `quarantinePropertiesKey` succeeds and then substitutes the calling
  process as the agent.

  macOS only. Windows can reach the same branch and does nothing there yet, for
  want of a measurement; Linux cannot reach it, because its `carry` never fails.
  Nothing changes for a caller that is not sandboxed.

  `Mark` is `#[non_exhaustive]`, so the new variant is not a breaking change for
  anyone matching on it.

## [0.3.9] - 2026-08-27

An adversarial review of the same day's hardening, which found that the
hardening was incomplete and that one of its fixes did not fire.

### Security

- **A member could still be renamed out from under the uniqueness check.** 0.3.8
  refused three ways a crafted end of central directory record could make this
  crate and its ZIP dependency resolve different directories. There was a
  fourth, and it is inside a single well-formed entry where no record-level
  guard can see it: the Info-ZIP Unicode Path extra field, tag 0x7075, carries a
  replacement name and the ZIP crate applies it, while `central.rs` skipped
  extra fields and counted the recorded name. Three members with distinct
  recorded names — the third renaming itself to the second's — came back
  conformant and unpacked to the third's bytes, while Python's `zipfile` saw two
  members of one name. The metadata member is swappable the same way.

  Refused rather than honoured, which is the decision §2.1 has now taken four
  times. Honouring it would make the field a third way to spell a member's name,
  after the name field and the bit 11 encoding SPEC §2.1 defines, and that is a
  change to the format rather than a fix to a reader. Measured against the tools
  available: Info-ZIP writes 0x5455 and 0x7875 even with `-UN=UTF8`, 7-Zip
  writes 0x000a, Python writes none. Those fields are untouched.

- **The provenance carry added in 0.3.7 did not fire on a read-only container.**
  `commit` set the replacement's permissions — taken from the file being
  replaced — before carrying the mark onto it, so a container at 0444 made the
  temporary read-only and `xattr::set` was then denied. Measured: a marked
  container at 0444 came back from `repack` with no mark, silently, exit zero.
  Where a platform enforces the mark it is worse: the carry fails, `commit`
  propagates it, and an in-place rewrite of any read-only marked container fails
  outright. The carry now runs first. The test that should have caught this used
  a default-mode file.

- **A failed `unpack --metadata` no longer leaves the metadata behind.** 0.3.8
  reordered the commits so a refusal leaves no payload; the inverse was left
  standing, so a payload that could not land left a metadata file the error
  never mentioned and the obvious retry then failed on it.

### Fixed

- `display_name` escapes a backslash, so its rendering is reversible. A name
  that literally spells `report\u{202E}fdp.exe` and one carrying the override
  used to come out identical.

## [0.3.8] - 2026-08-27

Findings from a security review of all three repositories, run the same day.
Everything here was measured against a fixture, and every fix was checked by
breaking it and watching the test that guards it fail.

### Security

- **A crafted end of central directory record could hide a duplicate member from
  the check SPEC §3 requires.** This crate resolves the central directory twice:
  once here, to count names the ZIP crate cannot see, and once in that crate,
  for every byte anything reads. Three fields in that record could be made to
  split the two, so uniqueness was established over one set of members while the
  payload came from another — the ambiguity SPEC §3's enumeration rule exists to
  prevent, arriving through the rule's own implementation.

  The counts. The record carries entries-on-this-disk and entries-in-total; this
  crate read the total and the ZIP crate reads the count on this disk. A record
  declaring 3 and 2 was counted here as two members and served by the crate as
  three, so a second `report.pdf` passed a conformant verdict and was the one
  extracted.

  The comment length. Overrun it and a reader taking the last signature it finds
  parts company with one that checks the length and keeps looking. Measured:
  this crate served the first archive in a file and Python's `zipfile` the
  second, with a conformant verdict on it.

  The Zip64 sentinel. The ZIP crate goes looking for a Zip64 record when the
  directory *size* field is saturated; this crate did not, so setting only that
  field pointed the two at different records.

  Chasing the crate's behaviour field by field is not the fix, because the next
  field is the next bug. `central.rs` now refuses any record that is not
  internally consistent: it must end the file, its two counts must agree, the
  archive must be single-disk, and the Zip64 gate matches the crate's. SPEC §2.1
  states the same rule, with three corpus cases in a class that had none.

- **`slipcase unpack --metadata` could leave the payload on disk, unmarked,
  while reporting failure.** Both destinations are reserved before either is
  written, and the code said that made this impossible. Reserving is a check and
  committing is the guarantee: `Destination::new` asks whether the path exists
  and a dangling symbolic link answers no, so the refusal arrived at the
  no-clobber rename — after the payload had been committed and before provenance
  was carried. One planted link in a directory somebody unpacks into was enough.
  The metadata commits first now, so a failure there leaves nothing at all.

- **`slipcase repack -o -` wrote provenance onto a file named `-`.** The carry
  added in 0.3.7 guarded `-` on the input and not on the output, so with the
  container going to standard output the mark went to a real file of that name
  in the working directory — and on Windows, where the mark is an alternate data
  stream reached through `std::fs`, it would have created one.

### Changed

- `Destination::in_place`'s documentation now names hard links alongside
  ownership as something a rename cannot carry: a container reachable under two
  names is rewritten under only the one it was opened by.

## [0.3.7] - 2026-08-27

### Added

- **`Limits::DEFAULT`**, the defaults as a constant. `Default::default` is not
  `const` and the struct is `#[non_exhaustive]`, so without it a caller outside
  this crate cannot say *the defaults, with this one changed* in a `const` —
  which is where a caller with a considered bound wants to put it.

### Security

- **Rewriting a container in place no longer launders it.** `Destination::in_place`
  replaces a file the way an editor does: it writes a fresh file beside the
  original and renames it over the top. A fresh file carries no mark, so
  whatever the platform had recorded about where the original came from —
  `com.apple.quarantine`, a `Zone.Identifier` stream, `user.xdg.origin.url` —
  was gone the moment anything rewrote the container. `slipcase repack --meta`
  on a container marked as downloaded returned it unmarked, and every payload
  unpacked from it afterwards was unmarked too, because `provenance::carry`
  copies from the container. True since `in_place` existed.

  This is the defect 0.3.5 fixed on the unpacking side arriving through a door
  nobody had looked at. `commit` now carries the mark onto the replacement
  before the rename, so the file that appears at the path is complete at the
  instant it appears. `Destination::new` is unchanged and inherits nothing: a
  caller naming an output file is creating one, there is no original whose
  origin it takes, and inventing one would be claiming a download that never
  happened. That is the line `new` already took about permissions.

  `slipcase repack -o <new>` carries it too, in the CLI rather than the library,
  because only the caller knows which container the bytes came out of. It warns
  rather than failing where it cannot: what the failing rule in `unpack` guards
  is a payload about to be handed to the operating system, and a container is
  opened by nothing but this tool, which reports provenance rather than acting
  on it.

### Changed

- **The default metadata bound is 1 MiB, down from the 16 MiB 0.3.6 shipped a
  few hours earlier.** That number rested on an estimate — that a parsed
  document costs *several times* its source — which was wrong by more than an
  order of magnitude. Measured against the densest conformant shape, shortest
  legal keys and shortest legal values: 256 KiB of metadata parses to 22 MB
  resident and 1 MiB to 85 MB, about 85 times, because `toml_edit` keeps a key's
  decor, span and representation so that a rewrite can put back what it did not
  touch. 16 MiB was therefore about 1.4 GB parsed, and several times that again
  in anything that renders the document.

  The multiplier follows key count rather than size, so the number is chosen
  against the dense shape. 1 MiB costs 85 MB at worst and is generous against
  every legitimate document: the format defines two keys, SPEC §2.2's example is
  four lines, and the largest metadata member in the conformance corpus is
  64 KiB.

- **`fs` implies `provenance`.** Not convenience: without the carry above,
  enabling `fs` in order to replace containers is enabling a laundering bug, and
  a security property should not depend on a caller having guessed that a second
  feature was involved. It costs one crate on Unix — `xattr` itself, whose tree
  of `rustix`, `bitflags` and `linux-raw-sys` `tempfile` already brings in — and
  nothing on Windows, where an alternate data stream is reached through
  `std::fs`. The other direction is unchanged: `provenance` does not imply `fs`.

## [0.3.6] - 2026-08-27

Everything here follows `excelano/slipcase`'s reader-side hardening of the same
day, which added four requirements to SPEC §3 and a Security Considerations
section as SPEC §6. Two of the four this library already satisfied and now
proves it does; the other two are new work.

### Security

- **A hostile metadata member no longer costs whatever it likes.** Identifying a
  container means decompressing the metadata member and parsing it as TOML, so a
  reader spends the memory before it knows whether the file was a container at
  all — which is not the position of a general ZIP consumer, who chooses what to
  extract. Measured before the fix: a 204,151-byte container whose metadata
  member deflates at a little over a thousand to one cost 620 MB resident here
  and 1,020 MB in the desktop viewer, took 0.61 seconds, and was reported
  conformant. At 1,019,488 bytes it was 5,019 MB and 5.1 seconds.

  `Limits` bounds it, defaulting to 16 MiB, and the four entry points that read
  a metadata member gained a `_with` twin that takes one: `Container::read_with`,
  `Container::open_with`, `validate_with` and `metadata_of_with`. The same two
  containers now cost 11 MB and 0.00 seconds. A container over the bound is
  `Verdict::Undetermined`, never `NonConformant`, because the bound belongs to
  the reader: answering non-conformant would publish this build's configuration
  as a property of somebody else's file, and two readers with different bounds
  would then disagree about conformance.

  The size a central directory records is not the bound and cannot be. Measured
  against `zip` 8.6: a directory rewritten to declare 100 bytes for that same
  member still inflated the whole of it and still cost 621 MB, because nothing
  in ZIP checks the two against each other. It is read first because it refuses
  the ordinary case without spending anything, and the guarantee is the bound on
  the bytes as they arrive.

  The parse-depth half of SPEC §6 is `toml_edit`'s and is already met. Measured
  across nested arrays, nested inline tables, dotted keys and table headers: it
  accepts nesting to depth 80 and refuses 81, with an error naming a recursion
  limit rather than a stack overflow.

- **`display_name` escapes the Unicode bidirectional formatting characters.**
  SPEC §2.3 permits them in `payload.file` and DESIGN says why — they are legal
  on every filesystem and a writer may hold a file that genuinely has one — so
  SPEC §3 puts the rule on the display, where the problem is. A payload called
  `report<U+202E>fdp.exe` reads as `report.pdf` wherever the override is applied,
  beside whatever offers to open it.

  `slipcase validate` now names the payload through it. `slipcase info` splits
  the way `ls` and `git` split: redirected into a file or a pipe it still
  reproduces the member byte for byte, which is what a caller redirecting it
  asked for, and onto a terminal it escapes — a terminal being the one place
  these characters are applied, and an unterminated override running to the end
  of the paragraph rather than the end of the value it sat in.

### Added

- **`Container::payload_mode`** — the permission bits the archive records for
  the payload, where it records any, with the file-type bits masked off. For
  saying and not for applying: SPEC §3 forbids putting an archive's recorded
  mode on an extracted file, and this exists so that something can tell a person
  a payload was executable where it came from and that their copy will not be.

  `Ok(None)` where the container says nothing, which is the common case and the
  reason this reads the external attributes off the central directory rather
  than asking the ZIP crate. The crate's `unix_mode` invents a mode for an
  archive made on DOS — `S_IFREG | 0o664`, or `0o444` where the read-only bit is
  set — which for its purposes beats nothing and here would mean every container
  written by a Windows tool got a confident answer to a question it never
  answered.

### Changed

- `RawName` became `Recorded` internally, carrying the external attributes
  alongside the name it was already reading from the same header. Not public.

### Fixed

- **`cargo test --all-features` inside the published crate would not compile**,
  in 0.3.5 and in this release until it was caught. `testsupport` is a path
  dev-dependency and `cargo package` strips a path dependency it cannot resolve,
  so the test files using it referenced a crate the packaged manifest no longer
  declared. That is what anybody vendoring or auditing the crate runs.
  `slpc/tests/provenance.rs` and `slipcase/tests/cli.rs` are now excluded from
  their packages rather than shipped broken: the helper they need marks a file
  the way a platform's downloader does, and it is shared precisely because two
  copies of it disagreed about the Windows arm within an hour, so inlining a
  third would undo that on purpose. Those tests need the workspace and stay in
  it.

### Notes

Two SPEC §3 requirements were already met and are now pinned by tests rather
than by luck. Extraction refuses to replace an existing file through an atomic
no-clobber rename rather than a check before the write, and the new test stands
the race still: the file arrives after `Destination::new` returned and `commit`
still refuses. Nothing applies an archive's permission bits, structurally —
`Container::payload` hands back a reader and `Destination` takes a path, and the
two never meet — and the new test would catch somebody wiring them together,
which is a two-line change and more tempting now that `payload_mode` exists.

## [0.3.5] - 2026-08-26

### Security

- **`slipcase unpack` no longer launders where a container came from.** A
  container downloaded from the internet is marked as such by the platform that
  downloaded it — `com.apple.quarantine` on macOS, a `Zone.Identifier` stream on
  Windows, `user.xdg.origin.url` on Linux — and all three are properties of the
  file rather than of its contents. The payload written out of it carried none
  of them, so whatever opened that payload next saw a file this machine made and
  the warning the container would have raised never appeared. That has been true
  since 0.1.0.

  `provenance::carry`, behind the new `provenance` feature, moves the mark
  across, and `unpack` calls it. It fails only where the platform gates opening
  on a mark, the source carries one, and the copy ends up carrying none — so for
  a caller about to hand a payload to the system, an error means do not open it.
  Where `unpack` meets that failure it removes the payload rather than leaving
  one that opens without the warning its origin earned. A container read from
  standard input has no source to read a mark from and is unpacked without one.

  Linux is a note rather than a gate: nothing there consults the attribute
  before opening a file, and `Mark::Noted` is a separate answer so that nothing
  reads one as the other.

### Fixed

- **A payload named for a Windows device now extracts as a file.** SPEC 2.3
  accepts `CON`, `COM1`, `AUX`, `LPT1`, `PRN` and `NUL`, and the conformance
  corpus carries a case for one, but Win32 resolves those names to devices
  wherever they appear. `dest.join(payload_name())` was therefore not a path in
  that directory — it was the console. Writing `CON` returned success at every
  step and left no file, and reading it back never returned at all, so
  `slipcase unpack` on such a container hung rather than failing.

  `payload_path` asks `canonicalize` of the directory and joins the name onto
  the answer, which reaches the filesystem without those names being looked for.
  Nothing holds a list of reserved names: which names are devices stays
  Windows's to know. Asking the shell to *open* such a file still fails, which
  is the truth about that container on that platform.

### Added

- **`payload_path` and `display_path`**, with the `fs` feature. The first is
  above; the second takes the `\\?\` prefix off a path before a person reads it,
  since that prefix is how a path is addressed rather than part of its name.
  Note that on Windows `payload_path` reports where the file is rather than how
  the caller spelled it: `canonicalize` expands 8.3 short names and resolves
  junctions, so a caller comparing its result against a path of their own must
  compare files and not strings.

- **The `provenance` feature**, off by default and separate from `fs`. It adds
  no crates on Windows, where an alternate data stream is reached through
  `std::fs`, and on Unix one crate on top of `fs` or four on its own — `fs`
  already brings `xattr`'s tree of `rustix`, `bitflags` and `linux-raw-sys` in
  through `tempfile`. `provenance::arrived_from_elsewhere` is there for a
  caller that wants to report where a container came from rather than act on it.

### Changed

- **`Mark` is `#[non_exhaustive]`**, as every other public enum in the crate is.
  What a platform records about a downloaded file is that platform's to change.

## [0.3.4] - 2026-08-21

### Fixed

- **Repointing `payload.file` no longer discards the comment and the whitespace
  around it, and no longer runs when the value has not changed.** `Repack`
  documents that a document handed to it is serialized without losing comments,
  key order, or whitespace, and that held for every key except the one the
  library edits itself. Setting the key dropped a fresh item over the old one,
  which takes the decor `toml_edit` keeps beside a value with it, so replacing a
  payload under a new name silently deleted whatever a person had written after
  `file = "..."`, and a container whose metadata is a multi-line inline table
  came back with the spacing before its trailing comma changed.

  Two callers reached it. A rename lost the comment because the value genuinely
  changed. Handing over the document and replacing the payload under the name it
  already had lost the comment for nothing, because the key was assigned the
  string it already held rather than left alone.

  The key is now set inside the existing value with its decor restored, and a
  key already holding the string being written is not touched at all. Packing is
  unaffected: it only ever sets a key that is absent, where there is no decor to
  carry over. Wrong since 0.3.0, where `Repack` shipped.

## [0.3.3] - 2026-08-21

### Added

- **`Container::check_payload_readable`**, which says whether this build can
  decode the payload before anything is extracted. `Container` could already
  state the payload's name and size without touching its bytes, and nothing
  said whether those bytes were reachable: a caller found out by attempting the
  extraction. That is the wrong shape for anything that commits to an operation
  before performing it, such as a window putting an Open button on a payload
  card.

  It refuses with the same three `Unsupported` variants `Container::payload`
  does, in the order that function meets them, so the two never name different
  reasons for one refusal. Both facts come from the central directory entry
  read when the container was opened, so it borrows shared, decompresses
  nothing, and reads nothing further.

  `Ok` says the decoder exists rather than that extraction will succeed:
  truncated data, a failed checksum, and an i/o error are still ahead. And it
  is a capability query rather than a verdict — SPEC 2.5 puts compression and
  encryption outside the conformance question, so a container whose payload is
  encrypted is conformant and its payload is out of reach, and `validate` goes
  on saying the first of those.

## [0.3.2] - 2026-08-20

### Changed

- **`Container::payload_size` borrows shared rather than mutably.** In 0.3.1 it
  reached into the archive at call time and so took `&mut self`, which meant the
  question anything reporting a container's contents actually asks did not
  compile:

  ```rust
  println!("{} is {} bytes", c.payload_name(), c.payload_size()?);
  ```

  The size now comes from the central directory entry read when the container
  was opened, eight bytes per member against a lookup that needed the archive.
  Tightening a `&mut self` receiver to `&self` breaks no caller, so 0.3.1 code
  compiles unchanged.

## [0.3.1] - 2026-08-20

### Added

- **`slpc::Destination`, behind the new `fs` feature**, which writes a container
  to a path: through a temporary file beside it, with the permissions a file
  there should have, renamed into place at the end. The reading side has taken a
  path since 0.1.0 through `Container::open` and the writing side never had the
  equivalent, so every caller putting a container on disk supplied that half
  itself. Off by default, so a caller writing into a socket or a buffer does not
  acquire a temporary-file dependency and the library's default tree stays at
  fifteen crates.

  `slipcase` now writes through it rather than through its own copy. No change
  to what the tool does: same permissions, same atomicity, same messages.

- **`Container::payload_size`**, the payload's uncompressed length, read off the
  central directory without decompressing anything. Refuses the way
  `Container::payload` does for a container declaring a version this build does
  not implement, since the payload was never located.

- **`slpc::metadata_of`**, the metadata document of a byte stream with no
  conformance question attached. A container whose `payload.file` names no
  member — or names several, or names something SPEC 2.3 forbids, or which is
  missing a required key — has a metadata document that parsed cleanly, and
  `Container::read` returns an error over the payload before a caller can reach
  it. This returns the document. It is not a verdict: `validate` remains the
  only function that answers the question SPEC 3 constrains.

## [0.3.0] - 2026-08-20

### Added

- **`slipcase repack`**, and `slpc::Repack` behind it. Replaces a container's
  metadata, its payload, or both, and copies every other member through as
  stored bytes. This is the operation the specification requires when a
  container is rewritten, and it is the one unpacking and packing again cannot
  do: that route discards every member the tool does not recognize. Writes back
  over the container it was given unless `-o` names somewhere else, resolving
  symbolic links and keeping the container's permissions.

- **`-` names standard output** wherever a file is written, alongside its long
  standing meaning of standard input wherever one is read. `slipcase info c.slpc
  | your-editor | slipcase repack --meta - c.slpc -o out.slpc` is the shape this
  is for. Writing a container to a terminal is refused rather than done.

- **A conformance corpus runner**, `corpus/`, which is not published and not
  part of `cargo test`. It checks both the verdict the library reaches and the
  exit code the tool returns against every case in the corpus from
  `excelano/slipcase`, and it is a step in `RELEASING.md` rather than a test,
  because it needs that repository checked out and a Python interpreter to
  generate the cases.

### Changed

- **`rewrite_metadata` and `rewrite_metadata_bytes` now require `Write + Seek`
  of their writer, where they required `Write` alone.** This is the breaking
  change in this release, and a caller passing a writer that cannot seek will
  no longer compile.

  A member copied through a rewrite already knows its compressed size, and a
  writer that cannot seek has nowhere to record it but a data descriptor after
  the data — which is a promise to a reader walking forward that a length is
  coming. Repacking never had a pipe for a source, since a ZIP's central
  directory is at the end of the file, so requiring the same of the destination
  costs a caller nothing they were not already paying.

  Packing is unaffected and keeps its `Write`-only destination: a payload
  arriving from a pipe genuinely has no size to write down, and a container can
  still be packed from a pipe straight into a socket.

### Fixed

- **Rewritten containers no longer claim a data descriptor they do not have.**
  `zip` 8.6 sets general purpose bit 3 on a member copied raw into a stream
  writer and then writes no descriptor, so the local header recorded a length of
  zero and nothing supplied the real one. Readers that walk the central
  directory were unaffected, which is why this survived undetected;
  Info-ZIP walks forward and reported `invalid zip file with overlapped
  components (possible zip bomb)`, exiting 12. Present in `rewrite_metadata`
  since 0.1.0, where no shipped verb reached it.

- **Files the tool writes now carry the permissions a new file gets.** Writing
  through a temporary file and renaming it into place carried the temporary
  file's private mode onto the destination, so under an 0022 umask `slipcase
  pack` produced a container at 0600 where an ordinary file would be 0644, and
  `slipcase unpack` wrote a payload nobody but its author could read. Wrong
  since 0.1.0.
