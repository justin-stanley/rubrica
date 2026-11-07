//! Writing `out/`: into a new directory beside it, then renamed into place,
//! so `out` is either the whole site or what it was before.
//!
//! - `out` must not exist yet, or be an empty directory (which the rename
//!   replaces); anything else (a file, a symbolic link, a directory with
//!   anything in it) is refused, and left as it is.
//! - Its parent is reached component by component from `/` (or the working
//!   directory, for a relative path), each opened with `O_DIRECTORY |
//!   O_NOFOLLOW`: a symbolic link anywhere on the way is refused, never
//!   followed (macOS's `/var` and `/tmp` are links: give the real path).
//! - The new tree is made inside a temporary directory there, mode 0700
//!   while it is written (no other user can add to it), every directory and
//!   file created relative to its parent's descriptor (`mkdirat`, `openat`
//!   with `O_CREAT | O_EXCL | O_NOFOLLOW`): nothing is overwritten, no link
//!   is followed, nothing lands outside it, and each name is one plain
//!   segment (never empty, `.`, `..` or with a `/` or NUL).
//! - Every file is 0644 and every directory 0755, set with `fchmod` (so no
//!   umask changes them), whatever the source's mode: nothing executable,
//!   and no link, is ever written.
//! - Then the temporary directory is renamed onto `out` (`renameat`, atomic
//!   on one file system). On any failure it is removed, by descriptor,
//!   never following a link, and `out` is untouched.

use std::ffi::CStr;
use std::io::{self, Write as _};
use std::os::fd::OwnedFd;
use std::path::{Component, Path};

use rustix::fs::{self as rfs, AtFlags, Dir, FileType, Mode, OFlags};
use rustix::io::Errno;

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

const NEW_FILE: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

/// A site being written.
pub struct OutDir {
    parent: OwnedFd,
    name: String,
    temp: String,
    root: Option<OwnedFd>,
    /// The open directories of the last file written, from the root: each
    /// name and descriptor.
    chain: Vec<(String, OwnedFd)>,
    written: usize,
    /// Debug builds only: fail after this many files (a test's injected
    /// error, `RUBRICA_TEST_FAIL_WRITE_AFTER`).
    fail_after: Option<usize>,
}

fn named(e: impl Into<io::Error>, what: &str) -> io::Error {
    let e = e.into();
    io::Error::new(e.kind(), format!("{what}: {e}"))
}

/// Whether `s` can be one path segment we create.
fn plain(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains('/') && !s.contains('\0')
}

impl OutDir {
    /// Starts a site at `out` (see the module docs for what is refused).
    pub fn create(out: &Path) -> io::Result<Self> {
        let shown = out.display().to_string();
        let Some(Component::Normal(name)) = out.components().next_back() else {
            return Err(io::Error::other(format!("{shown}: not a directory name")));
        };
        let name = name
            .to_str()
            .filter(|n| plain(n))
            .ok_or_else(|| io::Error::other(format!("{shown}: not a plain UTF-8 name")))?
            .to_owned();
        let parent_path = out.parent().unwrap_or(Path::new(""));
        let mut dir: OwnedFd = if parent_path.is_absolute() {
            rfs::open("/", DIR_FLAGS, Mode::empty()).map_err(|e| named(e, "/"))?
        } else {
            rfs::openat(rfs::CWD, ".", DIR_FLAGS, Mode::empty())
                .map_err(|e| named(e, "the working directory"))?
        };
        let mut walked = std::path::PathBuf::new();
        for c in parent_path.components() {
            walked.push(c.as_os_str());
            let seg = match c {
                Component::RootDir | Component::CurDir => continue,
                Component::ParentDir => "..".as_ref(),
                Component::Normal(s) => s,
                Component::Prefix(_) => {
                    return Err(io::Error::other(format!("{shown}: unsupported path")))
                }
            };
            dir = match rfs::openat(&dir, seg, DIR_FLAGS, Mode::empty()) {
                Ok(fd) => fd,
                Err(Errno::LOOP | Errno::NOTDIR) => {
                    return Err(io::Error::other(format!(
                        "{}: a symbolic link or not a directory, on the way to {shown}: refused (give the real path)",
                        walked.display()
                    )))
                }
                Err(e) => return Err(named(e, &walked.display().to_string())),
            };
        }
        match rfs::statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => {}
            Err(e) => return Err(named(e, &shown)),
            Ok(st) => match FileType::from_raw_mode(st.st_mode) {
                FileType::Directory => {
                    let fd = rfs::openat(&dir, name.as_str(), DIR_FLAGS, Mode::empty())
                        .map_err(|e| named(e, &shown))?;
                    let mut entries = Dir::read_from(&fd).map_err(|e| named(e, &shown))?;
                    let busy = entries.any(|e| {
                        e.map(|e| !matches!(e.file_name().to_bytes(), b"." | b".."))
                            .unwrap_or(true)
                    });
                    if busy {
                        return Err(io::Error::other(format!(
                            "{shown} exists and is not empty: refused (remove it, or build into a new directory)"
                        )));
                    }
                }
                FileType::Symlink => {
                    return Err(io::Error::other(format!(
                        "{shown} is a symbolic link: refused"
                    )))
                }
                _ => {
                    return Err(io::Error::other(format!(
                        "{shown} exists and is no directory: refused"
                    )))
                }
            },
        }
        // A temporary name of our own beside it.
        let mut n = 0u32;
        let temp = loop {
            let t = format!(".{name}.rubrica-{}-{n}", std::process::id());
            match rfs::mkdirat(&dir, t.as_str(), Mode::from_raw_mode(0o700)) {
                Ok(()) => break t,
                Err(Errno::EXIST) if n < 1000 => n += 1,
                Err(e) => return Err(named(e, &format!("a temporary directory beside {shown}"))),
            }
        };
        let root = match rfs::openat(&dir, temp.as_str(), DIR_FLAGS, Mode::empty()) {
            Ok(fd) => fd,
            Err(e) => {
                let _ = rfs::unlinkat(&dir, temp.as_str(), AtFlags::REMOVEDIR);
                return Err(named(e, &temp));
            }
        };
        let fail_after = if cfg!(debug_assertions) {
            std::env::var("RUBRICA_TEST_FAIL_WRITE_AFTER")
                .ok()
                .and_then(|v| v.parse().ok())
        } else {
            None
        };
        Ok(Self {
            parent: dir,
            name,
            temp,
            root: Some(root),
            chain: Vec::new(),
            written: 0,
            fail_after,
        })
    }

    /// Writes `bytes` at `rel` (`/`-separated, under the site), 0644, its
    /// directories 0755. A file already there is an error.
    pub fn write(&mut self, rel: &str, bytes: &[u8]) -> io::Result<()> {
        if self.fail_after == Some(self.written) {
            return Err(io::Error::other(
                "an injected write failure (RUBRICA_TEST_FAIL_WRITE_AFTER)",
            ));
        }
        let segs: Vec<&str> = rel.split('/').collect();
        if !segs.iter().all(|s| plain(s)) {
            return Err(io::Error::other(format!(
                "{rel}: not a plain relative path"
            )));
        }
        let (file, dirs) = segs.split_last().expect("split gives one");
        // Keep the open directories this file shares with the last one.
        let keep = self
            .chain
            .iter()
            .zip(dirs.iter())
            .take_while(|((a, _), b)| a == *b)
            .count();
        self.chain.truncate(keep);
        for d in &dirs[keep..] {
            let parent = match self.chain.last() {
                Some((_, fd)) => fd,
                None => self.root.as_ref().expect("open while writing"),
            };
            match rfs::mkdirat(parent, *d, Mode::from_raw_mode(0o700)) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(e) => return Err(named(e, rel)),
            }
            let fd =
                rfs::openat(parent, *d, DIR_FLAGS, Mode::empty()).map_err(|e| named(e, rel))?;
            rfs::fchmod(&fd, Mode::from_raw_mode(0o755)).map_err(|e| named(e, rel))?;
            self.chain.push(((*d).to_owned(), fd));
        }
        let parent = match self.chain.last() {
            Some((_, fd)) => fd,
            None => self.root.as_ref().expect("open while writing"),
        };
        let fd = rfs::openat(parent, *file, NEW_FILE, Mode::from_raw_mode(0o600))
            .map_err(|e| named(e, rel))?;
        rfs::fchmod(&fd, Mode::from_raw_mode(0o644)).map_err(|e| named(e, rel))?;
        std::fs::File::from(fd)
            .write_all(bytes)
            .map_err(|e| named(e, rel))?;
        self.written += 1;
        Ok(())
    }

    /// Puts the site in place: the temporary directory, 0755, renamed onto
    /// `out`.
    pub fn commit(mut self) -> io::Result<()> {
        self.chain.clear();
        let root = self.root.take().expect("open while writing");
        let r = rfs::fchmod(&root, Mode::from_raw_mode(0o755))
            .and_then(|()| {
                rfs::renameat(
                    &self.parent,
                    self.temp.as_str(),
                    &self.parent,
                    self.name.as_str(),
                )
            })
            .map_err(|e| named(e, &self.name));
        if r.is_err() {
            self.root = Some(root);
        }
        r
    }

    /// Removes the temporary directory and everything in it, by
    /// descriptor, following no link.
    fn remove_temp(&mut self) {
        self.chain.clear();
        if let Some(root) = self.root.take() {
            remove_contents(&root, 0);
            drop(root);
            let _ = rfs::unlinkat(&self.parent, self.temp.as_str(), AtFlags::REMOVEDIR);
        }
    }
}

impl Drop for OutDir {
    /// A site never committed (an error, a panic) leaves nothing behind.
    fn drop(&mut self) {
        self.remove_temp();
    }
}

/// Removes everything in the directory open at `dir`: no link is followed
/// (`unlinkat` removes a link itself; directories are opened `O_NOFOLLOW`).
fn remove_contents(dir: &OwnedFd, depth: usize) {
    // The tree is ours, at most the build's depth; this bounds a hostile one.
    if depth > 256 {
        return;
    }
    let Ok(entries) = Dir::read_from(dir) else {
        return;
    };
    let names: Vec<(Vec<u8>, FileType)> = entries
        .filter_map(Result::ok)
        .map(|e| (e.file_name().to_bytes().to_vec(), e.file_type()))
        .filter(|(n, _)| n.as_slice() != b"." && n.as_slice() != b"..")
        .collect();
    for (name, ft) in names {
        let Ok(c) = std::ffi::CString::new(name) else {
            continue;
        };
        let c: &CStr = &c;
        let is_dir = match ft {
            FileType::Directory => true,
            FileType::Unknown => rfs::statat(dir, c, AtFlags::SYMLINK_NOFOLLOW)
                .is_ok_and(|st| FileType::from_raw_mode(st.st_mode) == FileType::Directory),
            _ => false,
        };
        if is_dir {
            if let Ok(sub) = rfs::openat(dir, c, DIR_FLAGS, Mode::empty()) {
                remove_contents(&sub, depth + 1);
            }
            let _ = rfs::unlinkat(dir, c, AtFlags::REMOVEDIR);
        } else {
            let _ = rfs::unlinkat(dir, c, AtFlags::empty());
        }
    }
}
