# slipcase

The command-line tool for [slipcase](https://github.com/excelano/slipcase), a
container format that attaches metadata to a file.

<!-- shared:blurb -->
A `.slpc` file is a ZIP archive holding a payload file of any type together with a TOML metadata document describing it. The two become one file, so copying, moving, or sending the payload carries its metadata along.
<!-- /shared:blurb -->

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

## Using it

<!-- shared:verbs -->
```
slipcase pack report.pdf --meta owner.toml       # writes report.pdf.slpc
slipcase info report.pdf.slpc                    # prints the metadata, verbatim
slipcase repack report.pdf.slpc --meta new.toml  # changes it in place, keeping the rest
slipcase validate report.pdf.slpc                # exit 0 if conformant
slipcase unpack report.pdf.slpc --dest ./out     # writes the payload and nothing else
```
<!-- /shared:verbs -->

Five verbs, each doing one thing the format supports. `pack` sets both required
metadata keys itself, so a `--meta` file that contradicts either is refused
rather than silently overwritten, and a payload whose filename cannot be a
member name is rejected rather than renamed. `unpack` writes the payload and,
with `--metadata`, the metadata document; nothing else in the archive reaches
disk. It also carries whatever the platform records about where the container
came from onto the payload, so that unpacking something downloaded does not hand
its payload on as a file this machine made — and where it cannot, it removes the
payload and says so rather than leaving one that opens without the warning its
origin earned. A container read from standard input has no source to read that
from and is unpacked without it. `repack` changes the metadata, the payload, or both, and copies every
other member of the archive through untouched — which is how a container is
changed without losing what this tool does not understand, and why unpacking and
packing again is the wrong way to do it.

`repack` writes back over the container it was given unless `-o` names somewhere
else. It goes through a temporary file and a rename either way, and it reads back
what it wrote before replacing anything, so the failure that leaves you without a
container does not arise. Nothing else overwrites an existing file without
`--force`.

Wherever a file is read, `-` names standard input, and wherever one is written it
names standard output. `pack -` streams and needs `--name`, since a pipe carries
no filename to record. The reading verbs spool first, because a ZIP's central
directory is at the end of the file and there is no seeking in a pipe.

<!-- shared:exit-codes -->
Exit codes tell success from bad input, from a bad command line, from a container this build cannot judge. `slipcase --help` states the contract.
<!-- /shared:exit-codes -->

The library this is built on is [`slpc`](https://crates.io/crates/slpc), in the
same repository. 
<!-- shared:authority -->
The specification lives in `excelano/slipcase` and is the authority on the format.
<!-- /shared:authority -->

## License

MIT. See [LICENSE](https://github.com/excelano/slpc-rust/blob/main/LICENSE).
