// Archives built byte by byte, for the cases no ordinary writer will produce.
//
// The suite generates every fixture it uses, so it runs on a fresh clone with
// no network and nothing binary is checked in. A writer that could produce a
// CP437 member name, a name flagged UTF-8 that is not UTF-8, or a payload
// declaring a compression method this build lacks would be a writer with bugs,
// so those archives are stamped here instead.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

#![allow(dead_code)]

/// Unix, regular file, 0644.
const UNIX_FILE: (u16, u32) = (0x031E, 0o100_644 << 16);

/// One member, with every header field the tests need to reach.
pub struct Member {
    pub name: Vec<u8>,
    pub data: Vec<u8>,
    pub flags: u16,
    pub method: u16,
    pub version_made_by: u16,
    pub external_attributes: u32,
    /// The member's extra field block, written to both headers.
    ///
    /// Empty for almost every fixture. It exists because one field in there,
    /// Info-ZIP's Unicode Path at tag 0x7075, replaces the name a member is
    /// recorded under — so a test about names has to be able to write one.
    pub extra: Vec<u8>,
}

impl Member {
    /// A stored member with an ASCII name, made on unix.
    pub fn new(name: &str, data: &[u8]) -> Self {
        Self {
            name: name.as_bytes().to_vec(),
            data: data.to_vec(),
            flags: 0,
            method: 0,
            version_made_by: UNIX_FILE.0,
            external_attributes: UNIX_FILE.1,
            extra: Vec::new(),
        }
    }

    /// A member whose name is raw bytes rather than a Rust string.
    pub fn named_raw(name: &[u8], data: &[u8]) -> Self {
        let mut m = Self::new("", data);
        m.name = name.to_vec();
        m
    }

    /// Set general purpose bit 11, claiming the name is UTF-8.
    pub fn flagged_utf8(mut self) -> Self {
        self.flags |= 1 << 11;
        self
    }

    /// Set general purpose bit 0, claiming the data is encrypted.
    pub fn encrypted(mut self) -> Self {
        self.flags |= 1;
        self
    }

    /// Declare a compression method without compressing anything, which is
    /// enough to test that the method is noticed before the data is touched.
    pub fn claims_method(mut self, method: u16) -> Self {
        self.method = method;
        self
    }

    /// Mark the entry a symbolic link.
    pub fn symlink(self) -> Self {
        self.with_mode(0o120_777)
    }

    /// Set the unix mode, file-type bits included.
    pub fn with_mode(mut self, mode: u32) -> Self {
        self.external_attributes = mode << 16;
        self
    }

    /// Made on MS-DOS, where there are no unix permission bits to read.
    pub fn dos_made(mut self) -> Self {
        self.version_made_by = 0x0014;
        self.external_attributes = 0x20;
        self
    }
}

/// Stamp a ZIP archive. Every member is stored, so the bytes on disk are the
/// bytes handed in and nothing has to be compressed to be checked.
pub fn raw_zip(members: &[Member]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();

    for m in members {
        let crc = crc32fast::hash(&m.data);
        let offset = u32::try_from(out.len()).expect("fixture under 4 GiB");
        let len = u32::try_from(m.data.len()).expect("fixture under 4 GiB");
        let name_len = u16::try_from(m.name.len()).expect("name under 64 KiB");
        let extra_len = u16::try_from(m.extra.len()).expect("extra under 64 KiB");

        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&m.flags.to_le_bytes());
        out.extend_from_slice(&m.method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0x21u16.to_le_bytes()); // date: 1980-01-01
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes()); // compressed
        out.extend_from_slice(&len.to_le_bytes()); // uncompressed
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&extra_len.to_le_bytes());
        out.extend_from_slice(&m.name);
        out.extend_from_slice(&m.extra);
        out.extend_from_slice(&m.data);

        central.extend_from_slice(&0x0201_4B50u32.to_le_bytes());
        central.extend_from_slice(&m.version_made_by.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&m.flags.to_le_bytes());
        central.extend_from_slice(&m.method.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x21u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&extra_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&m.external_attributes.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(&m.name);
        central.extend_from_slice(&m.extra);
    }

    let count = u16::try_from(members.len()).expect("under 64 Ki members");
    let central_len = u32::try_from(central.len()).expect("directory under 4 GiB");
    let central_at = u32::try_from(out.len()).expect("fixture under 4 GiB");

    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk
    out.extend_from_slice(&0u16.to_le_bytes()); // disk with directory
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_len.to_le_bytes());
    out.extend_from_slice(&central_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    out
}

/// The smallest conformant metadata document.
///
/// The name goes into a TOML basic string, so a backslash or a quote in it is
/// escaped here. A fixture that writes a bad name is testing the library, and a
/// fixture that writes bad TOML by accident is testing nothing.
pub fn metadata(payload_file: &str) -> String {
    let escaped: String = payload_file
        .chars()
        .map(|c| match c {
            '\\' => "\\\\".to_string(),
            '"' => "\\\"".to_string(),
            // A control character cannot sit raw in a TOML basic string, so a
            // real container carrying one in payload.file escapes it and the
            // fixture has to as well. The name still holds the character; only
            // its spelling in the document changes.
            c if c.is_ascii() && c.is_control() => format!("\\u{:04X}", c as u32),
            c => c.to_string(),
        })
        .collect();
    format!("slipcase_version = \"1.0\"\n\n[payload]\nfile = \"{escaped}\"\n")
}

/// A container holding one stored payload and nothing else.
pub fn container(payload_file: &str, payload: &[u8]) -> Vec<u8> {
    raw_zip(&[
        Member::new(slpc::METADATA_MEMBER, metadata(payload_file).as_bytes()),
        Member::new(payload_file, payload),
    ])
}

/// Open a container from bytes.
pub fn open(bytes: &[u8]) -> slpc::Result<slpc::Container<std::io::Cursor<Vec<u8>>>> {
    slpc::Container::read(std::io::Cursor::new(bytes.to_vec()))
}

/// Read a payload out whole, for comparison.
///
/// Both test files want this, so it lives here rather than in each of them.
pub fn payload_of(c: &mut slpc::Container<std::io::Cursor<Vec<u8>>>) -> Vec<u8> {
    use std::io::Read;
    let mut got = Vec::new();
    c.payload().unwrap().read_to_end(&mut got).unwrap();
    got
}
