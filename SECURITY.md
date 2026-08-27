# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub Security Advisories at https://github.com/excelano/slpc-rust/security/advisories/new. If you would rather not use GitHub, email david.anderson@excelano.com instead. I aim to respond within seven days.

Please do not open public issues for security problems.

## Supported versions

The latest 0.x release receives security fixes. Older versions are not supported.

## What slipcase can access

slipcase is a CLI that runs locally on your machine, and `slpc` is the library behind it. Both read the file you point them at and write only where you tell them to. Neither makes a network call of any kind, has an auth layer, or implements any administrative operation. They can read and write only what your operating-system user already can.

A few things are worth stating because they are the ones a container format could plausibly get wrong.

**Unpacking writes the payload and nothing else.** `payload.file` is a plain filename and never a path, which the specification states as a list of exclusions in its section 2.3 and this implementation checks in full before the name is used. A name breaking any of them is rejected rather than repaired, so a payload cannot be written outside the destination directory. The payload member must also be a regular file entry, so a symbolic link, a directory, or a device cannot be written in a payload's place. Members of the archive other than the payload and the metadata document are never written to disk, whatever they are named. `--metadata` adds one more file, `slipcase.metadata.toml`, in the same destination.

**Staying inside the directory is not the same as being a file, and this paragraph used to conflate them.** It argued that because the checked name contains no separator, joining it to a directory cannot leave that directory — which is true, and is not the whole question. Windows resolves a handful of names to devices wherever they appear, so `CON` does not leave the destination directory; it is simply not in it, and before 0.3.5 writing there discarded the payload and reading it back could hang. The traversal assurance above stands on its own and always did. What has been added is that the destination is addressed in a form those names are not resolved in, so a conformant container naming its payload `CON` produces an ordinary file.

**Unpacking carries where the container came from onto the payload**, from 0.3.5. If the container is marked as downloaded — `com.apple.quarantine`, a `Zone.Identifier` stream, `user.xdg.origin.url` — the payload is marked the same way, so that whatever opens it next sees a file that arrived from elsewhere rather than one this machine made. Where the platform gates opening on that mark and it cannot be written, `unpack` removes the payload and says so rather than leaving one that opens without the warning its origin earned. This is the one case in which unpacking deletes something it wrote. A container read from standard input has no source to read a mark from and is unpacked without one.

**A payload is never executed, opened, or handed to another program.** The tool has no verb that runs anything. Nothing here inspects a payload's type or acts on it, and a container that names its payload `report.pdf` gets no different treatment from one that names it `setup.exe`.

**Identifying a container is bounded, from 0.3.6, and this paragraph used to say it needed no bound.** It held that resource limits belong to `zip` and `toml_edit` and apply equally to every other consumer of those crates. That is true of the payload, which nothing inflates until you ask. It was never true of the metadata member: deciding whether a file is a container means decompressing that member and parsing it as TOML, so the memory is spent before anything about the file is known, which is not the position of a general ZIP consumer who chooses what to extract. Measured before the fix, a 204,151-byte container cost 620 MB of memory and was reported conformant. The metadata member is now bounded — 16 MiB by default, adjustable through `Limits` — and a container over the bound is reported as undetermined rather than as non-conformant, because the bound belongs to the reader and not to the file.

**A payload name is escaped before it is shown to you.** The specification permits the Unicode bidirectional formatting characters in `payload.file`, because they are legal filenames everywhere and excluding them would make the name rules a table of special cases. A payload called `report<U+202E>fdp.exe` therefore reads as `report.pdf` in any terminal that applies the override. `slipcase validate` escapes it, and `slipcase info` escapes it when it is writing to a terminal — when it is redirected into a file or a pipe it still reproduces the metadata member byte for byte, since that is what a caller redirecting it asked for.

Beyond those, a container is an ordinary ZIP archive, and the usual cautions about archives from untrusted sources apply to it exactly as they do to any other.

## What slipcase stores

Nothing beyond the files you ask it to write. There is no configuration directory, no history file, no cache, no telemetry, no analytics, and no remote logging. Reading from standard input spools to a temporary file that is unlinked as it is created, so it leaves nothing behind however the process ends.

## Verifying releases

Every GitHub release includes a `.sha256` file next to each archive listing its SHA-256 hash. Verify any download before running it:

    sha256sum slipcase-x86_64-unknown-linux-gnu.tar.xz
    # compare against the value in slipcase-x86_64-unknown-linux-gnu.tar.xz.sha256

Release artifacts are built by GitHub Actions from a tagged commit using the cargo-dist configuration in this repo (`dist-workspace.toml` and the generated `.github/workflows/release.yml`). The workflow and build configuration are public and auditable.
