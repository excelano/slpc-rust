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

Nothing here takes or returns a container. Each of these reads a stream and
writes a stream, so a rewrite cannot accidentally hold a payload in memory.

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
so a container can be packed from a pipe into a socket.

## Changing one that already exists

`Repack` replaces the metadata, the payload, or both. Every other member is
copied through as stored bytes, in the order it arrived in, whether or not this
build can decompress it — which is what the specification requires of an
implementation rewriting a container, and what lets one be changed without being
fully understood.

```no_run
fn main() -> Result<(), slpc::Error> {
    let source = std::fs::File::open("report.pdf.slpc")?;
    let out = std::fs::File::create("report-v2.pdf.slpc")?;

    slpc::Repack::new(source)
        .payload_file("report-v2.pdf")?
        .write(out)?;
    Ok(())
}
```

The source and the destination both seek: a ZIP's central directory is at the
end of the file, and a member copied through already knows its compressed size,
which belongs in the header rather than in a promise of one to come. Packing has
neither constraint and keeps its `Write`-only destination.

A payload written under a new name carries `payload.file` with it. A payload
written under the name the container already used leaves the metadata member
alone, byte for byte. `rewrite_metadata` and `rewrite_metadata_bytes` are the
metadata-only case, which should not need a builder to say.

Everything on the way out is checked against the rules the read path reads by,
so what this writes is what it would accept back.

## Putting one on disk

Everything above writes into a stream the caller supplies. Turning on the `fs`
feature adds `Destination`, which writes a container to a path: through a
temporary file beside it, with the permissions a file there should have, renamed
into place at the end. A write that fails partway leaves nothing behind rather
than a truncated container that looks like one.

```toml
slpc = { version = "0.3", features = ["fs"] }
```

It is a feature rather than part of the default surface because a caller writing
into a socket or a buffer should not acquire a temporary-file dependency to do
it.

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
