# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub Security Advisories at https://github.com/excelano/slpc-rust/security/advisories/new. If you would rather not use GitHub, email david.anderson@excelano.com instead. I aim to respond within seven days.

Please do not open public issues for security problems.

## Supported versions

The latest 0.x release receives security fixes. Older versions are not supported.

## What slipcase can access

slipcase is a CLI that runs locally on your machine, and `slpc` is the library behind it. Both read the file you point them at and write only where you tell them to. Neither makes a network call of any kind, has an auth layer, or implements any administrative operation. They can read and write only what your operating-system user already can.

Two things are worth stating because they are the ones a container format could plausibly get wrong.

**Unpacking writes the payload and nothing else.** `payload.file` is a plain filename and never a path, which the specification states as a list of exclusions in its section 2.3 and this implementation checks in full before the name is used. A name breaking any of them is rejected rather than repaired, so joining it to a destination directory cannot leave that directory. The payload member must also be a regular file entry, so a symbolic link, a directory, or a device cannot be written in a payload's place. Members of the archive other than the payload and the metadata document are never written to disk, whatever they are named. `--metadata` adds one more file, `slipcase.metadata.toml`, in the same destination.

**A payload is never executed, opened, or handed to another program.** The tool has no verb that runs anything. Nothing here inspects a payload's type or acts on it, and a container that names its payload `report.pdf` gets no different treatment from one that names it `setup.exe`.

Resource limits when parsing ZIP or TOML belong to the libraries doing the parsing, `zip` and `toml_edit`, and apply equally to every other consumer of those crates. A container is an ordinary ZIP archive, and the usual cautions about archives from untrusted sources apply to it exactly as they do to any other.

## What slipcase stores

Nothing beyond the files you ask it to write. There is no configuration directory, no history file, no cache, no telemetry, no analytics, and no remote logging. Reading from standard input spools to a temporary file that is unlinked as it is created, so it leaves nothing behind however the process ends.

## Verifying releases

Every GitHub release includes a `.sha256` file next to each archive listing its SHA-256 hash. Verify any download before running it:

    sha256sum slipcase-x86_64-unknown-linux-gnu.tar.xz
    # compare against the value in slipcase-x86_64-unknown-linux-gnu.tar.xz.sha256

Release artifacts are built by GitHub Actions from a tagged commit using the cargo-dist configuration in this repo (`dist-workspace.toml` and the generated `.github/workflows/release.yml`). The workflow and build configuration are public and auditable.
