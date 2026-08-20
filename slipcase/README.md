# slipcase

The command-line tool for [slipcase](https://github.com/excelano/slipcase), a
container format that attaches metadata to a file.

A `.slpc` file is a ZIP archive holding a payload file of any type together with
a TOML metadata document describing it. The two become one file, so copying,
moving, or sending the payload carries its metadata along.

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

## Using it

```
slipcase pack report.pdf --meta owner.toml     # writes report.pdf.slpc
slipcase info report.pdf.slpc                  # prints the metadata, verbatim
slipcase validate report.pdf.slpc              # exit 0 if conformant
slipcase unpack report.pdf.slpc --dest ./out   # writes the payload and nothing else
```

Four verbs, each doing one thing the format supports. `pack` sets both required
metadata keys itself, so a `--meta` file that contradicts either is refused
rather than silently overwritten, and a payload whose filename cannot be a
member name is rejected rather than renamed. `unpack` writes the payload and,
with `--metadata`, the metadata document; nothing else in the archive reaches
disk. Neither verb overwrites an existing file without `--force`, and both write
through a rename, so a run that fails partway leaves nothing behind.

Wherever a file is read, `-` names standard input. `pack -` streams and needs
`--name`, since a pipe carries no filename to record. The reading verbs spool
first, because a ZIP's central directory is at the end of the file and there is
no seeking in a pipe.

Exit codes tell success from bad input, from a bad command line, from a
container this build cannot judge — the difference between "go and look at the
file" and "re-read `--help`". `slipcase --help` states the contract.

The library this is built on is [`slpc`](https://crates.io/crates/slpc), in the
same repository. The specification lives in `excelano/slipcase` and is the
authority on the format.

## License

MIT. See [LICENSE](https://github.com/excelano/slpc-rust/blob/main/LICENSE).
