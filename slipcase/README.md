# slipcase

The command-line tool for [slipcase](https://github.com/excelano/slipcase), a
container format that attaches metadata to a file.

A `.slpc` file is a ZIP archive holding a payload file of any type together with
a TOML metadata document describing it. The two become one file, so copying,
moving, or sending the payload carries its metadata along.

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
