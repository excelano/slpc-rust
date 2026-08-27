# Changelog

Notable changes to `slpc` and `slipcase`, which version in lockstep and are
released together. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
follows [semantic versioning](https://semver.org/spec/v2.0.0.html) — where a
minor bump below 1.0 is how a breaking change ships.

This file begins at 0.3.0. Earlier releases carried no notes, and the tags and
the commit history are the record for those.

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
  one crate on Unix and none on Windows, where an alternate data stream is
  reached through `std::fs`. `provenance::arrived_from_elsewhere` is there for a
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
