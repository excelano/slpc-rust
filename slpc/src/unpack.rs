// Extracting a container: the content file, and on request the flyleaf and
// additional members, written into a directory as one unit.
//
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use crate::container::{locate, Container, Located};
use crate::error::{EntryKind, MemberError, MemberNameError, Result, Unsupported};
use crate::{dest, name, Destination, FLYLEAF_MEMBER};

/// Write a container's content file into a directory, with the flyleaf and
/// additional members when asked for.
///
/// Requires the `fs` feature. Implements SPEC 3's rules for extraction: the
/// content file always, the flyleaf only on request, and another member only
/// when the caller names it or asks for all of them.
///
/// **One request, one unit.** Everything is checked before anything is
/// written: every member asked for has to be a regular file entry under a name
/// [`check_member_name`](crate::check_member_name) accepts, carried by one
/// member only, readable by this build, and not named as a directory that
/// another name passes through. A directory entry among the members asked for
/// is passed over. If writing then fails partway, the files and directories
/// this request created are removed before the error is returned.
///
/// **No link is followed beneath the destination.** Directories are created
/// one level at a time, and an existing component that is a symbolic link, or
/// anything other than a directory, refuses the request. Each file goes
/// through [`Destination`], so nothing is replaced without
/// [`force`](Unpack::force) and every file gets the permissions the umask
/// gives a new one.
///
/// **Known limitation.** Checking a path component and then using it is not
/// atomic: the standard library has no `openat`, and this crate forbids
/// `unsafe`. A process racing to swap a link into the destination while
/// extraction runs is not stopped. Extract into a directory no other user can
/// write to.
///
/// ```no_run
/// # fn main() -> slpc::Result<()> {
/// let mut c = slpc::Container::open("report.pdf.slpc")?;
/// let written = slpc::Unpack::new("out")
///     .member("records/events.toml")
///     .carry_from("report.pdf.slpc")
///     .write(&mut c)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Unpack {
    dir: PathBuf,
    flyleaf: bool,
    members: Vec<String>,
    all_members: bool,
    force: bool,
    carry_from: Option<PathBuf>,
}

enum Source {
    Content,
    Flyleaf,
    Member(usize),
}

impl Unpack {
    /// Extract into `dir`, which has to exist.
    #[must_use]
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            dir: dir.as_ref().to_owned(),
            flyleaf: false,
            members: Vec::new(),
            all_members: false,
            force: false,
            carry_from: None,
        }
    }

    /// Also write the flyleaf member, as stored.
    #[must_use]
    pub fn flyleaf(mut self) -> Self {
        self.flyleaf = true;
        self
    }

    /// Also write the additional member carrying `name`.
    #[must_use]
    pub fn member(mut self, name: &str) -> Self {
        self.members.push(name.to_owned());
        self
    }

    /// Also write every additional member. Names passed to
    /// [`member`](Unpack::member) are then redundant.
    #[must_use]
    pub fn all_members(mut self) -> Self {
        self.all_members = true;
        self
    }

    /// Replace files already at the paths being written.
    ///
    /// A replaced file is not one this request created, so a failure later in
    /// the request leaves the replacement where it is.
    #[must_use]
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Carry what the platform records about where `container` came from onto
    /// every file written, through [`provenance::carry`](crate::provenance::carry).
    /// A failure to carry it fails the request.
    #[must_use]
    pub fn carry_from<P: AsRef<Path>>(mut self, container: P) -> Self {
        self.carry_from = Some(container.as_ref().to_owned());
        self
    }

    /// Check the request against the container, then write it.
    ///
    /// Returns the paths written, in the order they were written: the flyleaf
    /// when asked for, the content file, then each member. Show them to a
    /// person through [`display_path`](crate::display_path).
    pub fn write<R: Read + Seek>(self, c: &mut Container<R>) -> Result<Vec<PathBuf>> {
        c.check_content_readable()?;
        let plan = self.plan(c)?;
        let base = dest::addressable(&self.dir)?;

        let mut created = Vec::new();
        let mut written = Vec::new();
        for (rel, source) in &plan {
            if let Err(e) = self.place(c, &base, rel, source, &mut created, &mut written) {
                for p in created.iter().rev() {
                    let _ = match std::fs::symlink_metadata(p) {
                        Ok(m) if m.is_dir() => std::fs::remove_dir(p),
                        _ => std::fs::remove_file(p),
                    };
                }
                return Err(e);
            }
        }
        Ok(written)
    }

    /// Every file the request writes, as a `/`-separated path under the
    /// destination.
    fn plan<R: Read + Seek>(&self, c: &Container<R>) -> Result<Vec<(String, Source)>> {
        let content = c.content_name().to_owned();
        let mut plan = Vec::new();
        if self.flyleaf {
            plan.push((FLYLEAF_MEMBER.to_owned(), Source::Flyleaf));
        }
        plan.push((content.clone(), Source::Content));

        let reserved = |n: &str| n == FLYLEAF_MEMBER || n == content;
        let mut members = Vec::new();
        if self.all_members {
            let mut counts: HashMap<String, usize> = HashMap::new();
            let mut decoded = Vec::new();
            for recorded in &c.names {
                let Some(n) = recorded.decoded() else {
                    return Err(MemberError::Name {
                        name: String::from_utf8_lossy(&recorded.bytes).into_owned(),
                        cause: MemberNameError::Undecodable,
                    }
                    .into());
                };
                *counts.entry(n.clone()).or_default() += 1;
                decoded.push(n);
            }
            for n in decoded {
                if reserved(&n) {
                    continue;
                }
                if let Some(&count) = counts.get(&n).filter(|&&k| k > 1) {
                    return Err(MemberError::Ambiguous { name: n, count }.into());
                }
                if let Located::One(i) = locate(&c.entries, &c.names, &n) {
                    members.push((n, i));
                }
            }
        } else {
            let mut seen = HashSet::new();
            for n in &self.members {
                if !seen.insert(n.as_str()) {
                    return Err(MemberError::RequestedTwice(n.clone()).into());
                }
                if reserved(n) {
                    return Err(MemberError::Reserved(n.clone()).into());
                }
                match locate(&c.entries, &c.names, n) {
                    Located::One(i) => members.push((n.clone(), i)),
                    Located::None => return Err(MemberError::Missing(n.clone()).into()),
                    Located::Several(count) => {
                        return Err(MemberError::Ambiguous {
                            name: n.clone(),
                            count,
                        }
                        .into())
                    }
                }
            }
        }

        for (n, i) in members {
            let entry = &c.entries[i];
            if entry.kind == EntryKind::Directory || n.ends_with('/') {
                continue;
            }
            if entry.kind != EntryKind::Regular {
                return Err(MemberError::NotARegularFile {
                    name: n,
                    kind: entry.kind,
                }
                .into());
            }
            name::check_member_name(&n).map_err(|cause| MemberError::Name {
                name: n.clone(),
                cause,
            })?;
            if entry.encrypted {
                return Err(Unsupported::Encrypted.into());
            }
            if let Some(m) = entry.unsupported_method {
                return Err(Unsupported::Compression(m).into());
            }
            plan.push((n, Source::Member(i)));
        }

        let files: HashSet<&str> = plan.iter().map(|(rel, _)| rel.as_str()).collect();
        for (rel, _) in &plan {
            for (at, _) in rel.match_indices('/') {
                if files.contains(&rel[..at]) {
                    return Err(MemberError::DirectoryClash {
                        file: rel[..at].to_owned(),
                        under: rel.clone(),
                    }
                    .into());
                }
            }
        }
        Ok(plan)
    }

    /// Write one file of the plan, creating the directories it needs.
    fn place<R: Read + Seek>(
        &self,
        c: &mut Container<R>,
        base: &Path,
        rel: &str,
        source: &Source,
        created: &mut Vec<PathBuf>,
        written: &mut Vec<PathBuf>,
    ) -> Result<()> {
        let mut segments: Vec<&str> = rel.split('/').collect();
        let file_name = segments.pop().unwrap_or_default();
        let mut path = base.to_owned();
        for segment in segments {
            path.push(segment);
            match std::fs::symlink_metadata(&path) {
                Ok(m) if m.is_dir() => {}
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(refusal(
                        &path,
                        "is a symbolic link, which extraction does not follow",
                    ));
                }
                Ok(_) => return Err(refusal(&path, "is in the way: it is not a directory")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(&path)?;
                    created.push(path.clone());
                }
                Err(e) => return Err(e.into()),
            }
        }
        path.push(file_name);

        let mut out = Destination::new(&path, self.force)?;
        match source {
            Source::Content => {
                std::io::copy(&mut c.content()?, out.writer())?;
            }
            Source::Flyleaf => out.writer().write_all(c.flyleaf_bytes())?,
            Source::Member(i) => {
                std::io::copy(&mut c.archive.by_index(*i)?, out.writer())?;
            }
        }
        let replacing = std::fs::symlink_metadata(&path).is_ok();
        out.commit()?;
        if !replacing {
            created.push(path.clone());
        }
        if let Some(from) = &self.carry_from {
            crate::provenance::carry(from, &path)?;
        }
        written.push(path);
        Ok(())
    }
}

fn refusal(path: &Path, why: &str) -> crate::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("{} {why}", dest::display_path(path)),
    )
    .into()
}
