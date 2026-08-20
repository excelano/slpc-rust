// Writing a container: packing one, and repacking one that already exists.
//
// None of this takes or returns a [`Container`]. They are stream-to-stream
// operations, so they sit beside [`crate::validate`] rather than hanging off a
// type that means "a container open for reading".
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::io::{Read, Seek, Write};
use std::path::Path;

use toml_edit::DocumentMut;
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
    let name = name_from_path(path)?;
    pack(name, std::fs::File::open(path)?, metadata, out)
}

/// Change a container, keeping everything that is not being changed.
///
/// The metadata, the payload, or both. Every other member is copied through as
/// stored bytes, in the order it arrived in, which is what SPEC 3 requires of
/// an implementation rewriting a container. Nothing is decompressed and nothing
/// is recompressed, so a container survives this whether or not the build can
/// read every member in it, and the members left alone come out byte for byte.
///
/// A reader and a writer rather than a container that gets saved: a rewrite
/// that streams cannot accidentally hold the payload in memory, and a container
/// is never half in memory and half on disk. It follows that the source and the
/// destination are two streams, and writing a file back over itself is the
/// caller's job to arrange.
///
/// **Both of them seek.** The source has to, because a ZIP's central directory
/// is at the end of the file. The destination has to because of what copying a
/// member through means: its compressed size is already known, and a writer
/// that cannot seek has nowhere to put a size except a data descriptor after
/// the data, which is a promise to a reader that the bytes it just walked past
/// have a length coming. Writing the sizes into the header is both simpler and
/// the only thing a reader walking forward can use. Packing keeps its
/// `Write`-only destination, since a payload arriving from a pipe genuinely has
/// no size to write down; repacking never had a pipe for a source, so the bound
/// costs a caller nothing they were not already paying.
///
/// The source must be a conformant container declaring a version this build
/// implements. Rewriting one that is not would mean guessing what it was meant
/// to say.
///
/// ```no_run
/// # fn main() -> slpc::Result<()> {
/// let source = std::fs::File::open("report.pdf.slpc")?;
/// let out = std::fs::File::create("report-v2.pdf.slpc")?;
///
/// slpc::Repack::new(source)
///     .payload_file("report-v2.pdf")?
///     .write(out)?;
/// # Ok(()) }
/// ```
pub struct Repack<'a, R> {
    source: R,
    metadata: Option<NewMetadata<'a>>,
    payload: Option<(String, Box<dyn Read + 'a>)>,
}

/// A document is edited where it has to be; bytes are stored as handed in.
enum NewMetadata<'a> {
    Document(&'a DocumentMut),
    Bytes(&'a [u8]),
}

impl<'a, R: Read + Seek> Repack<'a, R> {
    /// Read a container to change.
    ///
    /// With neither [`Repack::metadata`] nor [`Repack::payload`] set, this
    /// checks the container and writes it back out unchanged, every member
    /// copied as stored.
    #[must_use]
    pub fn new(source: R) -> Self {
        Self {
            source,
            metadata: None,
            payload: None,
        }
    }

    /// Replace the metadata with a document.
    ///
    /// The document is serialized on the way out, which `toml_edit` does
    /// without losing comments, key order, or whitespace.
    ///
    /// When a payload is also being written, `payload.file` is set to that
    /// payload's name here, whatever the document said. It is the one key the
    /// caller cannot mean anything else by: the member the old value named is
    /// the member being replaced.
    #[must_use]
    pub fn metadata(mut self, document: &'a DocumentMut) -> Self {
        self.metadata = Some(NewMetadata::Document(document));
        self
    }

    /// Replace the metadata with bytes chosen by the caller.
    ///
    /// For a caller holding a document this library did not produce: one from
    /// another parser, or one edited as text. The bytes are stored as handed
    /// in, which is the only way to put an exact document into a container, and
    /// it is why these are not edited the way [`Repack::metadata`]'s are. Bytes
    /// that name a payload other than the one being written are refused rather
    /// than corrected, because correcting them would mean they were no longer
    /// the bytes handed in.
    #[must_use]
    pub fn metadata_bytes(mut self, bytes: &'a [u8]) -> Self {
        self.metadata = Some(NewMetadata::Bytes(bytes));
        self
    }

    /// Replace the payload with a stream, stored under `name`.
    ///
    /// The name has to satisfy SPEC 2.3 and has to be one no other member of
    /// the container already carries, since SPEC 2.1 allows a container exactly
    /// one member under the payload's name. Where it differs from the name the
    /// container used, `payload.file` moves with it.
    #[must_use]
    pub fn payload(mut self, name: &str, payload: impl Read + 'a) -> Self {
        self.payload = Some((name.to_owned(), Box::new(payload)));
        self
    }

    /// Replace the payload with a file, taking its name from the path.
    ///
    /// The same relationship [`pack_file`] has to [`pack_reader`], and the same
    /// failure: a file on disk called something `payload.file` cannot express
    /// is refused rather than renamed.
    pub fn payload_file<P: AsRef<Path>>(mut self, path: P) -> Result<Self> {
        let path = path.as_ref();
        let name = name_from_path(path)?.to_owned();
        self.payload = Some((name, Box::new(std::fs::File::open(path)?)));
        Ok(self)
    }

    /// Write the container out.
    ///
    /// **The library validates what it is about to write.** Valid TOML, UTF-8,
    /// both required keys present and of the right type, and `payload.file`
    /// naming a member that the container being written actually holds. A key
    /// that is present and is a string still describes nothing if it points at
    /// a member that is not there, and a caller free to write bytes is free to
    /// break it that way. Without these checks this would be a way to produce a
    /// non-conformant container from the reference implementation.
    ///
    /// The writer seeks, for the reason given on [`Repack`].
    pub fn write<W: Write + Seek>(self, out: W) -> Result<()> {
        let Self {
            source,
            metadata,
            mut payload,
        } = self;

        let mut c = Container::read(source)?;
        if !c.version_is_recognised() {
            return Err(Unsupported::Version(c.version().to_owned()).into());
        }

        // The payload's name settles what the metadata has to say, so it is
        // checked before anything is decided about the document.
        if let Some((name, _)) = &payload {
            name::check_payload_name(name)?;
            // One member may already carry this name: the payload itself, when
            // only its contents are being replaced. Any other is a collision,
            // and writing into it would leave the container with two members
            // under the payload's name (SPEC 2.1).
            let carried = c.names.iter().filter(|n| n.decodes_to(name)).count();
            if carried > usize::from(name == c.payload_name()) {
                return Err(Malformed::PayloadNameTaken(name.clone()).into());
            }
        }
        let payload_name = payload.as_ref().map(|(n, _)| n.as_str());

        // `None` copies the metadata member through as stored. Nothing else
        // reproduces its bytes: TOML defines no canonical serialization, so a
        // document that survives a parse and a re-serialization has still been
        // through both.
        let new_metadata: Option<Vec<u8>> = match (metadata, payload_name) {
            (Some(NewMetadata::Bytes(b)), _) => Some(b.to_vec()),
            (Some(NewMetadata::Document(d)), None) => Some(d.to_string().into_bytes()),
            (Some(NewMetadata::Document(d)), Some(n)) => Some(repointed(d, n)?),
            (None, Some(n)) if n != c.payload_name() => Some(repointed(c.metadata(), n)?),
            (None, _) => None,
        };

        if let Some(bytes) = &new_metadata {
            let (_, keys) = metadata::parse(bytes)?;
            if keys.version != VERSION {
                return Err(Malformed::Disagrees {
                    key: VERSION_KEY,
                    found: keys.version,
                    writing: VERSION.to_owned(),
                }
                .into());
            }
            match payload_name {
                // The payload is being written under a name already checked
                // against SPEC 2.3, so the metadata has to be the metadata for
                // that payload. Only bytes reach here disagreeing; a document
                // was repointed above.
                Some(n) if keys.payload_file != n => {
                    return Err(Malformed::Disagrees {
                        key: PAYLOAD_FILE_KEY,
                        found: keys.payload_file,
                        writing: n.to_owned(),
                    }
                    .into())
                }
                Some(_) => {}
                // Nothing is being written under a name of its own, so the
                // metadata has to point at a member that is already there.
                // Against the archive it is about to describe, not against the
                // one it came from, because those are the same archive here and
                // will not be elsewhere.
                None => {
                    locate_payload(&c.entries, &c.names, &keys.payload_file)?;
                }
            }
        }

        let payload_at = if payload.is_some() {
            c.payload_index
        } else {
            None
        };

        let mut w = ZipWriter::new(out);
        for i in 0..c.entries.len() {
            // A second member named `slipcase.metadata.toml` is copied rather
            // than substituted or dropped. SPEC 3 says members this library
            // does not recognize survive a rewrite, and the result still reads
            // back as the document written here, since the first match is the
            // one that counts.
            if i == c.metadata_index {
                match &new_metadata {
                    Some(bytes) => {
                        w.start_file(METADATA_MEMBER, options())?;
                        w.write_all(bytes)?;
                    }
                    None => w.raw_copy_file(c.archive.by_index_raw(i)?)?,
                }
            } else if let Some((name, data)) = payload.as_mut().filter(|_| Some(i) == payload_at) {
                w.start_file(name.as_str(), options())?;
                std::io::copy(data, &mut w)?;
            } else {
                w.raw_copy_file(c.archive.by_index_raw(i)?)?;
            }
        }
        w.finish()?.flush()?;
        Ok(())
    }
}

/// Replace a container's metadata, preserving everything else.
///
/// [`Repack::metadata`] with nothing else changed. It stays a function of its
/// own because the two-argument case should not need a builder to express.
pub fn rewrite_metadata<R, W>(source: R, metadata: &DocumentMut, out: W) -> Result<()>
where
    R: Read + Seek,
    W: Write + Seek,
{
    Repack::new(source).metadata(metadata).write(out)
}

/// Replace a container's metadata with bytes chosen by the caller.
///
/// [`Repack::metadata_bytes`] with nothing else changed.
pub fn rewrite_metadata_bytes<R, W>(source: R, metadata: &[u8], out: W) -> Result<()>
where
    R: Read + Seek,
    W: Write + Seek,
{
    Repack::new(source).metadata_bytes(metadata).write(out)
}

/// A copy of a document, pointed at the payload being written.
fn repointed(doc: &DocumentMut, payload_name: &str) -> Result<Vec<u8>> {
    let mut doc = doc.clone();
    metadata::set(&mut doc, PAYLOAD_FILE_KEY, payload_name)?;
    Ok(doc.to_string().into_bytes())
}

/// The filename a path ends in, if a payload may be stored under it.
fn name_from_path(path: &Path) -> Result<&str> {
    let bad = |cause: NameError| Malformed::PayloadPathName {
        path: path.to_owned(),
        cause,
    };

    let name = path.file_name().ok_or_else(|| bad(NameError::Empty))?;
    let name = name.to_str().ok_or_else(|| bad(NameError::NotUtf8))?;
    name::check_payload_name(name).map_err(bad)?;
    Ok(name)
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
///
/// Repacking a payload sets `payload.file` outright instead. There the old
/// value named the member being replaced, so it is not a second opinion about
/// what is being written.
fn with_required_keys(mut doc: DocumentMut, payload_name: &str) -> Result<DocumentMut> {
    agree_or_set(&mut doc, VERSION_KEY, VERSION)?;
    agree_or_set(&mut doc, PAYLOAD_FILE_KEY, payload_name)?;
    Ok(doc)
}

/// Leave a key that already says the right thing, set one that is absent, and
/// refuse one that says something else.
fn agree_or_set(doc: &mut DocumentMut, key: &'static str, writing: &str) -> Result<()> {
    let Some(item) = metadata::lookup(doc, key) else {
        return metadata::set(doc, key, writing);
    };

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
