# slpc

The library half of the Rust implementation of [slipcase](https://github.com/excelano/slipcase),
a container format that attaches metadata to a file.

A `.slpc` file is a ZIP archive holding a payload file of any type together with
a TOML metadata document describing it. The two become one file, so copying,
moving, or sending the payload carries its metadata along.

```rust
let mut c = slpc::Container::open("report.pdf.slpc")?;
println!("{} holds {}", c.version(), c.payload_name());
std::io::copy(&mut c.payload()?, &mut std::io::stdout())?;
```

The payload is a stream and is never read into memory. Metadata is exposed as a
document that keeps comments, key order, and whitespace across a rewrite, and as
the member's bytes for a caller who wants a different parser or a hash.

The command-line tool built on this library is `slipcase`, in the same
repository. The specification lives in `excelano/slipcase` and is the authority
on the format.

## License

MIT. See [LICENSE](https://github.com/excelano/slpc-rust/blob/main/LICENSE).
