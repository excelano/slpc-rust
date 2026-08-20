# slpc-rust

The Rust implementation of [slipcase](https://github.com/excelano/slipcase), a container format that attaches metadata to a file.

A `.slpc` file is a ZIP archive holding a payload file of any type together with a TOML metadata file describing it. The two become one file, so copying, moving, or sending the payload carries its metadata along.

This repository is a Cargo workspace holding two crates:

- **`slpc`** — the library. Reads, writes, and validates containers.
- **`slipcase`** — the command-line tool, built on the library.

The specification lives in `excelano/slipcase` and is the authority on the format. This is a reference implementation: it exists to show that the specification is implementable and to be checked against it.

## The tool

```
slipcase pack report.pdf --meta owner.toml     # writes report.pdf.slpc
slipcase info report.pdf.slpc                  # prints the metadata, verbatim
slipcase validate report.pdf.slpc              # exit 0 if conformant
slipcase unpack report.pdf.slpc --dest ./out   # writes the payload and nothing else
```

Wherever a file is read, `-` names standard input. Exit codes are 0 for success or conformance, 1 for bad input, and 2 for a bad command line.

## The library

```rust
let mut c = slpc::Container::open("report.pdf.slpc")?;
println!("{} holds {}", c.version(), c.payload_name());
std::io::copy(&mut c.payload()?, &mut std::io::stdout())?;
```

The payload is a stream and is never read into memory. Metadata is exposed as a document that keeps comments, key order, and whitespace across a rewrite, and as the member's bytes for a caller who wants a different parser or a hash.

## This repository

`DESIGN.md` records the design and the reasoning behind each decision. `RELEASING.md` is the release loop.

## License

MIT. See [LICENSE](LICENSE).
