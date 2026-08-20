// The metadata member: reading the two keys SPEC 2.2 requires, and checking a
// document that is about to be written.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use toml_edit::DocumentMut;

use crate::error::{Malformed, Result};
use crate::{PAYLOAD_FILE_KEY, VERSION_KEY};

/// The two keys every version of the specification is dispatched on.
pub(crate) struct Keys {
    pub version: String,
    pub payload_file: String,
}

/// Parse a metadata member and read both required keys.
///
/// Used by the read path on the member it found and by the write path on the
/// bytes it is about to store, so that a document this library writes is one it
/// would accept back.
pub(crate) fn parse(bytes: &[u8]) -> Result<(DocumentMut, Keys)> {
    let text = std::str::from_utf8(bytes).map_err(|_| Malformed::MetadataNotUtf8)?;
    let doc: DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| Malformed::MetadataNotToml(e.to_string()))?;
    let keys = Keys {
        version: required_string(&doc, VERSION_KEY)?.to_owned(),
        payload_file: required_string(&doc, PAYLOAD_FILE_KEY)?.to_owned(),
    };
    Ok((doc, keys))
}

/// Read a required key that SPEC 2.2 says is a string.
///
/// The key may be dotted, as `payload.file` is. Both keys this library knows
/// are its own constants, so a key whose own name contains a dot cannot reach
/// here.
pub(crate) fn required_string<'d>(doc: &'d DocumentMut, key: &'static str) -> Result<&'d str> {
    let item = key
        .split('.')
        .try_fold(doc.as_item(), |item, part| item.get(part))
        .ok_or(Malformed::MissingKey(key))?;
    item.as_str()
        .ok_or_else(|| Malformed::KeyNotAString(key).into())
}
