# slpc-rust

The Rust implementation of [slipcase](https://github.com/excelano/slipcase), a container format that attaches metadata to a file.

<!-- shared:blurb -->
A `.slpc` file is a ZIP archive holding a payload file of any type together with a TOML metadata document describing it. The two become one file, so copying, moving, or sending the payload carries its metadata along.
<!-- /shared:blurb -->

This repository is a Cargo workspace holding two crates:

- **`slpc`** — the library. Reads, writes, and validates containers.
- **`slipcase`** — the command-line tool, built on the library.

<!-- shared:authority -->
The specification lives in `excelano/slipcase` and is the authority on the format.
<!-- /shared:authority -->
This is a reference implementation: it exists to show that the specification is implementable and to be checked against it.

<!-- shared:install -->
## Install

### Debian and Ubuntu

Add the [Excelano apt repository](https://excelano.com/apt/) once:

```sh
curl -fsSL https://excelano.com/apt/setup.sh | sudo sh
```

Then install it, so `apt upgrade` keeps it current:

```sh
sudo apt install slipcase
```

Both amd64 and arm64 packages ship with every release.

### Homebrew

```sh
brew install excelano/tap/slipcase
```

### crates.io

```sh
cargo install slipcase
```

### Anywhere else

```sh
curl -fsSL https://github.com/excelano/slpc-rust/releases/latest/download/slipcase-installer.sh | sh
```

PowerShell, for Windows:

```powershell
irm https://github.com/excelano/slpc-rust/releases/latest/download/slipcase-installer.ps1 | iex
```

Every release also carries plain archives for macOS, Linux, and Windows on both
Intel and ARM, each with a `.sha256` beside it.
<!-- /shared:install -->

## The tool

<!-- shared:verbs -->
```
slipcase pack report.pdf --meta owner.toml       # writes report.pdf.slpc
slipcase info report.pdf.slpc                    # prints the metadata, verbatim
slipcase repack report.pdf.slpc --meta new.toml  # changes it in place, keeping the rest
slipcase validate report.pdf.slpc                # exit 0 if conformant
slipcase unpack report.pdf.slpc --dest ./out     # writes the payload and nothing else
```
<!-- /shared:verbs -->

Wherever a file is read, `-` names standard input, and wherever one is written it
names standard output.

<!-- shared:exit-codes -->
Exit codes tell success from bad input, from a bad command line, from a container this build cannot judge. `slipcase --help` states the contract.
<!-- /shared:exit-codes -->

## The library

[docs.rs/slpc](https://docs.rs/slpc) is the library's own page, and its examples are compiled and run rather than transcribed.

## This repository

`DESIGN.md` records the design and the reasoning behind each decision.

## License

MIT. See [LICENSE](LICENSE).
