//! Reading a wiki repository from a directory, for `rubrica check`.
//!
//! The directory is untrusted (a checkout of anyone's commit, which another
//! process may be changing while it is read), so:
//! - the walk is by **directory file descriptors**: the root is opened once
//!   (it may itself be a symbolic link, which the caller names and which is
//!   followed), and every directory and file below it is opened relative to
//!   its parent's descriptor with `openat(…, O_NOFOLLOW)`, a directory with
//!   `O_DIRECTORY` too, and listed from its own descriptor. No path of more
//!   than one component is ever resolved, so a directory swapped for a
//!   symbolic link after it was listed (a
//!   `RENAME_EXCHANGE` race) is refused (`ENOTDIR`, on Linux and macOS
//!   alike), never followed: the walk cannot leave the root;
//! - an entry that is no longer what the listing said (a directory that
//!   cannot be opened as one, a file that is now a link, gone, or not a
//!   regular file by `fstat`) fails the read, "`<path>` changed during the
//!   walk" (exit 2): never reclassified, so nothing under it vanishes
//!   unreported;
//! - so does a directory whose mtime or ctime moved while it was listed (a
//!   listing taken during a change may miss an entry: overlayfs dropped an
//!   exchanged pair from 19,284 of 20,000). This listing check is
//!   **best-effort**. It is near-complete with fine-grained timestamps
//!   (Linux 6.13 and later, whose multigrain timestamps tell apart changes
//!   within a tick, and APFS). On kernels and file systems with coarse
//!   timestamps, which include Ubuntu 22.04 and 24.04 runners (kernels 5.15
//!   and 6.8), a change within one tick leaves the stamp as it was, and a
//!   continuous racer gets through about 97% of the time: on overlayfs, at
//!   a timestamp granularity of 1 ns, 1 ms, 4 ms, 10 ms and 1 s, 0, 19,265,
//!   19,410, 19,240 and 19,521 of 20,000 walks lost the exchanged pair.
//!   That matters little: whoever can race the tree can
//!   delete its pages anyway;
//! - the check is per directory, not a snapshot of the tree: a page moved
//!   from a directory not yet walked into one already walked is in neither
//!   listing, and the walk succeeds without it, exit 0 (a stress run lost
//!   it in 822 of 2,000 walks on macOS, 117 of 5,000 on virtiofs, 21 of
//!   5,000 on tmpfs and 0 of 5,000 on overlayfs). The build reads the
//!   git tree, which is a snapshot;
//! - anything written into the tree during a check (`| tee report.json`
//!   into the repository, an editor's autosave during the pre-push hook)
//!   can fail it spuriously, "changed during the walk" (exit 2);
//! - what holds whatever the timing: no symbolic link is followed, and
//!   nothing outside the root is read;
//! - no symbolic link is followed, nor read: one is [`FileKind::Symlink`],
//!   and a directory reached through one is never walked;
//! - only regular files are opened, with `O_NOFOLLOW` and `O_NONBLOCK` (a
//!   FIFO swapped in after the listing cannot block the read), and the
//!   opened file is `fstat`ed to be a regular one; anything else (a FIFO, a
//!   socket, a device) is [`FileKind::Other`];
//! - a hard link is a regular file, and is read even if its other name is
//!   outside the repository: git cannot make one, and it matters only when
//!   `check` runs with more privilege than whoever wrote the tree (which
//!   could read that file itself);
//! - a file is opened only if the site reads its bytes
//!   ([`rubrica_site::reads_bytes`]: a page source, an asset, or a `.md`
//!   file check-packages.cjs harvests, none under `node_modules`, `.git` or
//!   a hidden directory), and then read only up to [`MAX_FILE_BYTES`]: one larger
//!   is [`FileKind::TooLarge`], never read;
//! - a path past [`MAX_PATH_BYTES`] or [`MAX_PATH_SEGMENTS`] is handed to
//!   the site unread (it refuses it first), and a directory past them is
//!   not walked: it is handed over as one path, which the site reports;
//! - a name that is not UTF-8 is a [`ReadProblem::PathNotUtf8`], and
//!   nothing under it is read;
//! - the root's `.git` is not part of the repository and is skipped; a
//!   directory below with a `.git` of its own is a nested checkout, handed
//!   over as a [`FileKind::Submodule`], as git records a submodule.
//!
//! - every entry handed over, and every byte read, is counted against the
//!   repository budget ([`rubrica_site::RepoBudget`]); the walk stops at
//!   the first entry or file over it, so it never holds more than the
//!   budget and one file, and the site reports it
//!   ([`rubrica_site::Finding::RepositoryOverBudget`]).
//!
//! At most one descriptor per directory level with entries still to walk
//! is open (at most [`MAX_PATH_SEGMENTS`] and the one being listed), and a
//! file's only while it is read.
//!
//! It reads the directory as it is, as the wiki's Node checkers do, so a
//! file not yet committed is checked too; the build reads the git tree.

use std::ffi::CStr;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::rc::Rc;

use rubrica_render::frontmatter::MAX_FRONT_MATTER_BYTES;
use rubrica_render::markdown::MAX_PAGE_BYTES;
use rubrica_site::{
    FileKind, RepoBudget, SiteConfig, SourceFile, MAX_PATH_BYTES, MAX_PATH_SEGMENTS,
};
use rustix::fs::{self as rfs, AtFlags, Dir, FileType, Mode, OFlags};
use rustix::io::Errno;

/// The largest file `check` reads: the page cap plus the front-matter cap.
/// A larger file is refused unread, whatever it holds.
pub const MAX_FILE_BYTES: u64 = (MAX_PAGE_BYTES + MAX_FRONT_MATTER_BYTES) as u64;

/// What the reader could not hand to the site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadProblem {
    /// A name that is not UTF-8: neither it nor anything under it is read.
    PathNotUtf8 {
        /// The path as [`lossless_display`] shows it.
        path: String,
    },
}

/// The repository as read.
pub struct Tree {
    /// Every file and special entry, for the site.
    pub files: Vec<SourceFile>,
    /// What could not be handed over.
    pub problems: Vec<ReadProblem>,
}

/// `bytes` with its valid UTF-8 as it is, a backslash doubled, and every
/// other byte as `\xNN`: so the byte 0xfe (`\xfe`) and a name holding the
/// four characters `\xfe` (`\\xfe`) never show alike.
pub fn lossless_display(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if c == '\\' {
                out.push_str("\\\\");
            } else {
                out.push(c);
            }
        }
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    out
}

fn over_limits(rel: &str) -> bool {
    rel.len() > MAX_PATH_BYTES || rel.split('/').count() > MAX_PATH_SEGMENTS
}

/// Whether the site reads a file's bytes ([`rubrica_site::reads_bytes`]).
/// Every other file is a path only, to it, and is never opened.
fn bytes_needed(rel: &str, cfg: &SiteConfig) -> bool {
    rubrica_site::reads_bytes(rel, cfg)
}

fn entry(path: String, kind: FileKind, bytes: Vec<u8>) -> SourceFile {
    SourceFile {
        path,
        bytes,
        blob_id: None,
        kind,
    }
}

/// A directory still to walk: its name in its parent (kept open for it),
/// and its repository path with a trailing `/`.
struct Pending {
    parent: Rc<OwnedFd>,
    name: Vec<u8>,
    prefix: String,
}

const DIR_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::NOCTTY)
    .union(OFlags::CLOEXEC);

/// Reads the repository at `root` (which must be a directory, or a
/// symbolic link to one), for the site under `cfg`.
pub fn read_tree(root: &Path, cfg: &SiteConfig) -> io::Result<Tree> {
    let root_fd = rfs::open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let mut files = Vec::new();
    let mut problems = Vec::new();
    let mut stack: Vec<Pending> = Vec::new();
    let mut budget = RepoBudget::default();
    if !walk_dir(
        Rc::new(root_fd),
        "",
        cfg,
        &mut stack,
        &mut files,
        &mut problems,
        &mut budget,
    )? {
        return Ok(Tree { files, problems });
    }
    while let Some(Pending {
        parent,
        name,
        prefix,
    }) = stack.pop()
    {
        let rel = prefix.trim_end_matches('/');
        #[cfg(test)]
        tests::before_open(rel);
        let fd = open_dir(&parent, &name, rel)?;
        drop(parent);
        // A nested checkout: a `.git` of its own, of any kind.
        match rfs::statat(&fd, ".git", AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => {
                files.push(entry(rel.to_owned(), FileKind::Submodule, Vec::new()));
                if !budget.entry(rel) {
                    break;
                }
                continue;
            }
            Err(Errno::NOENT) => {}
            Err(e) => return Err(at_path(e.into(), &format!("{rel}/.git"))),
        }
        if !walk_dir(
            Rc::new(fd),
            &prefix,
            cfg,
            &mut stack,
            &mut files,
            &mut problems,
            &mut budget,
        )? {
            break;
        }
    }
    Ok(Tree { files, problems })
}

/// Lists the directory open at `dir` (repository path `prefix`, empty for
/// the root or ending in `/`): its files are handed over (and read, where
/// the site reads them), its directories pushed onto `stack`. `false` when
/// the budget ran out (the walk stops there).
fn walk_dir(
    dir: Rc<OwnedFd>,
    prefix: &str,
    cfg: &SiteConfig,
    stack: &mut Vec<Pending>,
    files: &mut Vec<SourceFile>,
    problems: &mut Vec<ReadProblem>,
    budget: &mut RepoBudget,
) -> io::Result<bool> {
    let at = |e: Errno| at_path(e.into(), prefix);
    // A listing taken while the directory changes may miss entries (an
    // exchanged one, on Linux's overlayfs), so a change to its mtime or
    // ctime across the listing fails the walk. Best-effort: with coarse
    // timestamps a change within one tick goes unseen (the module doc).
    let stamp =
        || rfs::fstat(&*dir).map(|s| (s.st_mtime, s.st_mtime_nsec, s.st_ctime, s.st_ctime_nsec));
    let before = stamp().map_err(at)?;
    // Lists from a descriptor of its own (`openat(dir, ".")`): the same
    // directory, never a path.
    for e in Dir::read_from(&*dir).map_err(at)? {
        let e = e.map_err(at)?;
        let raw: &CStr = e.file_name();
        let raw = raw.to_bytes();
        if raw == b"." || raw == b".." {
            continue;
        }
        let Ok(name) = std::str::from_utf8(raw) else {
            let mut bytes = prefix.as_bytes().to_vec();
            bytes.extend_from_slice(raw);
            problems.push(ReadProblem::PathNotUtf8 {
                path: lossless_display(&bytes),
            });
            continue;
        };
        if prefix.is_empty() && name == ".git" {
            continue;
        }
        let rel = format!("{prefix}{name}");
        // Not followed: the entry's own type, or `lstat`'s (relative to the
        // directory's descriptor) where the listing has none.
        let ft = match e.file_type() {
            FileType::Unknown => match rfs::statat(&*dir, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(st) => FileType::from_raw_mode(st.st_mode),
                Err(Errno::NOENT) => return Err(changed(&rel)),
                Err(e) => return Err(at_path(e.into(), &rel)),
            },
            t => t,
        };
        let handed = match ft {
            FileType::Directory => {
                if over_limits(&rel) {
                    entry(rel, FileKind::Other, Vec::new())
                } else {
                    stack.push(Pending {
                        parent: Rc::clone(&dir),
                        name: raw.to_vec(),
                        prefix: format!("{rel}/"),
                    });
                    continue;
                }
            }
            FileType::Symlink => entry(rel, FileKind::Symlink, Vec::new()),
            FileType::RegularFile => read_file(&dir, name, rel, cfg)?,
            _ => entry(rel, FileKind::Other, Vec::new()),
        };
        let within = budget.entry(&handed.path) && budget.read(handed.bytes.len() as u64);
        files.push(handed);
        if !within {
            return Ok(false);
        }
    }
    // Per directory only: a move between two directories, each listed
    // consistently, passes (the module doc).
    if stamp().map_err(at)? != before {
        return Err(changed(prefix.trim_end_matches('/')));
    }
    Ok(true)
}

/// The directory `name` of the one open at `parent` (repository path
/// `rel`), which the listing saw as a directory.
fn open_dir(parent: &OwnedFd, name: &[u8], rel: &str) -> io::Result<OwnedFd> {
    match rfs::openat(parent, name, DIR_FLAGS, Mode::empty()) {
        Ok(fd) => Ok(fd),
        // No longer a directory: swapped for a symbolic link (`ENOTDIR`
        // on Linux and macOS alike, not `ELOOP`), or gone. Refused, not
        // followed, nor guessed at.
        Err(Errno::NOTDIR | Errno::LOOP | Errno::NOENT) => Err(changed(rel)),
        Err(e) => Err(at_path(e.into(), rel)),
    }
}

/// An entry that is no longer what the listing said: the walk fails
/// (exit 2) rather than lose it or guess what it is now.
fn changed(rel: &str) -> io::Error {
    let rel = if rel.is_empty() { "." } else { rel };
    io::Error::other(format!("{rel} changed during the walk"))
}

/// `e`, naming the repository path it happened at (`.` for the root).
fn at_path(e: io::Error, rel: &str) -> io::Error {
    let rel = if rel.is_empty() {
        "."
    } else {
        rel.trim_end_matches('/')
    };
    io::Error::new(e.kind(), format!("{rel}: {e}"))
}

/// Debug builds only (absent from release builds): with
/// `RUBRICA_TRACE_OPENS` set, every file the reader opens (and every blob
/// the git reader reads, [`crate::git`]) is named on stderr
/// (`rubrica: open <path>`, escaped), so a test sees what was opened
/// whatever its permissions (as root, a mode-000 file opens).
pub(crate) fn trace_open(rel: &str) {
    if cfg!(debug_assertions) && std::env::var_os("RUBRICA_TRACE_OPENS").is_some() {
        eprintln!("rubrica: open {}", crate::report::escape(rel));
    }
}

/// The regular file `name` of the directory open at `dir` (repository path
/// `rel`), as the site gets it.
/// A file that takes the bytes read past the budget is still read whole
/// (at most [`MAX_FILE_BYTES`]), so the site sees the budget is over.
fn read_file(dir: &OwnedFd, name: &str, rel: String, cfg: &SiteConfig) -> io::Result<SourceFile> {
    if over_limits(&rel) || !bytes_needed(&rel, cfg) {
        return Ok(entry(rel, FileKind::Regular, Vec::new()));
    }
    trace_open(&rel);
    let fd = match rfs::openat(dir, name, FILE_FLAGS, Mode::empty()) {
        Ok(fd) => fd,
        // A symbolic link swapped in since the listing saw a file, or gone.
        Err(Errno::LOOP | Errno::NOENT) => return Err(changed(&rel)),
        Err(e) => return Err(at_path(e.into(), &rel)),
    };
    let st = rfs::fstat(&fd).map_err(|e| at_path(e.into(), &rel))?;
    if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
        return Err(changed(&rel));
    }
    let len = u64::try_from(st.st_size).unwrap_or(0);
    if len > MAX_FILE_BYTES {
        return Ok(entry(rel, FileKind::TooLarge, Vec::new()));
    }
    let mut bytes = Vec::with_capacity(len.min(MAX_FILE_BYTES) as usize);
    // At most one byte over the cap, should the file have grown.
    File::from(fd)
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| at_path(e, &rel))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Ok(entry(rel, FileKind::TooLarge, Vec::new()));
    }
    Ok(entry(rel, FileKind::Regular, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A hook `read_tree` runs before each directory's open.
    type Hook = Box<dyn FnMut(&str)>;

    thread_local! {
        /// Run by `read_tree` just before it opens each directory it listed,
        /// with that directory's repository path: a test changes the tree
        /// there, between the listing and the open, deterministically.
        static BEFORE_OPEN: RefCell<Option<Hook>> = RefCell::new(None);
    }

    pub(super) fn before_open(rel: &str) {
        BEFORE_OPEN.with(|h| {
            if let Some(f) = h.borrow_mut().as_mut() {
                f(rel);
            }
        });
    }

    /// `read_tree(root)` with `hook` run before each directory's open.
    fn read_tree_with(root: &Path, hook: impl FnMut(&str) + 'static) -> io::Result<Tree> {
        BEFORE_OPEN.with(|h| *h.borrow_mut() = Some(Box::new(hook)));
        let r = read_tree(root, &SiteConfig::default());
        BEFORE_OPEN.with(|h| *h.borrow_mut() = None);
        r
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("rubrica-read-{tag}-{}", std::process::id()));
        if base.exists() {
            std::fs::remove_dir_all(&base).unwrap();
        }
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    /// The race test's exchange, made once, at the
    /// one moment that matters: after the root's listing saw `dd` as a
    /// directory and `ddlink` as a link, and just before `dd` is opened.
    /// `read_tree` must fail naming `dd`, not reclassify it (as a link or
    /// `Other`) and lose its pages with exit 0, whatever `open_dir` does
    /// when called on its own. It holds at any timestamp granularity (PR
    /// 5e): the exchange comes after the root's listing and its check, and
    /// what fails the walk is `open_dir`'s `ENOTDIR`, not a stamp.
    #[test]
    fn a_directory_exchanged_for_a_link_just_before_its_open_fails_the_walk() {
        use rustix::fs::{renameat_with, RenameFlags, CWD};
        use std::fs;
        let base = scratch("exchange");
        let repo = base.join("repo");
        fs::create_dir_all(repo.join("dd")).unwrap();
        fs::create_dir_all(base.join("secret")).unwrap();
        fs::write(repo.join("home.md"), "---\ntitle: Home\n---\n").unwrap();
        fs::write(repo.join("dd/p.md"), "---\ntitle: P\n---\npublic\n").unwrap();
        fs::write(base.join("secret/p.md"), "---\ntitle: S\n---\nSECRET\n").unwrap();
        std::os::unix::fs::symlink(base.join("secret"), repo.join("ddlink")).unwrap();
        let (a, b) = (repo.join("dd"), repo.join("ddlink"));
        let mut swapped = false;
        let r = read_tree_with(&repo, move |rel| {
            if rel == "dd" && !swapped {
                renameat_with(CWD, &a, CWD, &b, RenameFlags::EXCHANGE).expect("exchange");
                swapped = true;
            }
        });
        let e = r.err().map(|e| e.to_string());
        fs::remove_dir_all(&base).ok();
        assert_eq!(e.as_deref(), Some("dd changed during the walk"));
    }

    /// A **known limit**, pinned as today's behaviour:
    /// the listing check is per directory, not a snapshot of the tree. A
    /// page moved out of a directory not yet walked into one already walked
    /// (each listing itself consistent) is in neither listing, and the walk
    /// succeeds without it (exit 0). The build reads the git tree, which
    /// is a snapshot. Should this ever fail the walk instead, change this
    /// test and the docs that describe the limit. It holds at any timestamp
    /// granularity: each move lands before the listing of the
    /// directory it changes or after that listing's check, never during
    /// one, so no stamp it moves is compared.
    #[test]
    fn a_page_moved_between_directories_mid_walk_is_lost_a_known_limit() {
        use std::fs;
        let base = scratch("move");
        let repo = base.join("repo");
        for d in ["a", "b"] {
            fs::create_dir_all(repo.join(d)).unwrap();
            fs::write(repo.join(format!("{d}/{d}.md")), "---\ntitle: X\n---\n").unwrap();
        }
        fs::write(repo.join("home.md"), "---\ntitle: Home\n---\n").unwrap();
        fs::write(repo.join("a/moved.md"), "---\ntitle: M\n---\n").unwrap();
        // Whichever of `a` and `b` is opened first (the listing's order),
        // `moved.md` is in the other one while the first is listed, and is
        // moved into the first just before the second is opened: neither
        // listing holds it.
        let other = |d: &str| if d == "a" { "b" } else { "a" };
        let mut first: Option<String> = None;
        let root = repo.clone();
        let r = read_tree_with(&repo, move |rel| match &first {
            None => {
                if rel == "a" {
                    fs::rename(root.join("a/moved.md"), root.join("b/moved.md")).unwrap();
                }
                first = Some(rel.to_owned());
            }
            Some(f) => {
                assert_eq!(rel, other(f));
                fs::rename(
                    root.join(format!("{rel}/moved.md")),
                    root.join(format!("{f}/moved.md")),
                )
                .unwrap();
            }
        });
        let moved_exists = repo.join("a/moved.md").exists() || repo.join("b/moved.md").exists();
        fs::remove_dir_all(&base).ok();
        assert!(moved_exists);
        let t = r.expect("today the walk succeeds");
        let paths: Vec<&str> = t.files.iter().map(|f| f.path.as_str()).collect();
        assert!(
            paths.contains(&"a/a.md") && paths.contains(&"b/b.md"),
            "{paths:?}"
        );
        assert!(!paths.iter().any(|p| p.ends_with("moved.md")), "{paths:?}");
    }

    #[test]
    fn a_name_that_is_not_utf8_is_shown_byte_for_byte() {
        assert_eq!(lossless_display(b"bad\xffname.md"), "bad\\xffname.md");
        assert_eq!(
            lossless_display(b"d\xfe/\xc3\xa9\xc3"),
            "d\\xfe/\u{e9}\\xc3"
        );
        assert_eq!(lossless_display("é/x".as_bytes()), "é/x");
        // A backslash is doubled, so a name holding the four characters
        // `\xfe` is never shown as the one byte 0xfe is.
        assert_eq!(lossless_display(b"lit\\xfe"), "lit\\\\xfe");
        assert_ne!(lossless_display(b"lit\\xfe"), lossless_display(b"lit\xfe"));
    }

    #[test]
    fn only_what_the_site_or_check_packages_uses_is_read() {
        let cfg = SiteConfig::default();
        let bytes_needed = |rel: &str| bytes_needed(rel, &cfg);
        assert!(bytes_needed("a/b.md"));
        assert!(bytes_needed("README.md"));
        assert!(bytes_needed("scripts/notes.md"));
        assert!(bytes_needed("assets/x.png"));
        assert!(bytes_needed("assets/x.md"));
        assert!(!bytes_needed("Assets/x.png"));
        assert!(!bytes_needed("scripts/node_modules/x.js"));
        // check-packages.cjs skips node_modules, .git and hidden directories, and the
        // site ignores them: nothing there is opened.
        assert!(!bytes_needed("scripts/node_modules/x/README.md"));
        assert!(!bytes_needed("node_modules/x.md"));
        assert!(!bytes_needed(".github/x.md"));
        assert!(!bytes_needed("a/.git/x.md"));
        assert!(!bytes_needed("packages.yaml"));
    }

    #[test]
    fn the_cap_is_the_page_cap_and_the_front_matter_cap() {
        assert_eq!(MAX_FILE_BYTES, 4 * 1024 * 1024 + 64 * 1024);
    }

    /// What the listing saw as a directory or a file, and is now a link, a
    /// file, a FIFO or gone, fails the read
    /// naming it; never reclassified, so its pages never vanish unreported.
    /// No listing and no stamp is involved: `open_dir` and `read_file` are
    /// called directly.
    #[test]
    fn an_entry_that_is_no_longer_what_was_listed_fails_the_read() {
        use std::fs;
        let base =
            std::env::temp_dir().join(format!("rubrica-read-changed-{}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        fs::create_dir_all(base.join("real")).unwrap();
        fs::write(base.join("file.md"), "x").unwrap();
        std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();
        std::os::unix::fs::symlink(base.join("file.md"), base.join("link.md")).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(base.join("fifo.md"))
            .status()
            .unwrap()
            .success());
        let root = rfs::open(&base, DIR_FLAGS, Mode::empty()).unwrap();
        let msg = |r: io::Result<()>| r.unwrap_err().to_string();
        let dir = |n: &str| open_dir(&root, n.as_bytes(), n).map(drop);
        let cfg = SiteConfig::default();
        let file = |n: &str| read_file(&root, n, n.to_owned(), &cfg).map(drop);
        assert!(dir("real").is_ok());
        for n in ["link", "file.md", "gone"] {
            assert_eq!(msg(dir(n)), format!("{n} changed during the walk"));
        }
        assert!(file("file.md").is_ok());
        for n in ["link.md", "fifo.md", "gone.md"] {
            assert_eq!(msg(file(n)), format!("{n} changed during the walk"));
        }
        fs::remove_dir_all(&base).ok();
    }

    /// A directory's ctime (seconds, nanoseconds), in the unit test.
    type Stamp = (i64, i64);

    /// The probe stops at this many changes, or at [`PROBE_BUDGET`].
    const PROBE_MAX_CHANGES: u64 = 2_000;
    /// The probe's time budget.
    const PROBE_BUDGET: std::time::Duration = std::time::Duration::from_millis(50);
    /// Fewer changes than this within the budget decide nothing: "coarse".
    const PROBE_MIN_CHANGES: u64 = 100;
    /// "Fine" allows at most one unseen change in this many.
    const PROBE_UNSEEN_ONE_IN: u64 = 1_000;

    /// Whether a file system's directory timestamps tell apart consecutive
    /// changes, as the walk's best-effort listing check needs them to
    /// (the module doc): the race test asserts that no walk loses the
    /// swapped directory only where they do. A CI runner
    /// on Debian 13 (kernel 6.12, jobs in containers on overlayfs)
    /// predates Linux 6.13's multigrain timestamps: its stamps are
    /// coarse, and a racer was seen to get past the check in about
    /// 97% of walks there.
    ///
    /// The probe does what the walk does to a directory and what the
    /// race does to it: `fstat`, then `RENAME_EXCHANGE` of two of its
    /// entries, then `fstat` again, over and over, for up to
    /// [`PROBE_BUDGET`] or [`PROBE_MAX_CHANGES`]. A change "unseen" left
    /// the ctime as it was.
    ///
    /// **Fine** needs at least [`PROBE_MIN_CHANGES`] changes and at most
    /// one unseen in [`PROBE_UNSEEN_ONE_IN`]. Why there:
    /// - with multigrain timestamps (Linux 6.13 and later: ext4, xfs,
    ///   btrfs, tmpfs, and overlayfs over them) a change after a query of
    ///   the ctime (the `fstat`) is always given a new, later stamp, and
    ///   APFS stamps in nanoseconds from a fine clock: none is unseen, so
    ///   the allowance is only for a coincidence;
    /// - coarse stamps move once per tick (Debian's kernels: `HZ=250`, 4
    ///   ms; at the finest, 1 ms). Every change but the last is made
    ///   within the 50 ms budget, so on a 1 ms tick at most 52 can be seen,
    ///   however fast or loaded the machine; "fine" needs at least 100,
    ///   all but one in 1,000 of them seen. So a coarse file system cannot
    ///   pass, by counting, not by chance. (APFS made 300 to 770 changes
    ///   in 50 ms here, one every 65 to 165 µs, so 100 leaves it room on a
    ///   busy machine; Linux 7.2's overlayfs and tmpfs made 2,000 in 3 to
    ///   12 ms.)
    /// - the errors are not alike. "Fine" on a coarse file system would
    ///   fail CI on most runs; "coarse" on a fine one only skips the one
    ///   assertion that needs the listing check (and says so), while the
    ///   security property is asserted everywhere. So a doubt (too few
    ///   changes in the budget) answers "coarse".
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct StampProbe {
        /// Changes made, each between two `fstat`s.
        changes: u64,
        /// Of them, those that left the ctime as it was.
        unseen: u64,
        /// How long they took.
        elapsed: std::time::Duration,
    }

    impl StampProbe {
        /// The probe's verdict from the ctimes it read: `stamps[0]` before
        /// the first change, `stamps[i]` after change `i`. A stamp that
        /// moves backwards (a clock set back) moved: the walk sees it.
        fn from_stamps<T: PartialEq>(stamps: &[T], elapsed: std::time::Duration) -> Self {
            let changes = stamps.len().saturating_sub(1) as u64;
            let unseen = stamps.windows(2).filter(|w| w[0] == w[1]).count() as u64;
            StampProbe {
                changes,
                unseen,
                elapsed,
            }
        }

        fn fine(&self) -> bool {
            self.changes >= PROBE_MIN_CHANGES
                && self.unseen.saturating_mul(PROBE_UNSEEN_ONE_IN) <= self.changes
        }
    }

    impl std::fmt::Display for StampProbe {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}: {} of {} directory changes in {:?} left its ctime unchanged",
                if self.fine() { "fine" } else { "coarse" },
                self.unseen,
                self.changes,
                self.elapsed
            )
        }
    }

    /// Probes the directory timestamps of the file system `dir` is on,
    /// in a directory of its own made (and removed) under it.
    fn probe_stamps(dir: &Path) -> StampProbe {
        use rustix::fs::{renameat_with, RenameFlags};
        use std::time::Instant;
        let probe = dir.join("stamp-probe");
        std::fs::create_dir_all(probe.join("d")).unwrap();
        std::os::unix::fs::symlink("d", probe.join("l")).unwrap();
        let fd = rfs::open(&probe, DIR_FLAGS, Mode::empty()).unwrap();
        // As the walk reads it (its `stamp`, less the mtime, which moves
        // with the ctime).
        let ctime = || {
            rfs::fstat(&fd)
                .map(|s| (s.st_ctime, s.st_ctime_nsec))
                .unwrap()
        };
        let mut stamps = vec![ctime()];
        let start = Instant::now();
        while (stamps.len() as u64) <= PROBE_MAX_CHANGES && start.elapsed() < PROBE_BUDGET {
            renameat_with(&fd, "d", &fd, "l", RenameFlags::EXCHANGE).expect("exchange");
            stamps.push(ctime());
        }
        let elapsed = start.elapsed();
        drop(fd);
        std::fs::remove_dir_all(&probe).unwrap();
        StampProbe::from_stamps(&stamps, elapsed)
    }

    /// The probe's verdict on stamp sequences no local kernel gives (PR
    /// 5e): coarse clocks at the ticks Linux kernels use, against fine
    /// ones. A sequence models `n` changes `op_ns` apart on a clock that
    /// moves in steps of `tick_ns`.
    #[test]
    fn the_stamp_probe_tells_coarse_timestamps_from_fine() {
        let ms = std::time::Duration::from_millis;
        let clock = |tick_ns: i64, op_ns: i64, n: i64| -> Vec<Stamp> {
            (0..=n)
                .map(|i| {
                    let t = 1_700_000_000_000_000_000 + i * op_ns;
                    let t = t - t.rem_euclid(tick_ns);
                    (t / 1_000_000_000, t % 1_000_000_000)
                })
                .collect()
        };
        let probe = |s: &[Stamp]| StampProbe::from_stamps(s, ms(50));
        // Fine: multigrain (every change after a query is a new stamp),
        // and APFS's nanoseconds, at any pace.
        for op_ns in [700, 10_000, 25_000, 240_000] {
            let p = probe(&clock(1, op_ns, 2_000));
            assert_eq!((p.unseen, p.fine()), (0, true), "{op_ns} ns a change: {p}");
        }
        // Coarse: Debian's 4 ms tick (HZ=250, the CI runner's), 1 ms
        // (HZ=1000), 10 ms (HZ=100), and one second, at a fast pace, a
        // slow one, and one just under a tick (a loaded machine).
        for tick_ms in [1, 4, 10, 1_000] {
            let tick = tick_ms * 1_000_000;
            for op_ns in [5_000, 25_000, 250_000, tick * 9 / 10] {
                let n = (50_000_000 / op_ns).clamp(1, 2_000);
                let p = probe(&clock(tick, op_ns, n));
                assert!(
                    !p.fine(),
                    "{tick_ms} ms tick, {op_ns} ns a change, {n}: {p}"
                );
            }
        }
        // Past the budget's bound, on the ratio alone: a 1 ms tick with
        // changes 0.9 ms apart (2,000 of them, 1.8 s: more than the
        // budget allows) still leaves about 200 unseen.
        let p = probe(&clock(1_000_000, 900_000, 2_000));
        assert!(p.unseen > 100 && !p.fine(), "{p}");
        // At the threshold: one or two unseen in 2,000 is fine, three
        // (more than one in 1,000) is not.
        let mut s = clock(1, 1_000, 2_000);
        s[1] = s[0];
        assert!(probe(&s).fine(), "one unseen in 2,000: {}", probe(&s));
        s[2] = s[1];
        s[3] = s[2];
        assert!(!probe(&s).fine(), "three unseen in 2,000: {}", probe(&s));
        // Too few changes to tell decides nothing: coarse.
        let p = probe(&clock(1, 1_000, PROBE_MIN_CHANGES as i64 - 1));
        assert_eq!((p.unseen, p.fine()), (0, false), "{p}");
        assert!(probe(&clock(1, 1_000, PROBE_MIN_CHANGES as i64)).fine());
        assert!(!probe(&[]).fine());
        assert!(!probe(&[(1, 0)]).fine());
        // A stamp set back is a change the walk sees.
        let mut s = clock(1, 1_000, 300);
        s[150] = (0, 0);
        assert_eq!(probe(&s).unseen, 0);
        // Its report names the verdict and the counts.
        let p = probe(&clock(4_000_000, 10_000, 2_000));
        assert!(
            p.to_string()
                == "coarse: 1995 of 2000 directory changes in 50ms left its ctime unchanged",
            "{p}"
        );
    }

    /// The race of a directory of the
    /// repository exchanged, again and again, with a symbolic link to a
    /// directory outside it, while the reader walks. A walk that opens by
    /// path follows the link whenever the exchange lands between listing a
    /// directory and opening what is in it (726 leaks in 3,000 runs); a walk
    /// by directory file descriptors, each opened `O_NOFOLLOW`, never does.
    /// Nor does a walk lose the directory: one
    /// listed as a directory that cannot be opened as one any more fails the
    /// read, so every walk either lists the swapped directory's pages, under
    /// whichever name it had, or is an error (exit 2).
    ///
    /// Two assertions: (a) no walk follows the link or reads past
    /// it, asserted everywhere, since it holds whatever the timing; (b) no
    /// walk exits 0 without the swapped directory, asserted only where
    /// [`probe_stamps`] finds fine-grained directory timestamps on the
    /// test's own file system, since (b) rests on the best-effort listing
    /// check. Elsewhere (the CI runner's kernel 6.12, say) (b) is skipped,
    /// saying so with the probe's numbers, and its losses are still counted
    /// and reported.
    ///
    /// (a) must not rest on the listing check either. Where that check sees
    /// every exchange, an exchange during the root's listing fails the walk
    /// there, and the window (a) is about, between the root's listing and
    /// `dd`'s open, is a few system calls wide: with `O_NOFOLLOW` dropped
    /// from `DIR_FLAGS`, 2,000 walks never followed the link, on macOS
    /// (where most walks got to the open) nor on Linux 7.2 (where the
    /// racer is so fast that almost every walk failed at the root). So
    /// every other walk is **widened**: the racer is held while the
    /// root is listed, and released, in the `BEFORE_OPEN` hook, just
    /// before `dd` (or `ddlink`) is opened; the hook waits for its next
    /// exchange. The listing is then consistent and the name opened has
    /// been exchanged since, an odd number of times (a link) about half
    /// the time, on any file system and at any timestamp granularity.
    /// `RUBRICA_RACE_ITERATIONS` sets the number of walks (default 2,000).
    #[test]
    fn a_directory_exchanged_with_a_symlink_mid_walk_is_never_followed() {
        use rustix::fs::{renameat_with, RenameFlags, CWD};
        use std::fs;
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::Arc;
        let base = std::env::temp_dir().join(format!("rubrica-read-race-{}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).unwrap();
        }
        let repo = base.join("repo");
        let secret = base.join("secret");
        fs::create_dir_all(repo.join("dd/sub")).unwrap();
        fs::create_dir_all(secret.join("sub")).unwrap();
        fs::write(repo.join("home.md"), "---\ntitle: Home\n---\n").unwrap();
        fs::write(repo.join("dd/p.md"), "---\ntitle: P\n---\npublic\n").unwrap();
        fs::write(repo.join("dd/sub/q.md"), "---\ntitle: Q\n---\npublic\n").unwrap();
        // The same names as the public side, so an open by path that lands
        // on the far side of an exchange finds a file there too.
        for f in ["p.md", "sub/q.md", "s.md"] {
            fs::write(secret.join(f), "---\ntitle: SECRET\n---\nSECRET-CONTENT\n").unwrap();
        }
        std::os::unix::fs::symlink(&secret, repo.join("ddlink")).unwrap();
        // Before the racer starts, so nothing else loads the machine.
        let stamps = probe_stamps(&base);
        let (a, b) = (repo.join("dd"), repo.join("ddlink"));
        let stop = Arc::new(AtomicBool::new(false));
        let swaps = Arc::new(AtomicU64::new(0));
        // `hold` asks the racer to stop between exchanges; `held` says it has.
        let (hold, held) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
        );
        let racer = {
            let (stop, swaps) = (stop.clone(), swaps.clone());
            let (hold, held) = (hold.clone(), held.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    if hold.load(Ordering::SeqCst) {
                        held.store(true, Ordering::SeqCst);
                        std::thread::yield_now();
                        continue;
                    }
                    held.store(false, Ordering::SeqCst);
                    renameat_with(CWD, &a, CWD, &b, RenameFlags::EXCHANGE).expect("exchange");
                    swaps.fetch_add(1, Ordering::SeqCst);
                }
            })
        };
        let runs: u64 = std::env::var("RUBRICA_RACE_ITERATIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2_000);
        let cfg = SiteConfig::default();
        // Until `f` holds, 10 s at most (a stuck racer fails the test).
        fn until(f: impl Fn() -> bool) {
            let start = std::time::Instant::now();
            while !f() {
                assert!(
                    start.elapsed() < std::time::Duration::from_secs(10),
                    "the racer is stuck"
                );
                std::thread::yield_now();
            }
        }
        // A widened walk: the racer held for the root's listing, released
        // just before the swapped directory's open, which waits for its
        // next exchange.
        let widened = || {
            // `held` is set only where the racer makes no exchange before
            // it looks at `hold` again, so a stale `true` cannot pass.
            held.store(false, Ordering::SeqCst);
            hold.store(true, Ordering::SeqCst);
            until(|| held.load(Ordering::SeqCst));
            let (release, swapped) = (hold.clone(), swaps.clone());
            let mut released = false;
            let r = read_tree_with(&repo, move |rel| {
                if !released && (rel == "dd" || rel == "ddlink") {
                    released = true;
                    let seen = swapped.load(Ordering::SeqCst);
                    release.store(false, Ordering::SeqCst);
                    until(|| swapped.load(Ordering::SeqCst) != seen);
                }
            });
            hold.store(false, Ordering::SeqCst);
            r
        };
        let (mut leaks, mut followed, mut lost) = (0u64, 0u64, 0u64);
        // Walks that failed, plain and widened.
        let mut errors = [0u64; 2];
        for i in 0..runs {
            let walk = if i % 2 == 1 {
                widened()
            } else {
                read_tree(&repo, &cfg)
            };
            match walk {
                Ok(t) => {
                    // The public pages, under `dd/` or `ddlink/`: the walk
                    // reported the swapped directory, not nothing.
                    let has = |tail: &str| {
                        t.files.iter().any(|f| {
                            (f.path == format!("dd/{tail}") || f.path == format!("ddlink/{tail}"))
                                && f.bytes.windows(6).any(|w| w == b"public")
                        })
                    };
                    if !(has("p.md") && has("sub/q.md")) {
                        lost += 1;
                    }
                    if t.files
                        .iter()
                        .any(|f| f.bytes.windows(14).any(|w| w == b"SECRET-CONTENT"))
                    {
                        leaks += 1;
                    }
                    if t.files.iter().any(|f| f.path.ends_with("s.md")) {
                        followed += 1;
                    }
                }
                Err(e) => {
                    // Named: the swapped entry, as having changed.
                    let e = e.to_string();
                    assert!(
                        [
                            ". changed during the walk",
                            "dd changed during the walk",
                            "ddlink changed during the walk"
                        ]
                        .contains(&e.as_str()),
                        "{e}"
                    );
                    errors[(i % 2) as usize] += 1;
                }
            }
        }
        stop.store(true, Ordering::SeqCst);
        racer.join().unwrap();
        fs::remove_dir_all(&base).ok();
        eprintln!(
            "read race: {runs} walks (every other one widened), {} exchanges: {leaks} leaked content, {followed} listed the far side, {lost} lost the directory, {} and {} I/O errors (plain and widened); timestamps {stamps}",
            swaps.load(Ordering::SeqCst),
            errors[0],
            errors[1]
        );
        // (a), everywhere.
        assert_eq!(
            (leaks, followed),
            (0, 0),
            "the walk followed the exchanged link"
        );
        // And the widened walks met the link (exit 2, or (a) failed above):
        // otherwise (a) was not tested.
        if runs >= 200 {
            assert!(
                errors[1] > 0,
                "no widened walk met an exchange: (a) may have gone untested"
            );
        }
        // (b), where the listing check can see every exchange.
        if stamps.fine() {
            assert_eq!(
                lost, 0,
                "a walk exited 0 without the swapped directory (timestamps {stamps})"
            );
        } else {
            eprintln!(
                "read race: NOT ASSERTED that no walk exits 0 without the swapped directory: this file system's directory timestamps are {stamps}, so the best-effort listing check can miss an exchange ({lost} of {runs} walks lost it); asserted: no walk followed the link"
            );
        }
    }
}
