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
