# slpc-rust

The Rust implementation of [slipcase](https://github.com/excelano/slipcase), a container format that attaches metadata to a file.

A `.slpc` file is a ZIP archive holding a payload file of any type together with a TOML metadata file describing it. The two become one file, so copying, moving, or sending the payload carries its metadata along.

This repository is a Cargo workspace holding two crates:

- **`slpc`** — the library. Reads, writes, and validates containers.
- **`slipcase`** — the command-line tool, built on the library.

The specification lives in `excelano/slipcase` and is the authority on the format. This is a reference implementation: it exists to show that the specification is implementable and to be checked against it.

## This repository

`DESIGN.md` records the design and the reasoning behind each decision.

## Status

Design draft. Nothing is built.

## License

MIT. See [LICENSE](LICENSE).
