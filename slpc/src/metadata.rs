// The metadata member: reading the two keys SPEC 2.2 requires, and checking a
// document that is about to be written.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use toml_edit::{value, DocumentMut, Item, Table};

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

/// Follow a key through the document.
///
/// The key may be dotted, as `payload.file` is, in which case each part is a
/// step down. Both keys this library knows are its own constants, so a key
/// whose own name contains a dot cannot reach here.
pub(crate) fn lookup<'d>(doc: &'d DocumentMut, key: &str) -> Option<&'d Item> {
    key.split('.')
        .try_fold(doc.as_item(), |item, part| item.get(part))
}

/// Read a required key that SPEC 2.2 says is a string.
pub(crate) fn required_string<'d>(doc: &'d DocumentMut, key: &'static str) -> Result<&'d str> {
    lookup(doc, key)
        .ok_or(Malformed::MissingKey(key))?
        .as_str()
        .ok_or_else(|| Malformed::KeyNotAString(key).into())
}

/// Set a key, creating any table on the way down to it.
///
/// Any table this has to create is created explicitly, because indexing through
/// a missing key leaves `toml_edit` to invent the shape and it invents
/// `payload = { file = "..." }`. That is valid TOML and a conformant container,
/// but it is not what SPEC 2.2's example looks like, and this is the
/// implementation whose output people will copy. It also reads worse the moment
/// a second key joins it under the same table.
///
/// The error names `payload` because that is the only intermediate there is:
/// both keys this library knows are its own constants and only one of them is
/// dotted.
pub(crate) fn set(doc: &mut DocumentMut, key: &str, to: &str) -> Result<()> {
    let path: Vec<&str> = key.split('.').collect();
    let (last, tables) = path.split_last().expect("a key is never empty");

    let mut at = doc.as_item_mut();
    for part in tables {
        match at.get(part) {
            None => at[*part] = Item::Table(Table::new()),
            Some(item) if item.is_table_like() => {}
            Some(_) => return Err(Malformed::PayloadNotATable.into()),
        }
        at = &mut at[*part];
    }
    at[*last] = value(to);
    Ok(())
}
