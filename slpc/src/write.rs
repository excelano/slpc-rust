// Writing a container: packing one, and rewriting the metadata of one that
// already exists.
//
// None of these take or return a [`Container`]. They are stream-to-stream
// operations, so they are free functions beside [`crate::validate`] rather than
// associated functions on a type that means "a container open for reading".
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::io::{Read, Seek, Write};
use std::path::Path;

use toml_edit::{value, DocumentMut};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::container::{locate_payload, Container};
use crate::error::{Malformed, NameError, Result, Unsupported};
use crate::{metadata, name, METADATA_MEMBER, PAYLOAD_FILE_KEY, VERSION, VERSION_KEY};

/// Pack a payload read from a stream.
///
/// Takes a `Read` and not a `Read + Seek`. Requiring seek would allow two
/// passes over the payload to measure it before the local header goes down, and
/// it would also rule out pipes, sockets, and anything generated as it is
/// written, which are what this form exists for. The member therefore goes out
/// with a data descriptor, which is conformant and which the specification
/// does not constrain.
///
/// `payload_name` is the name the payload will be stored and described under,
/// and it has to satisfy SPEC 2.3. That failure is a caller passing a bad
/// argument, which is why it reads differently from [`pack_file`]'s.
///
/// The writer needs only `Write`, so a container can be packed straight into a
/// socket.
pub fn pack_reader<M, R, W>(payload_name: &str, payload: R, metadata: M, out: W) -> Result<()>
where
    M: Into<DocumentMut>,
    R: Read,
    W: Write,
{
    name::check_payload_name(payload_name)?;
    pack(payload_name, payload, metadata, out)
}

/// Pack a payload named by a path, taking `payload.file` from the path.
///
/// The failure this has and [`pack_reader`] does not is a file on disk called
/// something `payload.file` cannot express. That is a payload which cannot be
/// packed as itself rather than a bad argument, and the two would read wrong
/// collapsed together: one of them would be complaining about an argument the
/// caller never supplied.
///
/// The payload is rejected rather than renamed. SPEC 3 requires refusing a name
/// that breaks SPEC 2.3 rather than sanitizing it, and the same reasoning holds
/// on the way in: a container that cannot name its own payload is worse than a
/// refusal to write one.
pub fn pack_file<M, P, W>(payload_path: P, metadata: M, out: W) -> Result<()>
where
    M: Into<DocumentMut>,
    P: AsRef<Path>,
    W: Write,
{
    let path = payload_path.as_ref();
    let bad = |cause: NameError| Malformed::PayloadPathName {
        path: path.to_owned(),
        cause,
    };

    let name = path.file_name().ok_or_else(|| bad(NameError::Empty))?;
    let name = name.to_str().ok_or_else(|| bad(NameError::NotUtf8))?;
    name::check_payload_name(name).map_err(bad)?;

    pack(name, std::fs::File::open(path)?, metadata, out)
}

/// Replace a container's metadata, preserving everything else.
///
/// Every other member is copied through untouched, as SPEC 3 requires, and in
/// the order it arrived in. Members are copied compressed, so a container
/// survives a rewrite whether or not this build can read every member in it.
///
/// This is a reader and a writer rather than a container that gets saved,
/// because a rewrite that streams cannot accidentally hold the payload in
/// memory and a container is never half in memory and half on disk.
///
/// The source must be a conformant container. Rewriting one that is not would
/// mean guessing what it was meant to say.
pub fn rewrite_metadata<R, W>(source: R, metadata: &DocumentMut, out: W) -> Result<()>
where
    R: Read + Seek,
    W: Write,
{
    rewrite_metadata_bytes(source, metadata.to_string().as_bytes(), out)
}

/// Replace a container's metadata with bytes chosen by the caller.
///
/// For a caller holding a document this library did not produce: one from
/// another parser, or one edited as text. The bytes are stored as handed in,
/// which is the only way to put an exact document into a container.
///
/// **The library validates what it is about to write.** Valid TOML, UTF-8, both
/// required keys present and of the right type, and `payload.file` naming a
/// member the archive actually contains and which is not a symbolic link entry.
/// A key that is present and is a string still describes nothing if it points
/// at a member that is not there, and a caller free to write bytes is free to
/// break it that way. Without these checks this would be a way to produce a
/// non-conformant container from the reference implementation.
pub fn rewrite_metadata_bytes<R, W>(source: R, metadata: &[u8], out: W) -> Result<()>
where
    R: Read + Seek,
    W: Write,
{
    let mut c = Container::read(source)?;
    if !c.version_is_recognised() {
        return Err(Unsupported::Version(c.version().to_owned()).into());
    }

    let (_, keys) = metadata::parse(metadata)?;
    if keys.version != VERSION {
        return Err(Malformed::Disagrees {
            key: VERSION_KEY,
            found: keys.version,
            writing: VERSION.to_owned(),
        }
        .into());
    }
    // Against the archive it is about to describe, not against the one it came
    // from, because those are the same archive here and will not be elsewhere.
    locate_payload(&c.entries, &c.names, &keys.payload_file)?;

    let mut w = ZipWriter::new_stream(out);
    for i in 0..c.entries.len() {
        // A second member named `slipcase.metadata.toml` is copied rather than
        // substituted or dropped. SPEC 3 says members this library does not
        // recognize survive a rewrite, and the result still reads back as the
        // document written here, since the first match is the one that counts.
        if i == c.metadata_index {
            w.start_file(METADATA_MEMBER, options())?;
            w.write_all(metadata)?;
        } else {
            w.raw_copy_file(c.archive.by_index_raw(i)?)?;
        }
    }
    finish(w)
}

/// Deflate, which every ZIP reader has had since 1993.
fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

/// Close the archive and flush whatever it was writing into.
///
/// A buffered writer that is only dropped swallows the error from its last
/// flush, which would turn a full disk into a container that looks written.
fn finish<W: Write>(w: ZipWriter<zip::write::StreamWriter<W>>) -> Result<()> {
    w.finish()?.into_inner().flush()?;
    Ok(())
}

/// Pack, with the name already checked by whichever form was called.
fn pack<M, R, W>(payload_name: &str, mut payload: R, metadata: M, out: W) -> Result<()>
where
    M: Into<DocumentMut>,
    R: Read,
    W: Write,
{
    let doc = with_required_keys(metadata.into(), payload_name)?;

    let mut w = ZipWriter::new_stream(out);
    w.start_file(METADATA_MEMBER, options())?;
    w.write_all(doc.to_string().as_bytes())?;
    w.start_file(payload_name, options())?;
    std::io::copy(&mut payload, &mut w)?;
    finish(w)
}

/// Set both required keys, and refuse metadata that contradicts them.
///
/// The library sets `payload.file` and `slipcase_version` itself. The caller
/// supplies neither and so cannot be inconsistent about either. Metadata that
/// sets one anyway is an error rather than a silent overwrite, because the
/// caller meant one of the two things and there is no way to tell which.
/// Everything else in the document passes through untouched.
fn with_required_keys(mut doc: DocumentMut, payload_name: &str) -> Result<DocumentMut> {
    agree_or_set(&mut doc, VERSION_KEY, VERSION)?;

    match doc.get("payload") {
        None => {}
        Some(item) if item.is_table_like() => {}
        Some(_) => return Err(Malformed::PayloadNotATable.into()),
    }
    agree_or_set(&mut doc, PAYLOAD_FILE_KEY, payload_name)?;

    Ok(doc)
}

/// Leave a key that already says the right thing, set one that is absent, and
/// refuse one that says something else.
fn agree_or_set(doc: &mut DocumentMut, key: &'static str, writing: &str) -> Result<()> {
    let path: Vec<&str> = key.split('.').collect();
    let existing = path
        .iter()
        .try_fold(doc.as_item(), |item, part| item.get(part));

    match existing {
        None => {
            // Any table this has to create is created explicitly, because
            // indexing through a missing key leaves toml_edit to invent the
            // shape and it invents `payload = { file = "..." }`. That is valid
            // TOML and a conformant container, but it is not what SPEC 2.2's
            // example looks like, and this is the implementation whose output
            // people will copy. It also reads worse the moment a second key
            // joins it under the same table.
            let mut at = doc.as_item_mut();
            for part in &path[..path.len() - 1] {
                if at.get(part).is_none() {
                    at[*part] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                at = &mut at[*part];
            }
            at[path[path.len() - 1]] = value(writing);
            Ok(())
        }
        Some(item) => {
            let found = item.as_str().ok_or(Malformed::KeyNotAString(key))?;
            if found == writing {
                Ok(())
            } else {
                Err(Malformed::Disagrees {
                    key,
                    found: found.to_owned(),
                    writing: writing.to_owned(),
                }
                .into())
            }
        }
    }
}
