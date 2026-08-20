# slpc

The library half of the Rust implementation of
[slipcase](https://github.com/excelano/slipcase), a container format that
attaches metadata to a file.

<!-- shared:blurb -->
A `.slpc` file is a ZIP archive holding a payload file of any type together with a TOML metadata document describing it. The two become one file, so copying, moving, or sending the payload carries its metadata along.
<!-- /shared:blurb -->

## Reading

```no_run
fn main() -> Result<(), slpc::Error> {
    let mut c = slpc::Container::open("report.pdf.slpc")?;
    println!("{} holds {}", c.version(), c.payload_name());
    std::io::copy(&mut c.payload()?, &mut std::io::stdout())?;
    Ok(())
}
```

The payload is a stream and is never read into memory. Metadata is exposed as a
document that keeps comments, key order, and whitespace across a rewrite, and as
the member's bytes for a caller who wants a different parser or a hash.

## Writing

Four free functions, because none of them takes or returns a container: each
reads a stream and writes a stream.

```no_run
use slpc::toml_edit::DocumentMut;

fn main() -> Result<(), slpc::Error> {
    let out = std::fs::File::create("report.pdf.slpc")?;
    slpc::pack_file("report.pdf", DocumentMut::new(), out)?;
    Ok(())
}
```

The metadata argument is anything convertible into a `DocumentMut`, which is a
document, a table, or, with `toml_edit`'s `serde` feature turned on by the
caller, whatever `toml_edit::ser::to_document` makes of a struct or a map.
Building metadata from nothing has no formatting to preserve, so there is
nothing for that conversion to lose.

`pack_reader` takes a payload from any `Read` and needs no `Seek` at either end,
so a container can be packed from a pipe into a socket. `rewrite_metadata`
replaces a container's metadata and copies every other member through
untouched, compressed members included.

## Validating

```no_run
fn main() -> Result<(), slpc::Error> {
    match slpc::validate(std::fs::File::open("report.pdf.slpc")?)? {
        slpc::Verdict::Conformant => println!("conformant"),
        other => println!("{other}"),
    }
    Ok(())
}
```

Four verdicts rather than two. A container whose metadata member cannot be read
is neither conformant nor non-conformant, and one declaring a version this build
does not implement is outside the question rather than failing it.

## No vocabulary

The two structural keys have typed accessors. Every other key is passed through
unexamined, because there is nothing to examine it against.

<!-- shared:authority -->
The specification lives in `excelano/slipcase` and is the authority on the format.
<!-- /shared:authority -->
This crate implements it and has no standing to change it. The command-line tool
built on it is `slipcase`, in the same repository.

## License

MIT. See [LICENSE](https://github.com/excelano/slpc-rust/blob/main/LICENSE).
