//! Reading the wiki's git tree at a commit, with each entry's mode, and its
//! history, read-only (gix, opened isolated: no system, global or
//! environment configuration, nothing run, nothing written).
//!
//! The tree is a snapshot: what the commit records, not the worktree, so no
//! uncommitted or untracked file is built and nothing can change under the
//! read (the directory walk's races do not apply). Each
//! entry is what its exact mode says:
//! - a blob (`100644`, `100755`, the legacy `100664`) is a
//!   [`FileKind::Regular`] file, read only if the site uses its bytes
//!   ([`rubrica_site::reads_bytes`]) and its size, from the object's
//!   header, is within [`MAX_FILE_BYTES`] (else [`FileKind::TooLarge`],
//!   unread); its mode is not kept;
//! - a link (`120000`) is a [`FileKind::Symlink`] and a gitlink (`160000`) a
//!   [`FileKind::Submodule`]: neither is read, nor what it points at; any
//!   other mode (`123456`, whose type bits gix reads as a link's) is
//!   [`FileKind::UnknownMode`];
//! - a tree (`040000`) is walked (iteratively), unless its path is over
//!   the limits: then it is handed over as one path, as the directory
//!   reader does;
//! - a name that is not UTF-8 is a [`ReadProblem::PathNotUtf8`], and
//!   nothing under it is read.
//!
//! **One object budget** (positive exclusion). Every git object the
//! build inflates, a commit, a tree, a blob or a tag, in any reader (the
//! tree built and the way to `--subtree`, the history's graph and its
//! diffs, the feed's versions, the pages' sources, `--rev`'s tags), is
//! read by one function ([`objects::Objects::read`]), which takes its size
//! from the object's header and charges it before inflating it: against a
//! cap for its kind (a commit or a tag [`MAX_COMMIT_BYTES`], a tree
//! [`MAX_REPO_TREE_BYTES`], a blob [`MAX_FILE_BYTES`]) and against one
//! cumulative budget, [`MAX_OBJECT_BYTES`]. Nothing else in this file can
//! reach the repository: the module that holds it is the only one that
//! names gix's readers, and it inflates in one place (a test scans for
//! it). When an object does not fit, it is not read: the tree built then
//! refuses the build ([`rubrica_site::Finding::RepositoryOverBudget`]),
//! and the history stops there, with a warning. The bytes the pages'
//! sources will need again, at the end, are set aside when the tree has
//! been read, so the history cannot spend them.
//!
//! **What the budget bounds, and what it does not**. It charges
//! each object's own size, so the bytes charged total at most
//! [`MAX_OBJECT_BYTES`]; it is not a bound on the bytes inflated. A packed
//! object is often a delta: rebuilding it inflates its chain's base and
//! every delta on the way, none of them charged (: a
//! 1 MB pack of 300 tiny pages, each a delta against one 1 GiB blob, was
//! charged 47 KB and took 34 s and 2.1 GB). What bounds that:
//! - each object's reconstruction allocates at most [`MAX_ALLOC_BYTES`]
//!   (gix's `gitoxide.objects.allocLimit`, set on the open: gix refuses,
//!   before allocating, a buffer over it); an object that cannot be
//!   rebuilt within it is refused by name ([`Budget::DeltaBase`]) in the
//!   tree built, and stops the history, with a warning;
//! - a chain is at most [`MAX_DELTA_DEPTH`] deltas (git's own maximum; gix
//!   has none), checked from the header before the charge
//!   ([`Budget::DeltaDepth`]);
//! - so the work of one read is bounded by its chain's depth times the
//!   limit (each delta applied writes at most half the limit), and gix's
//!   caches amortise repeats: [`OBJECT_CACHE_BYTES`] of whole objects by
//!   id (a page's second read), [`PACK_CACHE_BYTES`] of rebuilt objects by
//!   their place in the pack (a chain's next object, usually the next
//!   older version, starts from the one before it). A base that is never
//!   read itself (a 30 MB blob a page is a delta against) is inflated
//!   again for each object built on it: the caches keep what was built,
//!   not the bases it was built from.
//!
//! Charging the header before inflating walks a chain twice: once for the
//! header (its size and depth), once to rebuild it (
//! measured about 1.6 times the work on chains 639 deep): inherent in
//! charging before inflating, and kept.
//!
//! **The threat model.** A crafted pack needs write access to the wiki's
//! git repository, which only its owner and their agents have; the
//! editor's saves go through the forge's API, which writes the
//! objects itself. So a denial of service by pack (a delta chain built to
//! be slow within the limits) is out of scope until untrusted git pushes
//! are possible; then charging each delta chain from gix-pack's headers
//! before rebuilding it is to be revisited.
//!
//! What the other budgets add: the tree built still counts every entry it
//! reads (a directory, a name that is not UTF-8, the root's entries on the
//! way to `--subtree`) against [`rubrica_site::MAX_REPO_FILES`] and
//! [`rubrica_site::MAX_REPO_PATH_BYTES`], and the bytes it hands over
//! against [`rubrica_site::MAX_REPO_BYTES`]: those bound what the site is
//! made from. Every tree read is reached by an entry counted, so they bound
//! the trees read too (a tree reached by two paths is read, and counted,
//! twice). The history keeps [`MAX_HISTORY_COMMITS`], which bounds the
//! commits it holds, and the feed's [`MAX_FEED_VERSIONS`] and
//! [`MAX_FEED_BYTES`], which bound the versions it holds.
//!
//! **The history** is git's for each question asked of it, within the
//! commits it walks:
//! - the walk: the commits behind the one built, at most
//!   [`MAX_HISTORY_COMMITS`], each read once, newest first by committer
//!   time, then put in git's `--date-order` (no commit before all its
//!   children, otherwise newest first, ties by id); a shallow clone's
//!   boundary commits are walked as roots, and a commit that cannot be
//!   read, or whose time cannot, ends the walk there;
//! - a page's last update is `git log -1 -- <page>`'s commit: from the
//!   commit built, the page follows the first parent whose entry for it is
//!   the same (git's TREESAME, modes compared as git compares them:
//!   `100664` is `100644`), and the commit with no such parent (a root:
//!   one that has the page) is the date;
//! - recent changes are the first [`RECENT_COMMITS`] of `git log -- <every
//!   page>`'s commits (the same rule for all the pages together) the walk
//!   reaches, each with the pages it changed against every parent (a merge
//!   that changed none against every parent is left out), listed newest
//!   first by committer time, ties by the greater id;
//! - the feed's versions are `git log -- <feed>`'s commits that have it.
//!
//! A commit is compared with each of its parents once, however often it
//! names one. Only the paths asked about are compared, all from the root
//! (under `--subtree`, with its path), and a tree is entered only when its
//! id differs and one of those paths is under it, so a commit costs what
//! it changed there. Of each tree the history reads it keeps only the
//! entries of those paths, by the tree's id ([`TREE_CACHE_BYTES`] of them,
//! the least recently used dropped first), so each tree is inflated once
//! in a walk. Where an answer needs a commit the walk does not have (a
//! shallow boundary's parents, a commit past a bound, over the budget or
//! that could not be read), there is none (no date, no listing), and the
//! history says it is not complete, and why ([`Walked`]): the build goes
//! on, with a warning.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use gix::objs::{CommitRefIter, TagRefIter, TreeRefIter};
use gix::ObjectId;
use rubrica_site::build::{Commit, History, RecentCommit, RECENT_COMMITS, RECENT_PAGES_PER_COMMIT};
use rubrica_site::{
    Budget, FileKind, RepoBudget, SiteConfig, SourceFile, MAX_PATH_BYTES, MAX_PATH_SEGMENTS,
    MAX_REPO_TREE_BYTES,
};

use crate::read::{lossless_display, ReadProblem, Tree, MAX_FILE_BYTES};
use objects::{Draw, Kind, Objects, Refused};

/// The most commits the history walk reads.
pub const MAX_HISTORY_COMMITS: usize = 100_000;

/// The most bytes of git objects one build reads, summed over every
/// object of every reader, each charged by its header before it is
/// inflated: 4 GiB. A 66-page wiki's build read 1.9 MB (353
/// commits, 571 trees and 133 blobs). A large wiki of
/// 10,000 pages in one directory with 5,000 commits reads 1.85 GB (each
/// commit's root, of 370 KB, once), so it is dated completely with room
/// to spare; in directories of 100 pages, 55 MB. It bounds the objects'
/// own sizes, charged: not the bases and deltas inflated to rebuild a
/// packed one, which [`MAX_ALLOC_BYTES`] and [`MAX_DELTA_DEPTH`] bound for
/// each read (the module doc).
pub const MAX_OBJECT_BYTES: u64 = 4 << 30;

/// The largest commit or tag object read: 1 MiB. A commit is a few
/// hundred bytes, a merge of many parents some kilobytes; one with a
/// message of a gigabyte is never inflated.
pub const MAX_COMMIT_BYTES: u64 = 1 << 20;

/// The most bytes of the history's kept tree entries (by tree id; the
/// least recently used are dropped first, and read again if needed).
pub const TREE_CACHE_BYTES: usize = 32 << 20;

/// The most versions of the feed's source the history keeps.
pub const MAX_FEED_VERSIONS: usize = 256;

/// The most bytes of those versions it keeps.
pub const MAX_FEED_BYTES: u64 = 64 << 20;

/// The longest commit summary kept, in characters.
const MAX_SUMMARY_CHARS: usize = 300;

/// The most annotated tags `--rev` peels through to its commit.
const MAX_TAG_DEPTH: usize = 16;

/// The most bytes gix allocates to rebuild any one object
/// (`gitoxide.objects.allocLimit`, set on the isolated open): 64 MiB. gix
/// rebuilds a delta in one buffer of twice the largest object along its
/// chain, base included, and the chain's delta data, and refuses, before
/// allocating, a buffer over the limit. The largest object read is a tree
/// of [`MAX_REPO_TREE_BYTES`] (23.8 MB; a page is at most [`MAX_FILE_BYTES`],
/// 4.06 MB, a commit [`MAX_COMMIT_BYTES`]): one at its cap, against a base
/// no larger, with the largest delta git writes (half its target), needs
/// 2 × 23.8 + 11.9 = 59.4 MB. So any object within its cap rebuilds from
/// a chain whose objects are at most about 32 MB; a larger one (a page
/// delta-compressed against a 100 MB blob in the history) is refused, by
/// name ([`Budget::DeltaBase`]).
pub const MAX_ALLOC_BYTES: usize = 64 << 20;

// That arithmetic, checked as this compiles: twice a tree at its cap and
// half of it again (a page and a commit are smaller).
const _: () = assert!(
    2 * MAX_REPO_TREE_BYTES + MAX_REPO_TREE_BYTES / 2 <= MAX_ALLOC_BYTES as u64
        && MAX_FILE_BYTES < MAX_REPO_TREE_BYTES
        && MAX_COMMIT_BYTES < MAX_REPO_TREE_BYTES
);

/// The deepest delta chain read: 4095, the deepest git writes (it clamps
/// `--depth` there). gix sets no depth of its own; a chain deeper than
/// this is refused from its header ([`Budget::DeltaDepth`]), unread.
pub const MAX_DELTA_DEPTH: u32 = 4095;

/// gix's object cache: whole objects by id, the least recently used
/// dropped first. A page's source is read twice (the tree built, then its
/// `.md` copy), and the second read is this cache's.
pub const OBJECT_CACHE_BYTES: usize = 32 << 20;

/// gix's pack cache: each object it rebuilt from a delta, by its place in
/// the pack, the least recently used dropped first. A chain's next object
/// (an older version of a tree or a page, usually) then starts from it,
/// not from the chain's base.
pub const PACK_CACHE_BYTES: usize = 32 << 20;

/// The history walk's bounds ([`Limits::DEFAULT`]; tests use smaller).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most commits read.
    pub commits: usize,
    /// The most versions of the feed's source kept.
    pub feed_versions: usize,
    /// The most bytes of them.
    pub feed_bytes: u64,
}

impl Limits {
    /// [`MAX_HISTORY_COMMITS`], [`MAX_FEED_VERSIONS`], [`MAX_FEED_BYTES`].
    pub const DEFAULT: Limits = Limits {
        commits: MAX_HISTORY_COMMITS,
        feed_versions: MAX_FEED_VERSIONS,
        feed_bytes: MAX_FEED_BYTES,
    };
}

/// What a [`Git`] has read (a test hook): objects inflated, by kind, and
/// their bytes; and the bytes charged to the budget before each read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reads {
    /// Commit (and tag) objects inflated.
    pub commits: u64,
    /// Their bytes.
    pub commit_bytes: u64,
    /// Tree objects inflated.
    pub trees: u64,
    /// Their bytes.
    pub tree_bytes: u64,
    /// Blobs inflated.
    pub blobs: u64,
    /// Their bytes.
    pub blob_bytes: u64,
    /// Every object's bytes inflated.
    pub inflated: u64,
    /// Bytes charged to the budget, each before its object was inflated.
    pub charged: u64,
    /// The largest buffer gix grew to read one object, inflating it or
    /// rebuilding it from its delta chain (its capacity after the read,
    /// refused or not): at most [`MAX_ALLOC_BYTES`].
    pub largest_buffer: u64,
    /// Reads gix answered from its object cache ([`OBJECT_CACHE_BYTES`]).
    pub object_cache_hits: u64,
    /// Delta chains gix started from its pack cache ([`PACK_CACHE_BYTES`]).
    pub pack_cache_hits: u64,
}

/// The history, and why it is incomplete when it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Walked {
    /// The history.
    pub history: History,
    /// Why some of it is missing (`history.complete` is false): a shallow
    /// clone's boundary, a bound, a commit or tree that could not be read.
    pub why_incomplete: Option<String>,
}

/// The repository, and the one function every object is read through.
mod objects {
    use std::cell::Cell;
    use std::collections::HashSet;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    pub(super) use gix::object::Kind;
    use gix::objs::Find as _;
    use gix::ObjectId;

    use super::{
        Reads, MAX_ALLOC_BYTES, MAX_COMMIT_BYTES, MAX_DELTA_DEPTH, MAX_FILE_BYTES,
        MAX_REPO_TREE_BYTES, OBJECT_CACHE_BYTES, PACK_CACHE_BYTES,
    };

    /// Which part of the budget a read may spend.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Draw {
        /// What is left, less what is set aside ([`Objects::reserve`]).
        Walk,
        /// What is set aside too: the pages' sources, read again at the end.
        Reserved,
    }

    /// Why an object was not read.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Refused {
        /// It is larger than its kind's cap.
        Cap { kind: Kind, size: u64, cap: u64 },
        /// It is larger than what is left of the budget.
        Budget { size: u64, charged: u64, limit: u64 },
        /// gix could not rebuild it from its delta chain within
        /// [`MAX_ALLOC_BYTES`] (it refused before allocating).
        Alloc { kind: Kind, size: u64 },
        /// Its delta chain is deeper than [`MAX_DELTA_DEPTH`].
        Depth { kind: Kind, depth: u32 },
    }

    /// Whether `e` is gix's refusal of an allocation over its limit
    /// (gix-pack's "Entry too large to fit in memory", gix-odb's "allocation
    /// limit exceeded"): by its class, not its text.
    fn over_alloc_limit(e: &gix::Error) -> bool {
        e.classify().has(gix::error::Class::ResourceExhaustion(
            gix::error::ResourceExhaustionKind::AllocationLimit,
        ))
    }

    /// The repository, opened read-only, and the budget of what is read.
    pub(super) struct Objects {
        repo: gix::Repository,
        limit: u64,
        left: Cell<u64>,
        reserved: Cell<u64>,
        reads: Cell<Reads>,
        // The caches' hits, read by the test hook only.
        #[cfg_attr(not(test), allow(dead_code))]
        object_hits: Arc<AtomicU64>,
        #[cfg_attr(not(test), allow(dead_code))]
        pack_hits: Arc<AtomicU64>,
    }

    /// One of gix's caches, its hits counted (for [`Reads`]).
    struct Counted<C> {
        cache: C,
        hits: Arc<AtomicU64>,
    }

    impl<C> Counted<C> {
        fn hit<T>(&self, got: Option<T>) -> Option<T> {
            if got.is_some() {
                self.hits.fetch_add(1, Ordering::Relaxed);
            }
            got
        }
    }

    impl<C: gix::odb::pack::cache::Object> gix::odb::pack::cache::Object for Counted<C> {
        fn put(&mut self, id: ObjectId, kind: Kind, data: &[u8]) {
            self.cache.put(id, kind, data);
        }

        fn get(&mut self, id: &ObjectId, out: &mut Vec<u8>) -> Option<Kind> {
            let got = self.cache.get(id, out);
            self.hit(got)
        }
    }

    impl<C: gix::odb::pack::cache::DecodeEntry> gix::odb::pack::cache::DecodeEntry for Counted<C> {
        fn put(&mut self, pack: u32, offset: u64, data: &[u8], kind: Kind, compressed: usize) {
            self.cache.put(pack, offset, data, kind, compressed);
        }

        fn get(&mut self, pack: u32, offset: u64, out: &mut Vec<u8>) -> Option<(Kind, usize)> {
            let got = self.cache.get(pack, offset, out);
            self.hit(got)
        }
    }

    /// The cap for an object of `kind`.
    pub(super) fn cap(kind: Kind) -> u64 {
        match kind {
            Kind::Commit | Kind::Tag => MAX_COMMIT_BYTES,
            Kind::Tree => MAX_REPO_TREE_BYTES,
            Kind::Blob => MAX_FILE_BYTES,
        }
    }

    impl Objects {
        pub(super) fn open(dir: &Path, limit: u64) -> Result<Objects, String> {
            let opts = gix::open::Options::isolated()
                .config_overrides([format!("gitoxide.objects.allocLimit={MAX_ALLOC_BYTES}")]);
            let mut repo =
                gix::open_opts(dir, opts).map_err(|e| format!("not a git repository: {e}"))?;
            let (object_hits, pack_hits) = (Arc::default(), Arc::default());
            let hits = Arc::clone(&object_hits);
            repo.objects.set_object_cache(move || {
                Box::new(Counted {
                    cache: gix::odb::pack::cache::object::MemoryCappedHashmap::new(
                        OBJECT_CACHE_BYTES,
                    ),
                    hits: Arc::clone(&hits),
                })
            });
            let hits = Arc::clone(&pack_hits);
            repo.objects.set_pack_cache(move || {
                Box::new(Counted {
                    cache: gix::odb::pack::cache::lru::MemoryCappedHashmap::new(PACK_CACHE_BYTES),
                    hits: Arc::clone(&hits),
                })
            });
            Ok(Objects {
                repo,
                limit,
                left: Cell::new(limit),
                reserved: Cell::new(0),
                reads: Cell::new(Reads::default()),
                object_hits,
                pack_hits,
            })
        }

        pub(super) fn hash(&self) -> gix::hash::Kind {
            self.repo.object_hash()
        }

        #[cfg(test)]
        pub(super) fn reads(&self) -> Reads {
            Reads {
                object_cache_hits: self.object_hits.load(Ordering::Relaxed),
                pack_cache_hits: self.pack_hits.load(Ordering::Relaxed),
                ..self.reads.get()
            }
        }

        /// Sets `n` bytes of what is left aside for [`Draw::Reserved`].
        pub(super) fn reserve(&self, n: u64) {
            self.reserved.set(n.min(self.left.get()));
        }

        /// Object `id`'s kind and size, from its header (no inflation; a
        /// delta's header, read along its chain).
        pub(super) fn header(&self, id: ObjectId) -> Result<(Kind, u64), String> {
            self.head(id).map(|(kind, size, _)| (kind, size))
        }

        /// [`Objects::header`], and how many deltas its chain has (none
        /// for a loose object).
        pub(super) fn head(&self, id: ObjectId) -> Result<(Kind, u64, u32), String> {
            let h = self
                .repo
                .find_header(id)
                .map_err(|e| format!("{id} could not be read: {e}"))?;
            let deltas = match &h {
                gix::odb::find::Header::Packed(p) => p.num_deltas,
                gix::odb::find::Header::Loose { .. } => 0,
            };
            Ok((h.kind(), h.size(), deltas))
        }

        /// THE read: object `id`, which must be one of `kinds`, inflated
        /// only after its header's size is charged, within its kind's cap
        /// and what `draw` may spend of the budget; else, unread, why not.
        pub(super) fn read(
            &self,
            id: ObjectId,
            kinds: &[Kind],
            draw: Draw,
        ) -> Result<Result<Vec<u8>, Refused>, String> {
            let (kind, size, depth) = self.head(id)?;
            if !kinds.contains(&kind) {
                return Err(format!("{id} is a {kind}, not a {}", kinds[0]));
            }
            let cap = cap(kind);
            if size > cap {
                return Ok(Err(Refused::Cap { kind, size, cap }));
            }
            if depth > MAX_DELTA_DEPTH {
                return Ok(Err(Refused::Depth { kind, depth }));
            }
            let left = self.left.get();
            let usable = match draw {
                Draw::Walk => left.saturating_sub(self.reserved.get()),
                Draw::Reserved => left,
            };
            if size > usable {
                return Ok(Err(Refused::Budget {
                    size,
                    charged: self.limit - left,
                    limit: self.limit,
                }));
            }
            self.left.set(left - size);
            if draw == Draw::Reserved {
                self.reserved.set(self.reserved.get().saturating_sub(size));
            }
            let mut r = self.reads.get();
            r.charged += size;
            self.reads.set(r);
            // gix knows the empty tree without storing it.
            let data = if id == ObjectId::empty_tree(self.hash()) {
                Vec::new()
            } else {
                let mut buf = Vec::new();
                let found = self
                    .repo
                    .objects
                    .try_find(&id, &mut buf)
                    .map(|f| f.map(|f| (f.kind, f.data.to_vec())));
                let mut r = self.reads.get();
                r.largest_buffer = r.largest_buffer.max(buf.capacity() as u64);
                self.reads.set(r);
                let (found_kind, data) = match found {
                    Ok(Some(found)) => found,
                    Ok(None) => return Err(format!("{id} could not be read: it is not there")),
                    // Charged, not inflated: gix refused before allocating.
                    Err(e) if over_alloc_limit(&e) => {
                        return Ok(Err(Refused::Alloc { kind, size }))
                    }
                    Err(e) => return Err(format!("{id} could not be read: {e}")),
                };
                if found_kind != kind || data.len() as u64 != size {
                    return Err(format!(
                        "{id} could not be read: its header says a {kind} of {size} bytes, it is a {found_kind} of {}",
                        data.len()
                    ));
                }
                data
            };
            let mut r = self.reads.get();
            let n = data.len() as u64;
            match kind {
                Kind::Commit | Kind::Tag => {
                    r.commits += 1;
                    r.commit_bytes += n;
                }
                Kind::Tree => {
                    r.trees += 1;
                    r.tree_bytes += n;
                }
                Kind::Blob => {
                    r.blobs += 1;
                    r.blob_bytes += n;
                }
            }
            r.inflated += n;
            self.reads.set(r);
            Ok(Ok(data))
        }

        /// A shallow clone's boundary commits (none: no `shallow` file, or
        /// one that cannot be read; a missing parent then ends the walk as
        /// a commit that cannot be read).
        pub(super) fn shallow(&self) -> HashSet<ObjectId> {
            match self.repo.shallow_commits() {
                Ok(Some(c)) => c.iter().copied().collect(),
                _ => HashSet::new(),
            }
        }

        /// The object `rev` names, unpeeled, reading no object: `HEAD`, a
        /// full id, a reference (`main`, `refs/tags/v1`, `origin/main`;
        /// symbolic ones followed), or a unique abbreviated id of at least
        /// four hex digits.
        pub(super) fn name(&self, rev: &str) -> Result<ObjectId, String> {
            if rev != "HEAD" {
                if let Ok(id) = ObjectId::from_hex(rev.as_bytes()) {
                    return Ok(id);
                }
            }
            if let Ok(mut r) = self.repo.find_reference(rev) {
                for _ in 0..16 {
                    match r.follow() {
                        Some(next) => r = next.map_err(|e| e.to_string())?,
                        None => break,
                    }
                }
                return r
                    .target()
                    .try_id()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| "the reference names no object".into());
            }
            if rev.len() >= 4 && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
                let prefix = gix::hash::Prefix::from_hex(rev).map_err(|e| e.to_string())?;
                return match self.repo.objects.lookup_prefix(prefix, None) {
                    Ok(Some(Ok(id))) => Ok(id),
                    Ok(Some(Err(()))) => Err("the abbreviation is ambiguous".into()),
                    Ok(None) => Err("no object has that id".into()),
                    Err(e) => Err(e.to_string()),
                };
            }
            Err("no reference or object has that name".into())
        }
    }
}

/// Why an object was not read, as the readers answer it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum NotRead {
    /// Over a cap or the budget: the tree built refuses the build with it.
    Over {
        what: Budget,
        at_least: u64,
        limit: u64,
        why: String,
    },
    /// Missing, damaged, or not what it should be.
    Bad(String),
}

impl NotRead {
    /// With `path`, the file or directory of the tree built it is, in the
    /// name of an object that cannot be rebuilt.
    fn at(self, path: &str) -> NotRead {
        match self {
            NotRead::Over {
                what: Budget::DeltaBase { object },
                at_least,
                limit,
                why,
            } => NotRead::Over {
                what: Budget::DeltaBase {
                    object: format!("{object} ({path})").into(),
                },
                at_least,
                limit,
                why,
            },
            NotRead::Over {
                what: Budget::DeltaDepth { object },
                at_least,
                limit,
                why,
            } => NotRead::Over {
                what: Budget::DeltaDepth {
                    object: format!("{object} ({path})").into(),
                },
                at_least,
                limit,
                why,
            },
            other => other,
        }
    }

    fn why(self) -> String {
        match self {
            NotRead::Over { why, .. } | NotRead::Bad(why) => why,
        }
    }

    /// Into `budget` when it is a budget's; else the error.
    fn refuse(self, budget: &mut RepoBudget) -> Result<(), String> {
        match self {
            NotRead::Over {
                what,
                at_least,
                limit,
                ..
            } => {
                budget.refuse(what, at_least, limit);
                Ok(())
            }
            NotRead::Bad(why) => Err(why),
        }
    }
}

/// Why `--rev` names no commit to build ([`Git::resolve`]).
#[derive(Debug)]
pub enum Unresolved {
    /// It names none: missing, damaged, or not a commit or a tag of one.
    Bad(String),
    /// A tag on the way is over its cap or cannot be rebuilt: the build is
    /// refused with the budget's finding.
    Refused(RepoBudget),
}

/// What the build uses of a commit, read once.
struct Facts {
    tree: ObjectId,
    /// Its parents, each once, in order.
    parents: Vec<ObjectId>,
    time: i64,
    offset: i32,
    summary: Box<str>,
}

/// A repository, opened read-only.
pub struct Git {
    store: Objects,
    /// Every commit read, each once.
    commits: RefCell<HashMap<ObjectId, Rc<Facts>>>,
    /// Pairs of trees the history compared (a test hook).
    compared: std::cell::Cell<u64>,
}

/// Opens the repository at `dir` (a worktree or a bare repository).
pub fn open(dir: &Path) -> Result<Git, String> {
    open_with(dir, MAX_OBJECT_BYTES)
}

/// [`open`], with a budget of `object_bytes` ([`MAX_OBJECT_BYTES`]'s
/// place, for tests).
pub fn open_with(dir: &Path, object_bytes: u64) -> Result<Git, String> {
    Ok(Git {
        store: Objects::open(dir, object_bytes)?,
        commits: RefCell::new(HashMap::new()),
        compared: std::cell::Cell::new(0),
    })
}

const TREE_MODE: u16 = 0o040000;

/// What a tree entry is, by its exact mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Tree,
    Blob,
    Link,
    Gitlink,
    Unknown,
}

fn entry_kind(mode: u16) -> Entry {
    match mode {
        TREE_MODE => Entry::Tree,
        0o100644 | 0o100755 | 0o100664 => Entry::Blob,
        0o120000 => Entry::Link,
        0o160000 => Entry::Gitlink,
        _ => Entry::Unknown,
    }
}

/// A mode as git compares it (`canon_mode`): a regular file is `100644` or
/// `100755` by its owner's execute bit (so the legacy `100664` is
/// `100644`), a directory `040000` (gix reads a written `040000` as
/// `0o140000`), a link `120000`, anything else a gitlink.
fn canon(mode: u16) -> u16 {
    match mode & 0o170000 {
        0o100000 if mode & 0o100 != 0 => 0o100755,
        0o100000 => 0o100644,
        0o040000 | 0o140000 => TREE_MODE,
        0o120000 => 0o120000,
        _ => 0o160000,
    }
}

/// An entry looked up by its path.
enum Lookup {
    /// Its mode and id.
    Found(u16, ObjectId),
    /// There is none.
    Missing,
    /// The budget stopped the way there.
    Stopped,
}

/// The commits walked: each one's facts (no parents read from a shallow
/// boundary), in `--date-order`.
struct Graph {
    nodes: HashMap<ObjectId, Node>,
    order: Vec<ObjectId>,
    /// Why commits behind those walked were not: a bound, or a commit that
    /// could not be read.
    stopped: Option<String>,
}

struct Node {
    facts: Rc<Facts>,
    /// A shallow clone's boundary: its parents are not in the repository.
    shallow: bool,
}

/// `ids` without repeats, in order.
fn distinct(ids: &[ObjectId]) -> Vec<ObjectId> {
    let mut seen = HashSet::new();
    ids.iter().copied().filter(|i| seen.insert(*i)).collect()
}

/// A commit as the pages show it.
fn shown(id: ObjectId, f: &Facts) -> Commit {
    Commit {
        id: id.to_hex().to_string(),
        time: f.time,
        offset: f.offset,
        summary: f.summary.to_string(),
    }
}

impl Git {
    /// Object `id` (one of `kinds`), through the one read.
    fn read(&self, id: ObjectId, kinds: &[Kind], draw: Draw) -> Result<Vec<u8>, NotRead> {
        match self.store.read(id, kinds, draw) {
            Ok(Ok(data)) => Ok(data),
            Ok(Err(Refused::Cap { kind, size, cap })) => Err(NotRead::Over {
                what: match kind {
                    Kind::Tree => Budget::TreeBytes,
                    Kind::Blob => Budget::Bytes,
                    Kind::Commit | Kind::Tag => Budget::CommitBytes,
                },
                at_least: size,
                limit: cap,
                why: format!("{kind} {id} is {size} bytes, over the cap of {cap}"),
            }),
            Ok(Err(Refused::Budget {
                size,
                charged,
                limit,
            })) => Err(NotRead::Over {
                what: Budget::ObjectBytes,
                at_least: charged.saturating_add(size),
                limit,
                why: format!(
                    "the build read {charged} bytes of git objects, and the next ({id}, {size} bytes) is over the budget of {limit}"
                ),
            }),
            Ok(Err(Refused::Alloc { kind, size })) => Err(NotRead::Over {
                what: Budget::DeltaBase {
                    object: format!("{kind} {id}").into(),
                },
                at_least: MAX_ALLOC_BYTES as u64 + 1,
                limit: MAX_ALLOC_BYTES as u64,
                why: format!(
                    "{kind} {id} ({size} bytes) cannot be rebuilt within the memory limit of {MAX_ALLOC_BYTES} bytes (too large, or the pack is damaged: `git fsck` tells which)"
                ),
            }),
            Ok(Err(Refused::Depth { kind, depth })) => Err(NotRead::Over {
                what: Budget::DeltaDepth {
                    object: format!("{kind} {id}").into(),
                },
                at_least: depth.into(),
                limit: MAX_DELTA_DEPTH.into(),
                why: format!(
                    "{kind} {id}'s delta chain is {depth} deep, deeper than git writes ({MAX_DELTA_DEPTH})"
                ),
            }),
            Err(e) => Err(NotRead::Bad(e)),
        }
    }

    /// What has been read so far.
    #[cfg(test)]
    pub fn reads(&self) -> Reads {
        self.store.reads()
    }

    /// Commit `id`'s facts, read once (the walk, the tree built and the
    /// dates all share the one read).
    fn commit(&self, id: ObjectId) -> Result<Rc<Facts>, NotRead> {
        if let Some(f) = self.commits.borrow().get(&id) {
            return Ok(f.clone());
        }
        let data = self.read(id, &[Kind::Commit], Draw::Walk)?;
        let hash = self.store.hash();
        let iter = || CommitRefIter::from_bytes(&data, hash);
        let bad = |what: &str, e: &dyn std::fmt::Display| {
            NotRead::Bad(format!("commit {id}'s {what} could not be read: {e}"))
        };
        let tree = iter().tree_id().map_err(|e| bad("tree", &e))?;
        let parents = distinct(&iter().parent_ids().collect::<Vec<_>>());
        let time = iter()
            .committer()
            .map_err(|e| bad("committer", &e))?
            .time()
            .map_err(|e| bad("time", &e))?;
        let message = iter().message().map_err(|e| bad("message", &e))?;
        let summary: String = String::from_utf8_lossy(message)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(MAX_SUMMARY_CHARS)
            .collect();
        let f = Rc::new(Facts {
            tree,
            parents,
            time: time.seconds,
            offset: time.offset,
            summary: summary.into(),
        });
        self.commits.borrow_mut().insert(id, f.clone());
        Ok(f)
    }

    /// The commit `rev` names: `HEAD`, a full commit id, a reference
    /// (`main`, `refs/tags/v1`, `origin/main`), or a unique abbreviated id
    /// of at least four hex digits; a tag is peeled to its commit. A tag
    /// over its cap, or that cannot be rebuilt from its delta chain,
    /// refuses the build as the commit built does ([`Unresolved::Refused`]).
    pub fn resolve(&self, rev: &str) -> Result<ObjectId, Unresolved> {
        let not = |e: String| Unresolved::Bad(format!("{rev:?} names no commit: {e}"));
        let mut id = self.store.name(rev).map_err(not)?;
        for _ in 0..MAX_TAG_DEPTH {
            match self.store.header(id).map_err(not)?.0 {
                Kind::Commit => return Ok(id),
                Kind::Tag => {
                    let data = match self.read(id, &[Kind::Tag], Draw::Walk) {
                        Ok(d) => d,
                        Err(e) => {
                            let mut budget = RepoBudget::default();
                            return Err(match e.refuse(&mut budget) {
                                Ok(()) => Unresolved::Refused(budget),
                                Err(why) => not(why),
                            });
                        }
                    };
                    id = TagRefIter::from_bytes(&data, self.store.hash())
                        .target_id()
                        .map_err(|e| not(format!("tag {id}: {e}")))?;
                }
                kind => return Err(not(format!("{id} is a {kind}"))),
            }
        }
        Err(not(format!(
            "more than {MAX_TAG_DEPTH} tags before a commit"
        )))
    }

    /// A blob's bytes (a page's source, read again for its `.md` copy;
    /// from the bytes set aside for it).
    pub fn blob(&self, id: &str) -> Result<Vec<u8>, String> {
        let id = ObjectId::from_hex(id.as_bytes()).map_err(|e| format!("a blob id: {e}"))?;
        self.read(id, &[Kind::Blob], Draw::Reserved)
            .map_err(NotRead::why)
    }

    /// Pairs of trees the history has compared so far.
    #[cfg(test)]
    fn compared(&self) -> u64 {
        self.compared.get()
    }

    /// Bytes of tree objects read so far.
    #[cfg(test)]
    fn tree_bytes_read(&self) -> u64 {
        self.reads().tree_bytes
    }

    /// Bytes of blobs read so far.
    #[cfg(test)]
    fn blob_bytes_read(&self) -> u64 {
        self.reads().blob_bytes
    }

    /// The entry at `path` (`/`-separated, empty segments skipped) under
    /// tree `root`, for the tree built. Every entry read on the way counts
    /// against `budget`, before it is looked at.
    fn entry_at(
        &self,
        root: ObjectId,
        path: &str,
        budget: &mut RepoBudget,
    ) -> Result<Lookup, String> {
        let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut tree = root;
        let mut prefix = 0;
        for (i, seg) in segs.iter().enumerate() {
            let data = match self.read(tree, &[Kind::Tree], Draw::Walk) {
                Ok(d) => d,
                Err(e) => {
                    e.refuse(budget)?;
                    return Ok(Lookup::Stopped);
                }
            };
            let mut found = None;
            for e in TreeRefIter::from_bytes(&data, self.store.hash()) {
                let e = e.map_err(|e| format!("tree {tree} could not be read: {e}"))?;
                if !budget.entry_len(prefix + e.filename.len()) {
                    return Ok(Lookup::Stopped);
                }
                if e.filename == seg.as_bytes() {
                    found = Some((e.mode.value(), e.oid.to_owned()));
                    break;
                }
            }
            let Some((mode, id)) = found else {
                return Ok(Lookup::Missing);
            };
            if i + 1 == segs.len() {
                return Ok(Lookup::Found(mode, id));
            }
            if entry_kind(mode) != Entry::Tree {
                return Ok(Lookup::Missing);
            }
            tree = id;
            prefix += seg.len() + 1;
        }
        Ok(Lookup::Found(TREE_MODE, root))
    }

    /// The tree of `commit` (or its `subtree`), as the site reads it, and
    /// what it counted: when the budget does not hold, the read stopped
    /// there, and the build is refused ([`RepoBudget::findings`]). Then
    /// the bytes it read are set aside for the pages' sources.
    pub fn read_tree(
        &self,
        commit: ObjectId,
        subtree: Option<&str>,
        cfg: &SiteConfig,
    ) -> Result<(Tree, RepoBudget), String> {
        let mut files = Vec::new();
        let mut problems = Vec::new();
        let mut budget = RepoBudget::default();
        let facts = match self.commit(commit) {
            Ok(f) => f,
            Err(e) => {
                e.refuse(&mut budget)?;
                return Ok((Tree { files, problems }, budget));
            }
        };
        let root = match self.entry_at(facts.tree, subtree.unwrap_or(""), &mut budget)? {
            Lookup::Found(mode, id) if entry_kind(mode) == Entry::Tree => id,
            Lookup::Found(..) | Lookup::Missing => {
                return Err(format!(
                    "{} has no directory {}",
                    commit.to_hex(),
                    subtree.unwrap_or("")
                ))
            }
            Lookup::Stopped => return Ok((Tree { files, problems }, budget)),
        };
        let mut stack: Vec<(ObjectId, String)> = vec![(root, String::new())];
        'walk: while let Some((tree, prefix)) = stack.pop() {
            let data = match self.read(tree, &[Kind::Tree], Draw::Walk) {
                Ok(d) => d,
                Err(e) => {
                    e.at(if prefix.is_empty() { "/" } else { &prefix })
                        .refuse(&mut budget)?;
                    break;
                }
            };
            for e in TreeRefIter::from_bytes(&data, self.store.hash()) {
                let e = e.map_err(|e| format!("tree {tree} could not be read: {e}"))?;
                // Counted first, whatever it is.
                if !budget.entry_len(prefix.len() + e.filename.len()) {
                    break 'walk;
                }
                let Ok(name) = std::str::from_utf8(e.filename) else {
                    let mut bytes = prefix.as_bytes().to_vec();
                    bytes.extend_from_slice(e.filename);
                    problems.push(ReadProblem::PathNotUtf8 {
                        path: lossless_display(&bytes),
                    });
                    continue;
                };
                let id = e.oid.to_owned();
                let rel = format!("{prefix}{name}");
                let over = rel.len() > MAX_PATH_BYTES || rel.split('/').count() > MAX_PATH_SEGMENTS;
                let file = |kind: FileKind, bytes: Vec<u8>, blob: bool| SourceFile {
                    path: rel.clone(),
                    bytes,
                    blob_id: blob.then(|| id.to_hex().to_string()),
                    kind,
                };
                let handed = match entry_kind(e.mode.value()) {
                    Entry::Tree if over => file(FileKind::Other, Vec::new(), false),
                    Entry::Tree => {
                        stack.push((id, format!("{rel}/")));
                        continue;
                    }
                    Entry::Blob => {
                        if over || !rubrica_site::reads_bytes(&rel, cfg) {
                            file(FileKind::Regular, Vec::new(), true)
                        } else {
                            let (_, size) = self
                                .store
                                .header(id)
                                .map_err(|e| format!("{rel}'s object: {e}"))?;
                            if size > MAX_FILE_BYTES {
                                file(FileKind::TooLarge, Vec::new(), true)
                            } else {
                                crate::read::trace_open(&rel);
                                match self.read(id, &[Kind::Blob], Draw::Walk) {
                                    Ok(bytes) => file(FileKind::Regular, bytes, true),
                                    Err(e) => {
                                        e.at(&rel).refuse(&mut budget)?;
                                        break 'walk;
                                    }
                                }
                            }
                        }
                    }
                    Entry::Link => file(FileKind::Symlink, Vec::new(), false),
                    Entry::Gitlink => file(FileKind::Submodule, Vec::new(), false),
                    Entry::Unknown => file(FileKind::UnknownMode, Vec::new(), false),
                };
                let within = budget.read(handed.bytes.len() as u64);
                files.push(handed);
                if !within {
                    break 'walk;
                }
            }
        }
        self.store.reserve(budget.bytes);
        Ok((Tree { files, problems }, budget))
    }

    /// The commits behind `head`, at most `max`, read newest first by
    /// committer time (each once: a parent is read when it is first seen,
    /// for its time, and not again), then put in `--date-order`: Kahn's
    /// order over the commits walked, the newest ready commit first, ties
    /// by id. A shallow boundary's parents are not read; a commit that
    /// cannot be read, or whose time cannot, ends the walk (`stopped`).
    fn graph(&self, head: ObjectId, max: usize) -> Result<Graph, String> {
        let shallow = self.store.shallow();
        let head_facts = self.commit(head).map_err(NotRead::why)?;
        let mut facts: HashMap<ObjectId, Rc<Facts>> = HashMap::from([(head, head_facts.clone())]);
        let mut nodes: HashMap<ObjectId, Node> = HashMap::new();
        let mut heap: BinaryHeap<(i64, ObjectId)> = BinaryHeap::new();
        heap.push((head_facts.time, head));
        let mut stopped = None;
        'walk: while let Some((_, id)) = heap.pop() {
            if nodes.len() == max {
                stopped = Some(format!("the walk reached its bound of {max} commits"));
                break;
            }
            let f = facts[&id].clone();
            let is_shallow = shallow.contains(&id);
            nodes.insert(
                id,
                Node {
                    facts: f.clone(),
                    shallow: is_shallow,
                },
            );
            if is_shallow {
                continue;
            }
            for &p in &f.parents {
                if facts.contains_key(&p) {
                    continue;
                }
                match self.commit(p) {
                    Ok(pf) => {
                        heap.push((pf.time, p));
                        facts.insert(p, pf);
                    }
                    Err(e) => {
                        stopped = Some(e.why());
                        break 'walk;
                    }
                }
            }
        }
        // Each commit's children among those walked.
        let mut children: HashMap<ObjectId, usize> = HashMap::new();
        for n in nodes.values().filter(|n| !n.shallow) {
            for p in &n.facts.parents {
                if nodes.contains_key(p) {
                    *children.entry(*p).or_default() += 1;
                }
            }
        }
        let mut ready: BinaryHeap<(i64, ObjectId)> = BinaryHeap::new();
        ready.push((head_facts.time, head));
        let mut order = Vec::with_capacity(nodes.len());
        while let Some((_, id)) = ready.pop() {
            order.push(id);
            let n = &nodes[&id];
            if n.shallow {
                continue;
            }
            for p in &n.facts.parents {
                if let Some(c) = children.get_mut(p) {
                    *c -= 1;
                    if *c == 0 {
                        ready.push((nodes[p].facts.time, *p));
                    }
                }
            }
        }
        Ok(Graph {
            nodes,
            order,
            stopped,
        })
    }

    /// The commits behind `commit` (it first), every child before its
    /// parents and otherwise newest first by committer time (git's
    /// `--date-order`), ties by id.
    #[cfg(test)]
    pub fn date_order(&self, commit: ObjectId) -> Result<Vec<ObjectId>, String> {
        Ok(self.graph(commit, MAX_HISTORY_COMMITS)?.order)
    }

    /// The history the build needs: for each of `files` (paths in the
    /// (sub)tree built), the commit that last changed it; the newest
    /// [`RECENT_COMMITS`] commits that changed one; and, for `feed`, each
    /// version of it (newest first). The module doc says how each is git's.
    pub fn history(
        &self,
        commit: ObjectId,
        subtree: Option<&str>,
        files: &BTreeSet<String>,
        feed: Option<&str>,
    ) -> Result<Walked, String> {
        self.history_within(commit, subtree, files, feed, &Limits::DEFAULT)
    }

    /// [`Git::history`] within `limits`. Only the commit built must be
    /// readable (else `Err`); past it, whatever cannot be read ends the
    /// walk, which then says why.
    pub fn history_within(
        &self,
        commit: ObjectId,
        subtree: Option<&str>,
        files: &BTreeSet<String>,
        feed: Option<&str>,
        limits: &Limits,
    ) -> Result<Walked, String> {
        let graph = self.graph(commit, limits.commits)?;
        let start = shown(commit, &graph.nodes[&commit].facts);
        let paths = Paths::new(subtree, files, feed);
        let pending: Vec<u32> = (0..paths.files).collect();
        let mut walk = Walk {
            git: self,
            limits,
            feed: paths.feed,
            paths,
            cache: TreeCache::new(TREE_CACHE_BYTES),
            pending: HashMap::from([(commit, pending)]),
            recent_at: HashSet::from([commit]),
            feed_at: feed.map(|_| HashSet::from([commit])).unwrap_or_default(),
            last: BTreeMap::new(),
            recent: Vec::new(),
            feed_versions: Vec::new(),
            feed_bytes: 0,
            why: None,
        };
        for &id in &graph.order {
            if walk.pending.is_empty() && walk.recent_at.is_empty() && walk.feed_at.is_empty() {
                break;
            }
            if let Err(why) = walk.step(&graph, id) {
                walk.why.get_or_insert(why);
                break;
            }
        }
        // Newest first by committer time, ties by the greater id
        // (not git's order over its simplified graph).
        walk.recent
            .sort_by(|a, b| (b.commit.time, &b.commit.id).cmp(&(a.commit.time, &a.commit.id)));
        let why_incomplete = walk.why;
        Ok(Walked {
            history: History {
                commit: start,
                last: walk.last,
                recent: walk.recent,
                feed_versions: walk.feed_versions,
                complete: why_incomplete.is_none(),
            },
            why_incomplete,
        })
    }
}

/// The paths the history is asked about, from the repository's root (under
/// `--subtree`, with its path), and every directory one is under (with its
/// `/`), each by a slot: the files first (`0..files`), then the feed when
/// it is no file, then the directories.
struct Paths {
    /// Each slot's path.
    names: Vec<String>,
    /// Whether each slot is a directory.
    dir: Vec<bool>,
    slots: HashMap<String, u32>,
    /// How many of the slots are the files asked about.
    files: u32,
    /// The feed's slot.
    feed: Option<u32>,
    /// The length of `--subtree`'s path and its `/` (cut from what is
    /// answered).
    cut: usize,
    /// The longest slot's path, in bytes.
    longest: usize,
}

/// The slot of no directory: a commit's root.
const ROOT: u32 = u32::MAX;

impl Paths {
    fn new(subtree: Option<&str>, files: &BTreeSet<String>, feed: Option<&str>) -> Paths {
        let pre: String = subtree
            .map(|s| {
                let s: Vec<&str> = s.split('/').filter(|x| !x.is_empty()).collect();
                if s.is_empty() {
                    String::new()
                } else {
                    format!("{}/", s.join("/"))
                }
            })
            .unwrap_or_default();
        let mut p = Paths {
            names: Vec::new(),
            dir: Vec::new(),
            slots: HashMap::new(),
            files: 0,
            feed: None,
            cut: pre.len(),
            longest: 0,
        };
        let add = |p: &mut Paths, name: String, dir: bool| -> u32 {
            if let Some(&s) = p.slots.get(&name) {
                return s;
            }
            let s = p.names.len() as u32;
            p.slots.insert(name.clone(), s);
            p.longest = p.longest.max(name.len());
            p.names.push(name);
            p.dir.push(dir);
            s
        };
        for f in files {
            add(&mut p, format!("{pre}{f}"), false);
        }
        p.files = p.names.len() as u32;
        if let Some(f) = feed {
            p.feed = Some(add(&mut p, format!("{pre}{f}"), false));
        }
        for i in 0..p.names.len() {
            let full = p.names[i].clone();
            let mut end = 0;
            for seg in full.split('/') {
                end += seg.len() + 1;
                if end <= full.len() {
                    add(&mut p, full[..end].to_owned(), true);
                }
            }
        }
        p
    }

    /// Slot `s`'s path as the caller named it.
    fn answer(&self, s: u32) -> String {
        self.names[s as usize][self.cut..].to_owned()
    }

    fn is_file(&self, s: u32) -> bool {
        s < self.files
    }
}

/// What a tree holds of the paths asked about: (slot, mode as git compares
/// it, id), in slot order, one a slot.
type Kept = Rc<Vec<(u32, u16, ObjectId)>>;

/// Each tree's [`Kept`] entries by (tree, the directory slot it is read
/// at), at most `cap` bytes of them, the least recently used dropped first.
struct TreeCache {
    map: HashMap<(ObjectId, u32), (Kept, u64)>,
    by_use: BTreeMap<u64, (ObjectId, u32)>,
    tick: u64,
    bytes: usize,
    cap: usize,
}

impl TreeCache {
    fn new(cap: usize) -> TreeCache {
        TreeCache {
            map: HashMap::new(),
            by_use: BTreeMap::new(),
            tick: 0,
            bytes: 0,
            cap,
        }
    }

    /// What `k` holds in memory, its allocation and the map's share.
    fn cost(k: &Kept) -> usize {
        128 + k.capacity() * std::mem::size_of::<(u32, u16, ObjectId)>()
    }

    fn get(&mut self, key: (ObjectId, u32)) -> Option<Kept> {
        let (k, used) = self.map.get_mut(&key)?;
        self.by_use.remove(used);
        self.tick += 1;
        *used = self.tick;
        self.by_use.insert(self.tick, key);
        Some(k.clone())
    }

    fn put(&mut self, key: (ObjectId, u32), k: Kept) {
        let cost = Self::cost(&k);
        if cost > self.cap {
            return;
        }
        while self.bytes + cost > self.cap {
            let Some((_, old)) = self.by_use.pop_first() else {
                break;
            };
            if let Some((gone, _)) = self.map.remove(&old) {
                self.bytes -= Self::cost(&gone);
            }
        }
        self.tick += 1;
        self.by_use.insert(self.tick, key);
        self.map.insert(key, (k, self.tick));
        self.bytes += cost;
    }
}

impl Git {
    /// Tree `tree`'s entries of the paths asked about, read at directory
    /// slot `at` ([`ROOT`]: a commit's root): from the cache, or read once
    /// and kept. A path's entry is its first that is not a tree (else its
    /// first tree), a directory's its first tree, so a blob and a tree of
    /// one name are each what they are.
    fn kept(
        &self,
        tree: ObjectId,
        at: u32,
        paths: &Paths,
        cache: &mut TreeCache,
    ) -> Result<Kept, NotRead> {
        if let Some(k) = cache.get((tree, at)) {
            return Ok(k);
        }
        let data = self.read(tree, &[Kind::Tree], Draw::Walk)?;
        let pre: &str = if at == ROOT {
            ""
        } else {
            &paths.names[at as usize]
        };
        let mut out: Vec<(u32, bool, u16, ObjectId)> = Vec::new();
        let mut key = String::from(pre);
        for e in TreeRefIter::from_bytes(&data, self.store.hash()) {
            let e = e.map_err(|e| NotRead::Bad(format!("tree {tree} could not be read: {e}")))?;
            // No path asked about is longer, or not UTF-8.
            if pre.len() + e.filename.len() > paths.longest {
                continue;
            }
            let Ok(name) = std::str::from_utf8(e.filename) else {
                continue;
            };
            let mode = canon(e.mode.value());
            let is_tree = mode == TREE_MODE;
            key.truncate(pre.len());
            key.push_str(name);
            if let Some(&s) = paths.slots.get(key.as_str()) {
                if !paths.dir[s as usize] {
                    out.push((s, is_tree, mode, e.oid.to_owned()));
                }
            }
            if is_tree {
                key.push('/');
                if let Some(&s) = paths.slots.get(key.as_str()) {
                    out.push((s, false, mode, e.oid.to_owned()));
                }
            }
        }
        // Stable: within a slot, a non-tree first, then the tree's order.
        out.sort_by_key(|&(s, is_tree, ..)| (s, is_tree));
        out.dedup_by_key(|e| e.0);
        let mut k: Vec<(u32, u16, ObjectId)> =
            out.into_iter().map(|(s, _, m, i)| (s, m, i)).collect();
        k.shrink_to_fit();
        let k: Kept = Rc::new(k);
        cache.put((tree, at), k.clone());
        Ok(k)
    }

    /// The paths asked about whose entries (mode as git compares it, and
    /// id) differ between trees `a` and `b` (either absent), into `out`;
    /// entering only trees whose ids differ and that hold a path asked
    /// about.
    fn diff(
        &self,
        a: Option<ObjectId>,
        b: Option<ObjectId>,
        paths: &Paths,
        cache: &mut TreeCache,
        out: &mut BTreeSet<u32>,
    ) -> Result<(), NotRead> {
        let none: Kept = Rc::new(Vec::new());
        let mut stack: Vec<(u32, Option<ObjectId>, Option<ObjectId>)> = vec![(ROOT, a, b)];
        while let Some((at, a, b)) = stack.pop() {
            if a == b {
                continue;
            }
            self.compared.set(self.compared.get() + 1);
            let ka = match a {
                Some(t) => self.kept(t, at, paths, cache)?,
                None => none.clone(),
            };
            let kb = match b {
                Some(t) => self.kept(t, at, paths, cache)?,
                None => none.clone(),
            };
            // Both in slot order: merged.
            let (mut i, mut j) = (0, 0);
            while i < ka.len() || j < kb.len() {
                let (x, y) = match (ka.get(i), kb.get(j)) {
                    (Some(x), Some(y)) if x.0 == y.0 => {
                        i += 1;
                        j += 1;
                        (Some(x), Some(y))
                    }
                    (Some(x), Some(y)) if x.0 < y.0 => {
                        i += 1;
                        (Some(x), None)
                    }
                    (Some(x), None) => {
                        i += 1;
                        (Some(x), None)
                    }
                    (_, y) => {
                        j += 1;
                        (None, y)
                    }
                };
                if x.map(|e| (e.1, e.2)) == y.map(|e| (e.1, e.2)) {
                    continue;
                }
                let s = x.or(y).expect("one side").0;
                if paths.dir[s as usize] {
                    stack.push((s, x.map(|e| e.2), y.map(|e| e.2)));
                } else {
                    out.insert(s);
                }
            }
        }
        Ok(())
    }

    /// The entry of path slot `s` in root tree `root`, through the cache:
    /// down the directories it is under.
    fn lookup(
        &self,
        root: ObjectId,
        s: u32,
        paths: &Paths,
        cache: &mut TreeCache,
    ) -> Result<Option<(u16, ObjectId)>, NotRead> {
        let name = &paths.names[s as usize];
        let mut at = ROOT;
        let mut tree = root;
        let mut end = 0;
        for seg in name.split('/') {
            end += seg.len() + 1;
            let last = end > name.len();
            let want = if last { s } else { paths.slots[&name[..end]] };
            let k = self.kept(tree, at, paths, cache)?;
            let Some(&(_, mode, id)) = k.iter().find(|e| e.0 == want) else {
                return Ok(None);
            };
            if last {
                return Ok(Some((mode, id)));
            }
            at = want;
            tree = id;
        }
        Ok(None)
    }
}

/// The history walk's state ([`Git::history_within`]).
struct Walk<'a> {
    git: &'a Git,
    limits: &'a Limits,
    paths: Paths,
    /// The feed's slot, until its walk ends.
    feed: Option<u32>,
    cache: TreeCache,
    /// The file slots whose `git log -1` walk is at each commit.
    pending: HashMap<ObjectId, Vec<u32>>,
    /// The commits `git log -- <pages>` reaches and has not yet decided.
    recent_at: HashSet<ObjectId>,
    /// Those `git log -- <feed>` reaches.
    feed_at: HashSet<ObjectId>,
    last: BTreeMap<String, Commit>,
    recent: Vec<RecentCommit>,
    feed_versions: Vec<(Commit, Vec<u8>)>,
    feed_bytes: u64,
    /// Why an answer is missing, once one is.
    why: Option<String>,
}

/// What git's simplification makes of a commit for a set of paths.
enum Simplified {
    /// TREESAME to this parent (the first such): not shown; only it is
    /// followed.
    Same(ObjectId),
    /// TREESAME to no parent: shown; every parent is followed.
    Shown,
    /// A root without any of the paths: not shown, nothing followed.
    Empty,
    /// It depends on a parent the walk does not have.
    Unknown,
}

impl Walk<'_> {
    /// One commit, in `--date-order` (so every child that could hand it a
    /// page has been): the pages whose walks are at it, and its place in
    /// recent changes' and the feed's. `Err` ends the walk, saying why.
    fn step(&mut self, graph: &Graph, id: ObjectId) -> Result<(), String> {
        let pending = self.pending.remove(&id).unwrap_or_default();
        let recent = self.recent_at.remove(&id);
        let feed = self.feed_at.remove(&id);
        if pending.is_empty() && !recent && !feed {
            return Ok(());
        }
        let node = &graph.nodes[&id];
        let here = node.facts.tree;
        // Each parent's differences (each parent once), in order, up to
        // the first the walk does not have.
        let mut known: Vec<(ObjectId, BTreeSet<u32>)> = Vec::new();
        let mut unknown: Option<String> = None;
        if node.shallow {
            unknown = Some(format!(
                "the clone is shallow: what its boundary commit {} changed is unknown",
                id.to_hex()
            ));
        } else {
            for &p in &node.facts.parents {
                let Some(pn) = graph.nodes.get(&p) else {
                    unknown = Some(
                        graph
                            .stopped
                            .clone()
                            .unwrap_or_else(|| format!("commit {p} was not walked")),
                    );
                    break;
                };
                let mut d = BTreeSet::new();
                self.git
                    .diff(
                        Some(pn.facts.tree),
                        Some(here),
                        &self.paths,
                        &mut self.cache,
                        &mut d,
                    )
                    .map_err(NotRead::why)?;
                known.push((p, d));
            }
        }
        let root = !node.shallow && node.facts.parents.is_empty();
        let mut present = BTreeSet::new();
        if root {
            self.git
                .diff(None, Some(here), &self.paths, &mut self.cache, &mut present)
                .map_err(NotRead::why)?;
        }
        let info = || shown(id, &node.facts);

        // Last updated: each page follows the first parent with its entry.
        let mut rest = pending;
        for (p, d) in &known {
            if rest.is_empty() {
                break;
            }
            let (same, differ): (Vec<u32>, Vec<u32>) =
                rest.into_iter().partition(|f| !d.contains(f));
            if !same.is_empty() {
                self.pending.entry(*p).or_default().extend(same);
            }
            rest = differ;
        }
        if !rest.is_empty() {
            match &unknown {
                Some(why) => {
                    self.why.get_or_insert(why.clone());
                }
                None => {
                    for f in rest.into_iter().filter(|f| !root || present.contains(f)) {
                        self.last.insert(self.paths.answer(f), info());
                    }
                }
            }
        }

        let simplify = |inside: &dyn Fn(u32) -> bool| -> Simplified {
            for (p, d) in &known {
                if !d.iter().any(|&f| inside(f)) {
                    return Simplified::Same(*p);
                }
            }
            if unknown.is_some() {
                Simplified::Unknown
            } else if root && !present.iter().any(|&f| inside(f)) {
                Simplified::Empty
            } else {
                Simplified::Shown
            }
        };
        let paths = &self.paths;
        if recent && self.recent.len() < RECENT_COMMITS {
            match simplify(&|f| paths.is_file(f)) {
                Simplified::Same(p) => {
                    self.recent_at.insert(p);
                }
                Simplified::Unknown => {
                    self.why.get_or_insert(unknown.clone().unwrap_or_default());
                }
                Simplified::Empty => {}
                Simplified::Shown => {
                    self.recent_at.extend(node.facts.parents.iter().copied());
                    // The pages it changed against every parent.
                    let mut changed: BTreeSet<u32> = if root {
                        present.clone()
                    } else {
                        known[0].1.clone()
                    };
                    for (_, d) in known.iter().skip(1) {
                        changed.retain(|f| d.contains(f));
                    }
                    let mut pages: Vec<String> = changed
                        .into_iter()
                        .filter(|&f| paths.is_file(f))
                        .map(|f| paths.answer(f))
                        .collect();
                    if !pages.is_empty() {
                        pages.sort();
                        let more = pages.len().saturating_sub(RECENT_PAGES_PER_COMMIT);
                        pages.truncate(RECENT_PAGES_PER_COMMIT);
                        self.recent.push(RecentCommit {
                            commit: info(),
                            files: pages,
                            more,
                        });
                        if self.recent.len() == RECENT_COMMITS {
                            self.recent_at.clear();
                        }
                    }
                }
            }
        }
        if let (true, Some(f)) = (feed, self.feed) {
            match simplify(&|p| p == f) {
                Simplified::Same(p) => {
                    self.feed_at.insert(p);
                }
                Simplified::Unknown => {
                    self.why.get_or_insert(unknown.clone().unwrap_or_default());
                }
                Simplified::Empty => {}
                Simplified::Shown => {
                    self.feed_at.extend(node.facts.parents.iter().copied());
                    self.feed_version(id, &node.facts, f)?;
                }
            }
        }
        Ok(())
    }

    /// The feed's version at commit `id`, if it has one: its size from
    /// the object's header first, so a version over the bounds is never
    /// read; then the feed's walk ends.
    fn feed_version(&mut self, id: ObjectId, facts: &Facts, f: u32) -> Result<(), String> {
        let Some((mode, blob)) = self
            .git
            .lookup(facts.tree, f, &self.paths, &mut self.cache)
            .map_err(NotRead::why)?
        else {
            return Ok(());
        };
        if !matches!(mode, 0o100644 | 0o100755) {
            return Ok(());
        }
        let (_, size) = self.git.store.header(blob)?;
        let over_cap = size > objects::cap(Kind::Blob);
        if over_cap
            || self.feed_versions.len() == self.limits.feed_versions
            || self.feed_bytes.saturating_add(size) > self.limits.feed_bytes
        {
            self.why.get_or_insert(format!(
                "the feed's versions reached their bound ({} versions, {} bytes, each at most {})",
                self.limits.feed_versions,
                self.limits.feed_bytes,
                objects::cap(Kind::Blob)
            ));
            self.feed_at.clear();
            self.feed = None;
            return Ok(());
        }
        let bytes = self
            .git
            .read(blob, &[Kind::Blob], Draw::Walk)
            .map_err(NotRead::why)?;
        self.feed_bytes += bytes.len() as u64;
        self.feed_versions.push((shown(id, facts), bytes));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! The history against git itself (`git log -1 -- <path>`, `git log --
    //! <paths>`, `git rev-list --date-order`) on generated histories, and
    //! the readers' bounds with small [`Limits`].

    use super::*;
    use rubrica_site::build::RECENT_PAGES_PER_COMMIT;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A directory under the system temp dir, removed (with all it holds)
    /// when dropped, passing or panicking: a passing run leaves nothing in
    /// TMPDIR (`scripts/ci.sh` checks).
    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl AsRef<Path> for Scratch {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    fn scratch(tag: &str) -> Scratch {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rubrica-git-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    /// `git` in `dir`, isolated from any user or system config.
    fn git(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> String {
        let mut cmd = Command::new("git");
        cmd.current_dir(dir)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@e.invalid")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@e.invalid")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("git");
        {
            use std::io::Write as _;
            let mut i = child.stdin.take().unwrap();
            if let Some(b) = stdin {
                i.write_all(b).unwrap();
            }
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// A repository made by `git fast-import` from `stream`.
    fn import(tag: &str, stream: &[u8]) -> Scratch {
        let dir = scratch(tag);
        git(&dir, &["init", "-q", "-b", "main"], None);
        git(&dir, &["fast-import", "--quiet"], Some(stream));
        dir
    }

    /// A fast-import commit on `branch` (mark `mark`, at `time`), its tree
    /// exactly `files`.
    fn commit(
        out: &mut Vec<u8>,
        branch: &str,
        mark: usize,
        time: i64,
        parents: &[usize],
        files: &BTreeMap<String, String>,
    ) {
        let msg = format!("c{mark}");
        out.extend_from_slice(
            format!(
                "commit refs/heads/{branch}\nmark :{mark}\ncommitter T <t@e.invalid> {time} +0000\ndata {}\n{msg}\n",
                msg.len()
            )
            .as_bytes(),
        );
        for (i, p) in parents.iter().enumerate() {
            let kw = if i == 0 { "from" } else { "merge" };
            out.extend_from_slice(format!("{kw} :{p}\n").as_bytes());
        }
        out.extend_from_slice(b"deleteall\n");
        for (path, body) in files {
            out.extend_from_slice(
                format!("M 100644 inline {path}\ndata {}\n{body}\n", body.len()).as_bytes(),
            );
        }
    }

    fn body(path: &str, v: u64) -> String {
        format!("---\ntitle: {path}\n---\n# {path}\n\nversion {v}\n")
    }

    const PAGES: [&str; 7] = [
        "home.md", "a.md", "b.md", "d/c.md", "d/e/f.md", "d/g.md", "h.md",
    ];

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            // xorshift64*
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) % n
        }
    }

    /// A commit of a generated history: its branch, mark, time, parents
    /// (marks) and its whole tree (path → mode and content).
    struct Spec {
        branch: String,
        mark: usize,
        time: i64,
        parents: Vec<usize>,
        files: BTreeMap<String, (&'static str, String)>,
    }

    /// A random history: commits on up to four branches, merges of two,
    /// octopus merges of three, criss-cross merges, pages deleted and
    /// brought back, reverts (a page's content is one of four, so a change
    /// often restores an older one), and committer times in any order
    /// (clock skew), each distinct so `--date-order` has no ties. With
    /// `modes`, an edit is sometimes a page's mode alone (`100644`, the
    /// legacy `100664` git reads as the same, or `100755`). The last spec
    /// is the head.
    fn random_specs(seed: u64, steps: usize, modes: bool) -> Vec<Spec> {
        type Files = BTreeMap<String, (&'static str, String)>;
        const MODES: [&str; 3] = ["100644", "100664", "100755"];
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut used = HashSet::new();
        let mut time = |r: &mut Rng| loop {
            let t = 1_700_000_000 + r.below(1_000_000) as i64;
            if used.insert(t) {
                return t;
            }
        };
        let mut out = Vec::new();
        let mut spec =
            |branch: String, mark: usize, time: i64, parents: &[usize], files: &Files| {
                out.push(Spec {
                    branch,
                    mark,
                    time,
                    parents: parents.to_vec(),
                    files: files.clone(),
                })
            };
        let mut mark = 1;
        let first: Files = PAGES
            .iter()
            .filter(|p| **p == "home.md" || r.below(3) > 0)
            .map(|p| (p.to_string(), ("100644", body(p, 0))))
            .collect();
        spec("b0".into(), mark, time(&mut r), &[], &first);
        let mut branches: Vec<(usize, Files)> = vec![(mark, first)];
        let edit = |r: &mut Rng, files: &mut Files| {
            let p = PAGES[r.below(PAGES.len() as u64) as usize];
            if modes && r.below(3) == 0 {
                if let Some(e) = files.get_mut(p) {
                    e.0 = MODES[r.below(3) as usize];
                    return;
                }
            }
            let v = r.below(5);
            if v == 4 && p != "home.md" {
                files.remove(p);
            } else {
                files.insert(p.to_owned(), ("100644", body(p, v.min(3))));
            }
        };
        let resolve = |r: &mut Rng, sides: &[&Files]| {
            let mut m = BTreeMap::new();
            for p in PAGES {
                let pick = r.below(sides.len() as u64 + 1) as usize;
                let v = if pick == sides.len() {
                    Some(("100644", body(p, r.below(4))))
                } else {
                    sides[pick].get(p).cloned()
                };
                if let Some(v) = v.or_else(|| (p == "home.md").then(|| ("100644", body(p, 0)))) {
                    m.insert(p.to_owned(), v);
                }
            }
            m
        };
        for _ in 0..steps {
            let n = branches.len() as u64;
            match r.below(100) {
                0..=44 => {
                    let b = r.below(n) as usize;
                    let mut files = branches[b].1.clone();
                    for _ in 0..=r.below(2) {
                        edit(&mut r, &mut files);
                    }
                    mark += 1;
                    spec(
                        format!("b{b}"),
                        mark,
                        time(&mut r),
                        &[branches[b].0],
                        &files,
                    );
                    branches[b] = (mark, files);
                }
                45..=59 if n < 4 => {
                    let b = r.below(n) as usize;
                    branches.push(branches[b].clone());
                }
                60..=79 if n >= 2 => {
                    let a = r.below(n) as usize;
                    let b = (a + 1 + r.below(n - 1) as usize) % n as usize;
                    let files = resolve(&mut r, &[&branches[a].1, &branches[b].1]);
                    mark += 1;
                    let ps = [branches[a].0, branches[b].0];
                    spec(format!("b{a}"), mark, time(&mut r), &ps, &files);
                    branches[a] = (mark, files);
                }
                80..=87 if n >= 3 => {
                    let a = r.below(n) as usize;
                    let b = (a + 1) % n as usize;
                    let c = (a + 2) % n as usize;
                    let files = resolve(&mut r, &[&branches[a].1, &branches[b].1, &branches[c].1]);
                    mark += 1;
                    let ps = [branches[a].0, branches[b].0, branches[c].0];
                    spec(format!("b{a}"), mark, time(&mut r), &ps, &files);
                    branches[a] = (mark, files);
                }
                88..=99 if n >= 2 => {
                    // Criss-cross: each of two branches merges the other.
                    let a = r.below(n) as usize;
                    let b = (a + 1 + r.below(n - 1) as usize) % n as usize;
                    let (ha, hb) = (branches[a].0, branches[b].0);
                    let fa = resolve(&mut r, &[&branches[a].1, &branches[b].1]);
                    let fb = resolve(&mut r, &[&branches[b].1, &branches[a].1]);
                    spec(format!("b{a}"), mark + 1, time(&mut r), &[ha, hb], &fa);
                    spec(format!("b{b}"), mark + 2, time(&mut r), &[hb, ha], &fb);
                    branches[a] = (mark + 1, fa);
                    branches[b] = (mark + 2, fb);
                    mark += 2;
                }
                _ => {}
            }
        }
        // HEAD is the last branch to have been committed on, or b0.
        let head = branches.iter().map(|b| b.0).max().unwrap();
        let at = out.iter().position(|s| s.mark == head).unwrap();
        let last = out.len() - 1;
        if at != last {
            // Keep the marks' order (fast-import needs a parent first);
            // say which is the head by a spec with no tree of its own.
            out.push(Spec {
                branch: "main".into(),
                mark: 0,
                time: 0,
                parents: vec![head],
                files: BTreeMap::new(),
            });
        }
        out
    }

    /// The head of `specs`: its last, or the one a closing marker names.
    fn head_mark(specs: &[Spec]) -> usize {
        let last = specs.last().unwrap();
        if last.mark == 0 {
            last.parents[0]
        } else {
            last.mark
        }
    }

    /// [`random_specs`] without modes, as a `git fast-import` stream.
    fn random_history(seed: u64, steps: usize) -> Vec<u8> {
        let specs = random_specs(seed, steps, false);
        let mut out = Vec::new();
        for sp in specs.iter().filter(|s| s.mark != 0) {
            let files: BTreeMap<String, String> = sp
                .files
                .iter()
                .map(|(p, (m, b))| {
                    assert_eq!(*m, "100644");
                    (p.clone(), b.clone())
                })
                .collect();
            commit(&mut out, &sp.branch, sp.mark, sp.time, &sp.parents, &files);
        }
        let head = head_mark(&specs);
        out.extend_from_slice(format!("reset refs/heads/main\nfrom :{head}\n").as_bytes());
        out
    }

    fn lines(s: &str) -> Vec<String> {
        s.lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// The pages `c` changed against every parent (a root: those it has),
    /// as git's `diff-tree` says, among `files`.
    fn changed_by(dir: &Path, c: &str, files: &[&str]) -> BTreeSet<String> {
        let parents = lines(&git(dir, &["rev-list", "--parents", "-n", "1", c], None))[0]
            .split(' ')
            .skip(1)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut args = vec!["ls-tree", "-r", "--name-only", c, "--"];
        args.extend_from_slice(files);
        if parents.is_empty() {
            return lines(&git(dir, &args, None)).into_iter().collect();
        }
        let mut all: Option<BTreeSet<String>> = None;
        for p in &parents {
            let mut args = vec!["diff-tree", "-r", "--name-only", "--no-renames", p, c, "--"];
            args.extend_from_slice(files);
            let set: BTreeSet<String> = lines(&git(dir, &args, None)).into_iter().collect();
            all = Some(match all {
                None => set,
                Some(a) => a.intersection(&set).cloned().collect(),
            });
        }
        all.unwrap()
    }

    /// Checks the history of `dir`'s HEAD against git: every page's date is
    /// `git log -1 -- <page>`'s commit; the walk's order is `git rev-list
    /// --date-order`'s; recent changes is the first [`RECENT_COMMITS`] of
    /// `git log -- <pages>`'s commits in that order, each with the pages it
    /// changed against every parent (none: left out), listed newest first
    /// by committer time, ties by the greater id; the feed's
    /// versions are `git log -- <feed>`'s commits that have it, with its
    /// bytes.
    fn agrees_with_git(dir: &Path, feed: Option<&str>) {
        agrees_with_git_in(dir, None, feed);
    }

    /// [`agrees_with_git`] for the directory `sub` (`--subtree`): the paths
    /// the history is asked about are relative to it, git's are not.
    fn agrees_with_git_in(dir: &Path, sub: Option<&str>, feed: Option<&str>) {
        let pre = sub.map(|s| format!("{s}/")).unwrap_or_default();
        let full = |f: &str| format!("{pre}{f}");
        let g = open(dir).unwrap();
        let head = g.resolve("HEAD").unwrap();
        let files: BTreeSet<String> =
            lines(&git(dir, &["ls-tree", "-r", "--name-only", "HEAD"], None))
                .into_iter()
                .filter(|f| f.ends_with(".md"))
                .filter_map(|f| f.strip_prefix(&pre).map(str::to_owned))
                .collect();
        let w = g.history(head, sub, &files, feed).unwrap();
        assert!(w.history.complete, "{:?}", w.why_incomplete);
        assert_eq!(w.why_incomplete, None);
        for f in &files {
            let want = git(
                dir,
                &["log", "-1", "--format=%H", "HEAD", "--", &full(f)],
                None,
            );
            assert_eq!(
                w.history.last.get(f).map(|c| c.id.as_str()),
                Some(want.as_str()),
                "{f} in {}",
                dir.display()
            );
        }
        let order: Vec<String> = g
            .date_order(head)
            .unwrap()
            .iter()
            .map(|i| i.to_hex().to_string())
            .collect();
        let rev_list = lines(&git(dir, &["rev-list", "--date-order", "HEAD"], None));
        assert_eq!(order, rev_list, "{}", dir.display());
        let full_paths: Vec<String> = files.iter().map(|f| full(f)).collect();
        let paths: Vec<&str> = full_paths.iter().map(String::as_str).collect();
        let mut args = vec!["log", "--format=%H", "HEAD", "--"];
        args.extend_from_slice(&paths);
        let shown: HashSet<String> = if paths.is_empty() {
            HashSet::new()
        } else {
            lines(&git(dir, &args, None)).into_iter().collect()
        };
        let want: Vec<(String, Vec<String>, usize)> = rev_list
            .iter()
            .filter(|c| shown.contains(*c))
            .filter_map(|c| {
                let pages: Vec<String> = changed_by(dir, c, &paths)
                    .into_iter()
                    .map(|p| p[pre.len()..].to_owned())
                    .collect();
                let more = pages.len().saturating_sub(RECENT_PAGES_PER_COMMIT);
                (!pages.is_empty()).then(|| {
                    (
                        c.clone(),
                        pages.into_iter().take(RECENT_PAGES_PER_COMMIT).collect(),
                        more,
                    )
                })
            })
            .take(RECENT_COMMITS)
            .collect();
        let times: HashMap<String, i64> =
            lines(&git(dir, &["log", "--format=%H %ct", "HEAD"], None))
                .iter()
                .map(|l| {
                    let (c, t) = l.split_once(' ').unwrap();
                    (c.to_owned(), t.parse().unwrap())
                })
                .collect();
        let mut want = want;
        want.sort_by(|a, b| (times[&b.0], &b.0).cmp(&(times[&a.0], &a.0)));
        let got: Vec<(String, Vec<String>, usize)> = w
            .history
            .recent
            .iter()
            .map(|r| (r.commit.id.clone(), r.files.clone(), r.more))
            .collect();
        assert_eq!(got, want, "recent changes of {}", dir.display());
        if let Some(f) = feed {
            let shown: HashSet<String> = lines(&git(
                dir,
                &["log", "--format=%H", "HEAD", "--", &full(f)],
                None,
            ))
            .into_iter()
            .collect();
            let want: Vec<(String, Vec<u8>)> = rev_list
                .iter()
                .filter(|c| shown.contains(*c))
                .filter_map(|c| {
                    let has = Command::new("git")
                        .current_dir(dir)
                        .args(["cat-file", "-e", &format!("{c}:{}", full(f))])
                        .env_clear()
                        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                        .env("HOME", dir)
                        .env("GIT_CONFIG_NOSYSTEM", "1")
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .stderr(Stdio::null())
                        .status()
                        .unwrap()
                        .success();
                    has.then(|| {
                        let b = git(dir, &["show", &format!("{c}:{}", full(f))], None);
                        (c.clone(), format!("{b}\n").into_bytes())
                    })
                })
                .collect();
            let got: Vec<(String, Vec<u8>)> = w
                .history
                .feed_versions
                .iter()
                .map(|(c, b)| (c.id.clone(), b.clone()))
                .collect();
            assert_eq!(got, want, "the feed's versions of {}", dir.display());
        }
    }

    #[test]
    fn the_history_is_gits_on_generated_histories() {
        // Merges, octopus and criss-cross merges, reverts, deletions and
        // clock skew: 80 histories of 30 steps, each from the root (a feed
        // at the top or nested) and as `--subtree d`.
        for seed in 0..80 {
            let dir = import("random", &random_history(seed, 30));
            let feed = if seed % 2 == 0 { "a.md" } else { "d/e/f.md" };
            agrees_with_git_in(&dir, None, Some(feed));
            agrees_with_git_in(&dir, Some("d"), Some("e/f.md"));
        }
    }

    #[test]
    fn a_merge_that_kept_one_sides_page_dates_it_by_that_side() {
        // The first review's P1/Q/P2/M: P1 changes the page, Q (a side
        // branch, newer) changes it too, P2 changes another page, and M
        // merges Q into P2 keeping P1's version. `git log -1 -- page` is
        // P1; a walk by date without pruning dated it by Q, whose change is
        // not in HEAD.
        let f = |v: u64| -> BTreeMap<String, String> {
            [("home.md", 0), ("page.md", v)]
                .into_iter()
                .map(|(p, v)| (p.to_owned(), body(p, v)))
                .collect()
        };
        let mut s = Vec::new();
        commit(&mut s, "main", 1, 1_700_000_100, &[], &f(0));
        commit(&mut s, "main", 2, 1_700_000_200, &[1], &f(1));
        commit(&mut s, "side", 3, 1_700_000_400, &[1], &f(2));
        let mut p2 = f(1);
        p2.insert("other.md".into(), body("other.md", 0));
        commit(&mut s, "main", 4, 1_700_000_300, &[2], &p2);
        commit(&mut s, "main", 5, 1_700_000_500, &[4, 3], &p2);
        let dir = import("p1qp2m", &s);
        let g = open(&dir).unwrap();
        let head = g.resolve("HEAD").unwrap();
        let files: BTreeSet<String> = ["home.md", "page.md", "other.md"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let w = g.history(head, None, &files, None).unwrap();
        let p1 = git(&dir, &["rev-parse", "main~2"], None);
        assert_eq!(w.history.last["page.md"].id, p1);
        agrees_with_git(&dir, Some("page.md"));
    }

    #[test]
    fn a_criss_cross_merge_dates_a_page_as_git_does() {
        let f = |v: u64| -> BTreeMap<String, String> {
            [("home.md", 0), ("page.md", v)]
                .into_iter()
                .map(|(p, v)| (p.to_owned(), body(p, v)))
                .collect()
        };
        let mut s = Vec::new();
        commit(&mut s, "main", 1, 1_700_000_100, &[], &f(0));
        commit(&mut s, "main", 2, 1_700_000_300, &[1], &f(1));
        commit(&mut s, "side", 3, 1_700_000_200, &[1], &f(2));
        commit(&mut s, "main", 4, 1_700_000_500, &[2, 3], &f(1));
        commit(&mut s, "side", 5, 1_700_000_400, &[3, 2], &f(2));
        commit(&mut s, "main", 6, 1_700_000_600, &[4, 5], &f(2));
        let dir = import("crisscross", &s);
        agrees_with_git(&dir, Some("page.md"));
    }

    #[test]
    fn the_walk_is_in_date_order_whatever_the_clocks_say() {
        // A parent newer than its children (clock skew): a walk by
        // committer time alone lists it before its second child.
        let f = |v: u64| -> BTreeMap<String, String> {
            [("home.md", v)]
                .into_iter()
                .map(|(p, v)| (p.to_owned(), body(p, v)))
                .collect()
        };
        let mut s = Vec::new();
        commit(&mut s, "main", 1, 1_700_000_100, &[], &f(0));
        commit(&mut s, "main", 2, 1_700_000_900, &[1], &f(1));
        commit(&mut s, "main", 3, 1_700_000_300, &[2], &f(2));
        commit(&mut s, "side", 4, 1_700_000_200, &[2], &f(3));
        commit(&mut s, "main", 5, 1_700_001_000, &[3, 4], &f(1));
        let dir = import("skew", &s);
        let g = open(&dir).unwrap();
        let head = g.resolve("HEAD").unwrap();
        let order: Vec<String> = g
            .date_order(head)
            .unwrap()
            .iter()
            .map(|i| i.to_hex().to_string())
            .collect();
        assert_eq!(
            order,
            lines(&git(&dir, &["rev-list", "--date-order", "HEAD"], None))
        );
        agrees_with_git(&dir, None);
    }

    #[test]
    fn recent_changes_keeps_fifty_pages_of_a_commit_and_counts_the_rest() {
        let mut files = BTreeMap::new();
        files.insert("home.md".to_owned(), body("home.md", 0));
        let mut s = Vec::new();
        commit(&mut s, "main", 1, 1_700_000_100, &[], &files);
        for i in 0..60 {
            let p = format!("p{i:02}.md");
            files.insert(p.clone(), body(&p, 0));
        }
        commit(&mut s, "main", 2, 1_700_000_200, &[1], &files);
        let dir = import("many", &s);
        let g = open(&dir).unwrap();
        let head = g.resolve("HEAD").unwrap();
        let names: BTreeSet<String> = files.keys().cloned().collect();
        let w = g.history(head, None, &names, None).unwrap();
        let newest = &w.history.recent[0];
        assert_eq!(newest.files.len(), RECENT_PAGES_PER_COMMIT);
        assert_eq!(newest.more, 10);
        assert_eq!(newest.files[0], "p00.md");
        agrees_with_git(&dir, None);
    }

    /// A repository whose commits each change `site/x.md` beside a root of
    /// `n` other entries.
    fn wide_root(n: usize, commits: usize) -> Scratch {
        let mut s = Vec::new();
        let mut files = BTreeMap::new();
        for i in 0..n {
            files.insert(format!("r{i:05}"), String::new());
        }
        files.insert("site/home.md".to_owned(), body("home.md", 0));
        for c in 0..commits {
            files.insert("site/x.md".to_owned(), body("x.md", c as u64));
            let parents: Vec<usize> = (c > 0).then_some(c).into_iter().collect();
            commit(
                &mut s,
                "main",
                c + 1,
                1_700_000_000 + c as i64,
                &parents,
                &files,
            );
        }
        import("wide", &s)
    }

    #[test]
    fn a_subtrees_path_from_the_root_counts_against_the_history_budget() {
        // The root's 2,000 entries, read on
        // the way to `site/` in each of 20 commits, are 20 roots of about
        // 50 KB: over a budget of 500,000 bytes, which the subtree's own
        // trees never reach.
        let dir = wide_root(2_000, 20);
        let g = open_with(&dir, 500_000).unwrap();
        let head = g.resolve("HEAD").unwrap();
        let files: BTreeSet<String> = ["home.md", "x.md"].into_iter().map(str::to_owned).collect();
        let w = g
            .history(head, Some("site"), &files, Some("home.md"))
            .unwrap();
        assert!(!w.history.complete, "the root's entries were not counted");
        assert!(w.why_incomplete.is_some());
        assert!(g.reads().charged <= 500_000, "{:?}", g.reads());
        // Within the default budget, it is complete.
        let g = open(&dir).unwrap();
        let w = g
            .history(head, Some("site"), &files, Some("home.md"))
            .unwrap();
        assert!(w.history.complete, "{:?}", w.why_incomplete);
    }

    #[test]
    fn a_subtrees_path_from_the_root_counts_against_the_repository_budget() {
        // the root's entries outside `--subtree` are entries
        // read, as the subtree's are.
        let dir = wide_root(2_000, 1);
        let g = open(&dir).unwrap();
        let head = g.resolve("HEAD").unwrap();
        let (tree, budget) = g
            .read_tree(head, Some("site"), &SiteConfig::default())
            .unwrap();
        assert_eq!(tree.files.len(), 2);
        assert!(budget.files >= 2_002, "{budget:?}");
    }

    #[test]
    fn a_feed_version_over_the_budget_is_never_read() {
        // the size comes from the object's header first.
        let mut s = Vec::new();
        let mut files = BTreeMap::new();
        files.insert("home.md".to_owned(), body("home.md", 0));
        files.insert("changelog.md".to_owned(), "## 0.1\n".repeat(20_000));
        commit(&mut s, "main", 1, 1_700_000_000, &[], &files);
        let dir = import("feedsize", &s);
        let g = open(&dir).unwrap();
        let head = g.resolve("HEAD").unwrap();
        let names: BTreeSet<String> = files.keys().cloned().collect();
        let small = Limits {
            feed_bytes: 1_000,
            ..Limits::DEFAULT
        };
        let w = g
            .history_within(head, None, &names, Some("changelog.md"), &small)
            .unwrap();
        assert!(w.history.feed_versions.is_empty());
        assert!(!w.history.complete);
        assert_eq!(g.blob_bytes_read(), 0, "a version was read whole");
    }

    /// A tree object of `n` entries named by 1,000 bytes each (one blob):
    /// `n` × 1,028 bytes, made with `git mktree`.
    fn wide_tree(dir: &Path, n: usize) -> String {
        let blob = git(dir, &["hash-object", "-w", "--stdin"], Some(b"x"));
        let mut listing = String::new();
        for i in 0..n {
            listing.push_str(&format!("100644 blob {blob}\t{i:06}{}\n", "n".repeat(994)));
        }
        git(dir, &["mktree"], Some(listing.as_bytes()))
    }

    #[test]
    fn a_tree_larger_than_the_budget_is_refused_by_its_header_unread() {
        // 25,000 entries of 1,028 bytes are 25.7 MB of tree,
        // over MAX_REPO_TREE_BYTES (23.8 MB): refused by the object's
        // header before it is read or decoded.
        let dir = scratch("widetree");
        git(&dir, &["init", "-q", "-b", "main"], None);
        let big = wide_tree(&dir, 25_000);
        let home = git(
            &dir,
            &["hash-object", "-w", "--stdin"],
            Some(b"---\ntitle: H\n---\n"),
        );
        let root = git(
            &dir,
            &["mktree"],
            Some(format!("040000 tree {big}\tbig\n100644 blob {home}\thome.md\n").as_bytes()),
        );
        let c = git(&dir, &["commit-tree", &root, "-m", "wide"], None);
        let g = open(&dir).unwrap();
        let (_, budget) = g
            .read_tree(
                ObjectId::from_hex(c.as_bytes()).unwrap(),
                None,
                &SiteConfig::default(),
            )
            .unwrap();
        assert!(!budget.holds());
        assert!(
            matches!(
                budget.findings()[..],
                [rubrica_site::Finding::RepositoryOverBudget {
                    what: rubrica_site::Budget::TreeBytes,
                    ..
                }]
            ),
            "{:?}",
            budget.findings()
        );
        assert!(
            g.tree_bytes_read() < 1_000,
            "{} bytes of tree read",
            g.tree_bytes_read()
        );
        // The history walk stops there too, unread: the big tree is a
        // parent's.
        let home2 = git(
            &dir,
            &["hash-object", "-w", "--stdin"],
            Some(b"---\ntitle: H2\n---\n"),
        );
        let root2 = git(
            &dir,
            &["mktree"],
            Some(format!("040000 tree {big}\tbig\n100644 blob {home2}\thome.md\n").as_bytes()),
        );
        let c2 = git(&dir, &["commit-tree", &root2, "-p", &c, "-m", "two"], None);
        let g = open(&dir).unwrap();
        let files: BTreeSet<String> = ["home.md".to_owned()].into_iter().collect();
        let w = g
            .history(
                ObjectId::from_hex(c2.as_bytes()).unwrap(),
                None,
                &files,
                None,
            )
            .unwrap();
        // `big/` holds no wanted path, so it is never entered: complete.
        assert!(w.history.complete, "{:?}", w.why_incomplete);
        assert!(g.tree_bytes_read() < 1_000);
    }

    #[test]
    fn a_root_tree_larger_than_the_budget_stops_the_history_unread() {
        // In the history: a root over the per-tree bound.
        let dir = scratch("wideroot");
        git(&dir, &["init", "-q", "-b", "main"], None);
        let blob = git(&dir, &["hash-object", "-w", "--stdin"], Some(b"x"));
        let home = git(
            &dir,
            &["hash-object", "-w", "--stdin"],
            Some(b"---\ntitle: H\n---\n"),
        );
        let mut listing = String::new();
        for i in 0..25_000 {
            listing.push_str(&format!("100644 blob {blob}\t{i:06}{}\n", "n".repeat(994)));
        }
        listing.push_str(&format!("100644 blob {home}\thome.md\n"));
        let root = git(&dir, &["mktree"], Some(listing.as_bytes()));
        let c1 = git(&dir, &["commit-tree", &root, "-m", "wide"], None);
        let small = git(
            &dir,
            &["mktree"],
            Some(format!("100644 blob {home}\thome.md\n").as_bytes()),
        );
        let c2 = git(
            &dir,
            &["commit-tree", &small, "-p", &c1, "-m", "small"],
            None,
        );
        let g = open(&dir).unwrap();
        let files: BTreeSet<String> = ["home.md".to_owned()].into_iter().collect();
        let w = g
            .history(
                ObjectId::from_hex(c2.as_bytes()).unwrap(),
                None,
                &files,
                None,
            )
            .unwrap();
        assert!(!w.history.complete);
        assert!(w.history.last.is_empty(), "{:?}", w.history.last);
        assert!(g.tree_bytes_read() < 1_000, "{}", g.tree_bytes_read());
    }

    // ---------- one object budget ----------

    /// Objects written straight into a repository's object store (gix, in
    /// process), so a test can make what git's porcelain will not: a mode
    /// `100664`, names of any length, a blob and a tree of one name in one
    /// tree, a commit naming a parent twice.
    struct Odb {
        repo: gix::Repository,
        // After `repo`: dropped, so removed, after it.
        dir: Scratch,
    }

    type Files = BTreeMap<String, (&'static str, String)>;

    impl Odb {
        fn new(tag: &str) -> Odb {
            let dir = scratch(tag);
            git(&dir, &["init", "-q", "-b", "main"], None);
            let repo = gix::open_opts(&*dir, gix::open::Options::isolated()).unwrap();
            Odb { dir, repo }
        }

        fn write(&self, kind: gix::object::Kind, data: &[u8]) -> ObjectId {
            use gix::objs::Write as _;
            self.repo.objects.write_buf(kind, data).unwrap()
        }

        fn blob(&self, data: &[u8]) -> ObjectId {
            self.write(gix::object::Kind::Blob, data)
        }

        /// A tree of exactly `entries` (mode, name, id), in the order given.
        fn raw_tree(&self, entries: &[(&str, &[u8], ObjectId)]) -> ObjectId {
            let mut raw = Vec::new();
            for (mode, name, id) in entries {
                raw.extend_from_slice(mode.as_bytes());
                raw.push(b' ');
                raw.extend_from_slice(name);
                raw.push(0);
                raw.extend_from_slice(id.as_bytes());
            }
            self.write(gix::object::Kind::Tree, &raw)
        }

        /// The tree of `files`, nested, in git's order (a tree sorts as its
        /// name and a `/`).
        fn tree(&self, files: &Files, prefix: &str) -> ObjectId {
            let mut entries: Vec<(Vec<u8>, &str, Vec<u8>, ObjectId)> = Vec::new();
            let mut dirs = BTreeSet::new();
            for (path, (mode, body)) in files.range(prefix.to_owned()..) {
                let Some(rest) = path.strip_prefix(prefix) else {
                    break;
                };
                match rest.split_once('/') {
                    Some((d, _)) => {
                        dirs.insert(d.to_owned());
                    }
                    None => entries.push((
                        rest.as_bytes().to_vec(),
                        mode,
                        rest.as_bytes().to_vec(),
                        self.blob(body.as_bytes()),
                    )),
                }
            }
            for d in dirs {
                let id = self.tree(files, &format!("{prefix}{d}/"));
                entries.push((format!("{d}/").into_bytes(), "40000", d.into_bytes(), id));
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let e: Vec<(&str, &[u8], ObjectId)> = entries
                .iter()
                .map(|(_, m, n, i)| (*m, n.as_slice(), *i))
                .collect();
            self.raw_tree(&e)
        }

        fn commit(&self, tree: ObjectId, parents: &[ObjectId], time: i64, msg: &[u8]) -> ObjectId {
            let mut raw = format!("tree {}\n", tree.to_hex()).into_bytes();
            for p in parents {
                raw.extend_from_slice(format!("parent {}\n", p.to_hex()).as_bytes());
            }
            raw.extend_from_slice(
                format!("author T <t@e.invalid> {time} +0000\ncommitter T <t@e.invalid> {time} +0000\n\n")
                    .as_bytes(),
            );
            raw.extend_from_slice(msg);
            self.write(gix::object::Kind::Commit, &raw)
        }

        fn head(&self, c: ObjectId) {
            git(
                &self.dir,
                &["update-ref", "refs/heads/main", &c.to_hex().to_string()],
                None,
            );
        }

        /// `specs` written as commits; HEAD the head they name.
        fn specs(&self, specs: &[Spec]) {
            let mut ids: HashMap<usize, ObjectId> = HashMap::new();
            for sp in specs.iter().filter(|s| s.mark != 0) {
                let tree = self.tree(&sp.files, "");
                let parents: Vec<ObjectId> = sp.parents.iter().map(|m| ids[m]).collect();
                let c = self.commit(
                    tree,
                    &parents,
                    sp.time,
                    format!("c{}\n", sp.mark).as_bytes(),
                );
                ids.insert(sp.mark, c);
            }
            self.head(ids[&head_mark(specs)]);
        }
    }

    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn hex(id: ObjectId) -> String {
        id.to_hex().to_string()
    }

    #[test]
    fn every_object_read_goes_through_one_function() {
        // one function charges and inflates every object. Outside
        // the module that holds the repository nothing names gix's
        // readers, and in it there is one inflation. A scan of this file's
        // source text: it sees the names below, not what
        // they resolve to, so a reader reached another way (a trait
        // method of another name, a macro, another file) is not caught.
        let src = include_str!("git.rs");
        let code: String = src[..src.find("#[cfg(test)]\nmod tests").unwrap()]
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let at = code.find("\nmod objects {").expect("a module `objects`");
        let len = code[at..].find("\n}\n").expect("its end");
        let (inside, outside) = (
            &code[at..at + len],
            format!("{}{}", &code[..at], &code[at + len..]),
        );
        for word in [
            "find_object",
            "find_commit",
            "find_tree",
            "find_blob",
            "find_tag",
            "try_find",
            "find_header",
            "peel_to",
            "head_id",
            "head_commit",
            "head_tree",
            "rev_parse",
            "find_reference",
            "lookup_prefix",
            ".objects",
            "gix::open",
            "gix::Repository",
        ] {
            assert!(
                !outside.contains(word),
                "`{word}` outside the objects module"
            );
        }
        for word in [
            "find_object",
            "find_commit",
            "find_tree",
            "find_blob",
            "find_tag",
            "peel_to",
            "head_id",
            "head_commit",
            "head_tree",
            "rev_parse",
        ] {
            assert!(!inside.contains(word), "`{word}` in the objects module");
        }
        assert_eq!(inside.matches("try_find(").count(), 1, "one inflation");
    }

    #[test]
    fn repeated_parents_are_diffed_once() {
        // repro 1 scaled: a commit naming its parent P 2,000
        // times, P's root holding an entry with a 256 KiB name. Each repeat
        // was diffed again: 2,000 reads of P's root (the review's 20,000
        // and 20 MB took 108 s from a 124 KB repository).
        let o = Odb::new("parents");
        let h1 = o.blob(b"---\ntitle: H\n---\n# 1\n");
        let h2 = o.blob(b"---\ntitle: H\n---\n# 2\n");
        let x = o.blob(b"x");
        let big = vec![b'n'; 256 << 10];
        let p_root = o.raw_tree(&[("100644", b"home.md", h1), ("100644", &big, x)]);
        let p = o.commit(p_root, &[], 1_700_000_000, b"p\n");
        let c_root = o.raw_tree(&[("100644", b"home.md", h2)]);
        let c = o.commit(c_root, &vec![p; 2_000], 1_700_000_100, b"c\n");
        o.head(c);
        let g = open(&o.dir).unwrap();
        let w = g.history(c, None, &set(&["home.md"]), None).unwrap();
        assert!(w.history.complete, "{:?}", w.why_incomplete);
        assert_eq!(w.history.last["home.md"].id, hex(c));
        let r = g.reads();
        assert_eq!(r.trees, 2, "each root once: {r:?}");
        assert_eq!(r.commits, 2, "each commit once: {r:?}");
        // C against P once, and P's root against nothing: whatever is kept.
        assert_eq!(g.compared(), 2);
        assert_eq!(r.inflated, r.charged, "{r:?}");
    }

    /// `n` linear commits, each root holding `home.md` (never changed) and
    /// an entry of its own with a `name`-byte name; then HEAD, `home.md`
    /// alone. Returns the repository, HEAD and the roots' bytes.
    fn distinct_roots(n: usize, name: usize) -> (Odb, ObjectId, u64) {
        let o = Odb::new("roots");
        let h = o.blob(b"---\ntitle: H\n---\n# H\n");
        let x = o.blob(b"x");
        let mut parent = Vec::new();
        let mut bytes = 0;
        for i in 0..n {
            let mut nm = format!("{i:04}").into_bytes();
            nm.resize(name, b'n');
            let t = o.raw_tree(&[("100644", &nm, x), ("100644", b"home.md", h)]);
            bytes += (2 * 28 + nm.len() + 7) as u64;
            let c = o.commit(t, &parent, 1_700_000_000 + i as i64, b"c\n");
            parent = vec![c];
        }
        let t = o.raw_tree(&[("100644", b"home.md", h)]);
        bytes += 35;
        let c = o.commit(t, &parent, 1_700_000_000 + n as i64, b"head\n");
        o.head(c);
        (o, c, bytes)
    }

    #[test]
    fn each_tree_is_read_once_in_a_linear_history() {
        // repro 2 scaled, and L-B: each commit's root was read
        // twice, as a child's base and as its own; now each tree is read
        // once, its wanted entries kept by its id.
        let (o, head, bytes) = distinct_roots(50, 64 << 10);
        let g = open(&o.dir).unwrap();
        let w = g.history(head, None, &set(&["home.md"]), None).unwrap();
        assert!(w.history.complete, "{:?}", w.why_incomplete);
        let r = g.reads();
        assert_eq!(r.trees, 51, "{r:?}");
        assert_eq!(r.tree_bytes, bytes, "{r:?}");
        assert_eq!(r.commits, 51, "{r:?}");
        assert_eq!(r.inflated, r.charged, "{r:?}");
    }

    #[test]
    fn the_history_stops_at_the_object_budget() {
        // repro 2 scaled: 51 roots of 64 KiB are 3.3 MB, over
        // a budget of 1 MiB: the walk stops there, with a warning, having
        // read no more than the budget (the review's 200 roots of 20 MB
        // took 2.2 s, and 18 minutes at the commit bound).
        let (o, head, _) = distinct_roots(50, 64 << 10);
        let g = open_with(&o.dir, 1 << 20).unwrap();
        let w = g.history(head, None, &set(&["home.md"]), None).unwrap();
        assert!(!w.history.complete);
        let why = w.why_incomplete.unwrap();
        assert!(why.contains("budget"), "{why}");
        assert!(
            w.history.last.is_empty(),
            "home.md's change is past the budget"
        );
        let r = g.reads();
        assert!(r.charged <= 1 << 20, "{r:?}");
        assert_eq!(r.inflated, r.charged, "{r:?}");
    }

    #[test]
    fn the_pages_sources_are_set_aside_from_the_history() {
        // The build reads each page's source again at the end (its `.md`
        // copy): the bytes the tree read are set aside, so a history that
        // spends the rest of the budget cannot leave the build without
        // them. A 200 KiB page, roots of 64 KiB, and a budget that runs out
        // halfway through the history.
        let o = Odb::new("reserve");
        let page = format!("---\ntitle: H\n---\n{}\n", "x".repeat(200 << 10));
        let h = o.blob(page.as_bytes());
        let x = o.blob(b"x");
        let mut parent = Vec::new();
        for i in 0..20 {
            let mut nm = format!("{i:04}").into_bytes();
            nm.resize(64 << 10, b'n');
            let t = o.raw_tree(&[("100644", &nm, x), ("100644", b"home.md", h)]);
            parent = vec![o.commit(t, &parent, 1_700_000_000 + i, b"c\n")];
        }
        let head = o.commit(
            o.raw_tree(&[("100644", b"home.md", h)]),
            &parent,
            1_700_000_100,
            b"h\n",
        );
        o.head(head);
        let files = set(&["home.md"]);
        let cfg = SiteConfig::default();
        let g = open(&o.dir).unwrap();
        g.read_tree(head, None, &cfg).unwrap();
        let tree_read = g.reads().charged;
        g.history(head, None, &files, None).unwrap();
        let history = g.reads().charged - tree_read;
        let budget = tree_read + history / 2;
        let g = open_with(&o.dir, budget).unwrap();
        let (_, b) = g.read_tree(head, None, &cfg).unwrap();
        assert!(b.holds());
        let w = g.history(head, None, &files, None).unwrap();
        assert!(!w.history.complete);
        g.blob(&hex(h))
            .expect("the page's source, from the bytes set aside");
        let r = g.reads();
        assert!(r.charged <= budget, "{r:?}");
        assert_eq!(r.inflated, r.charged, "{r:?}");
    }

    #[test]
    fn a_tree_over_the_object_budget_refuses_the_build() {
        // In the tree built, the budget running out refuses the build.
        let (o, _, _) = distinct_roots(1, 64 << 10);
        let first =
            ObjectId::from_hex(git(&o.dir, &["rev-parse", "main~1"], None).as_bytes()).unwrap();
        let g = open_with(&o.dir, 32 << 10).unwrap();
        let (_, budget) = g.read_tree(first, None, &SiteConfig::default()).unwrap();
        assert!(
            matches!(
                budget.findings()[..],
                [rubrica_site::Finding::RepositoryOverBudget {
                    what: rubrica_site::Budget::ObjectBytes,
                    ..
                }]
            ),
            "{:?}",
            budget.findings()
        );
        let r = g.reads();
        assert!(r.charged <= 32 << 10 && r.trees == 0, "{r:?}");
    }

    #[test]
    fn a_commit_over_its_cap_is_never_inflated() {
        // scaled: one old commit with a 2 MiB message (the
        // review's 1 GB gave 1.0 GB of RSS from a 1.0 MB repository).
        let o = Odb::new("bigcommit");
        let h1 = o.blob(b"---\ntitle: H\n---\n# 1\n");
        let h2 = o.blob(b"---\ntitle: H\n---\n# 2\n");
        let a = o.blob(b"---\ntitle: A\n---\n# A\n");
        let h3 = o.blob(b"---\ntitle: H\n---\n# 3\n");
        let msg = vec![b'a'; 2 << 20];
        let r = o.commit(
            o.raw_tree(&[("100644", b"a.md", a), ("100644", b"home.md", h1)]),
            &[],
            1_700_000_000,
            &msg,
        );
        let m = o.commit(
            o.raw_tree(&[("100644", b"a.md", a), ("100644", b"home.md", h2)]),
            &[r],
            1_700_000_050,
            b"m\n",
        );
        let c = o.commit(
            o.raw_tree(&[("100644", b"a.md", a), ("100644", b"home.md", h3)]),
            &[m],
            1_700_000_100,
            b"c\n",
        );
        o.head(c);
        let g = open(&o.dir).unwrap();
        let w = g
            .history(c, None, &set(&["a.md", "home.md"]), None)
            .unwrap();
        assert!(!w.history.complete);
        let why = w.why_incomplete.unwrap();
        assert!(
            why.contains(&r.to_hex().to_string()) && why.contains("bytes"),
            "{why}"
        );
        assert_eq!(w.history.last["home.md"].id, hex(c));
        assert!(!w.history.last.contains_key("a.md"));
        let reads = g.reads();
        assert!(reads.commit_bytes < MAX_COMMIT_BYTES, "{reads:?}");
        assert_eq!(reads.inflated, reads.charged, "{reads:?}");
        // As the commit built, it refuses the build, unread.
        let g = open(&o.dir).unwrap();
        let (_, budget) = g.read_tree(r, None, &SiteConfig::default()).unwrap();
        assert!(
            matches!(
                budget.findings()[..],
                [rubrica_site::Finding::RepositoryOverBudget {
                    what: rubrica_site::Budget::CommitBytes,
                    ..
                }]
            ),
            "{:?}",
            budget.findings()
        );
        assert_eq!(g.reads().commits, 0, "{:?}", g.reads());
        assert_eq!(MAX_COMMIT_BYTES, 1 << 20);
    }

    #[test]
    fn each_commit_is_read_once() {
        // the walk read each commit twice (its parents, then
        // its time), the tree read once more, and each date once more.
        let mut s = Vec::new();
        let mut files = BTreeMap::new();
        files.insert("a.md".to_owned(), body("a.md", 0));
        for c in 0..20 {
            files.insert("home.md".to_owned(), body("home.md", c as u64));
            let parents: Vec<usize> = (c > 0).then_some(c).into_iter().collect();
            commit(
                &mut s,
                "main",
                c + 1,
                1_700_000_000 + c as i64,
                &parents,
                &files,
            );
        }
        let dir = import("once", &s);
        let g = open(&dir).unwrap();
        let head = g.resolve("HEAD").unwrap();
        let (tree, budget) = g.read_tree(head, None, &SiteConfig::default()).unwrap();
        assert!(budget.holds() && tree.files.len() == 2);
        let w = g
            .history(head, None, &set(&["a.md", "home.md"]), None)
            .unwrap();
        assert!(w.history.complete);
        let r = g.reads();
        assert_eq!(r.commits, 20, "{r:?}");
        assert_eq!(r.inflated, r.charged, "{r:?}");
    }

    #[test]
    fn a_mode_git_reads_as_the_same_changes_nothing() {
        // Git compares modes canonically (`100664` is
        // `100644`), so a commit that changed only that is no change.
        let o = Odb::new("modes");
        let f = |home: u64, q: &'static str, p: &'static str| -> Files {
            [
                ("home.md", ("100644", body("home.md", home))),
                ("q.md", (q, body("q.md", 0))),
                ("p.md", (p, body("p.md", 0))),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect()
        };
        let c1 = o.commit(
            o.tree(&f(0, "100644", "100644"), ""),
            &[],
            1_700_000_100,
            b"1\n",
        );
        let c2 = o.commit(
            o.tree(&f(0, "100664", "100664"), ""),
            &[c1],
            1_700_000_200,
            b"2\n",
        );
        let c3 = o.commit(
            o.tree(&f(1, "100664", "100755"), ""),
            &[c2],
            1_700_000_300,
            b"3\n",
        );
        o.head(c3);
        let g = open(&o.dir).unwrap();
        let w = g
            .history(c3, None, &set(&["home.md", "p.md", "q.md"]), None)
            .unwrap();
        assert_eq!(w.history.last["q.md"].id, hex(c1));
        assert_eq!(w.history.last["p.md"].id, hex(c3));
        assert_eq!(
            git(
                &o.dir,
                &["log", "-1", "--format=%H", "HEAD", "--", "q.md"],
                None
            ),
            hex(c1)
        );
        agrees_with_git(&o.dir, Some("q.md"));
    }

    #[test]
    fn the_history_is_gits_on_generated_histories_with_modes() {
        // As the_history_is_gits_on_generated_histories, an edit now and
        // then a page's mode alone: 100644, the legacy 100664, 100755.
        for seed in 0..40 {
            let o = Odb::new("random-modes");
            o.specs(&random_specs(seed, 30, true));
            let feed = if seed % 2 == 0 { "a.md" } else { "d/e/f.md" };
            agrees_with_git_in(&o.dir, None, Some(feed));
            agrees_with_git_in(&o.dir, Some("d"), Some("e/f.md"));
        }
    }

    #[test]
    fn a_page_beside_a_directory_of_its_name_is_not_changed_by_the_directory() {
        // A tree holding a blob `pzed.md` and a tree
        // `pzed.md` (which git's porcelain never writes): a commit that
        // changed only a file under the directory is no change of the page.
        let o = Odb::new("pzed");
        let h = o.blob(b"---\ntitle: H\n---\n# H\n");
        let page = o.blob(b"---\ntitle: P\n---\n# P\n");
        let mut ids = Vec::new();
        for v in [&b"# 1\n"[..], b"# 2\n"] {
            let x = o.blob(v);
            let sub = o.raw_tree(&[("100644", b"x.md", x)]);
            let root = o.raw_tree(&[
                ("100644", b"home.md", h),
                ("100644", b"pzed.md", page),
                ("40000", b"pzed.md", sub),
            ]);
            let parents: Vec<ObjectId> = ids.last().copied().into_iter().collect();
            ids.push(o.commit(root, &parents, 1_700_000_000 + ids.len() as i64, b"c\n"));
        }
        o.head(ids[1]);
        let g = open(&o.dir).unwrap();
        let files = set(&["home.md", "pzed.md", "pzed.md/x.md"]);
        let w = g.history(ids[1], None, &files, None).unwrap();
        assert_eq!(w.history.last["pzed.md"].id, hex(ids[0]));
        assert_eq!(w.history.last["pzed.md/x.md"].id, hex(ids[1]));
        assert_eq!(w.history.recent[0].files, ["pzed.md/x.md"]);
    }

    #[test]
    fn recent_changes_are_newest_first_ties_by_the_greater_id() {
        // decided: not git's order over the simplified graph,
        // but committer time, newest first, ties by the greater id.
        let o = Odb::new("ties");
        let blob = |s: &str| o.blob(format!("---\ntitle: {s}\n---\n# {s}\n").as_bytes());
        let (h, a, b) = (blob("H"), blob("A"), blob("B"));
        let root = o.commit(
            o.raw_tree(&[("100644", b"home.md", h)]),
            &[],
            1_700_000_000,
            b"r\n",
        );
        // A parent and a child of one second, the parent's id the greater.
        let (c1, c2) = (0..)
            .find_map(|i| {
                let c1 = o.commit(
                    o.raw_tree(&[("100644", b"a.md", a), ("100644", b"home.md", h)]),
                    &[root],
                    1_700_000_100,
                    format!("a {i}\n").as_bytes(),
                );
                let c2 = o.commit(
                    o.raw_tree(&[
                        ("100644", b"a.md", a),
                        ("100644", b"b.md", b),
                        ("100644", b"home.md", h),
                    ]),
                    &[c1],
                    1_700_000_100,
                    b"b\n",
                );
                (c1 > c2).then_some((c1, c2))
            })
            .unwrap();
        o.head(c2);
        let g = open(&o.dir).unwrap();
        let w = g
            .history(c2, None, &set(&["a.md", "b.md", "home.md"]), None)
            .unwrap();
        let got: Vec<String> = w
            .history
            .recent
            .iter()
            .map(|r| r.commit.id.clone())
            .collect();
        assert_eq!(got, [hex(c1), hex(c2), hex(root)]);
    }

    // ---------- rebuilding an object from its delta chain ----------

    /// `git` in `dir`, its output as bytes ([`git`]'s).
    fn git_bytes(dir: &Path, args: &[&str], stdin: &[u8]) -> Vec<u8> {
        use std::io::Write as _;
        let mut child = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("git");
        let mut input = child.stdin.take().unwrap();
        let feed = std::thread::scope(|t| {
            let w = t.spawn(move || input.write_all(stdin));
            let out = child.wait_with_output().unwrap();
            w.join().unwrap().unwrap();
            out
        });
        assert!(
            feed.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&feed.stderr)
        );
        feed.stdout
    }

    /// A pack entry's header: its type and size.
    fn entry_header(kind: u8, mut size: usize) -> Vec<u8> {
        let mut b = (kind << 4) | (size & 15) as u8;
        size >>= 4;
        let mut out = Vec::new();
        while size > 0 {
            out.push(b | 0x80);
            b = (size & 0x7f) as u8;
            size >>= 7;
        }
        out.push(b);
        out
    }

    /// `data` as a zlib stream of stored (uncompressed) blocks.
    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        let mut chunks: Vec<&[u8]> = data.chunks(0xffff).collect();
        if chunks.is_empty() {
            chunks.push(&[]);
        }
        let last = chunks.len() - 1;
        for (i, c) in chunks.iter().enumerate() {
            out.push(u8::from(i == last));
            let n = c.len() as u16;
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&(!n).to_le_bytes());
            out.extend_from_slice(c);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + u32::from(x)) % 65521;
            b = (b + a) % 65521;
        }
        out.extend_from_slice(&((b << 16) | a).to_be_bytes());
        out
    }

    fn blob_id(data: &[u8]) -> ObjectId {
        let mut h = gix::hash::hasher(gix::hash::Kind::Sha1);
        h.update(format!("blob {}\0", data.len()).as_bytes());
        h.update(data);
        h.try_finalize().unwrap()
    }

    /// A ref-delta entry rebuilding `body` from `base` (of `base_len`
    /// bytes): insertions only, so the base's bytes are never copied, but
    /// the whole base must be there to apply it.
    fn ref_delta(base: ObjectId, base_len: usize, body: &[u8]) -> Vec<u8> {
        let varint = |mut n: usize, out: &mut Vec<u8>| loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        };
        let mut d = Vec::new();
        varint(base_len, &mut d);
        varint(body.len(), &mut d);
        for c in body.chunks(127) {
            d.push(c.len() as u8);
            d.extend_from_slice(c);
        }
        let mut e = entry_header(7, d.len());
        e.extend_from_slice(base.as_bytes());
        e.extend_from_slice(&zlib(&d));
        e
    }

    /// A blob entry, whole.
    fn blob_entry(body: &[u8]) -> Vec<u8> {
        let mut e = entry_header(3, body.len());
        e.extend_from_slice(&zlib(body));
        e
    }

    /// The entry of a blob of `len` zeros, compressed (by git: a few
    /// kilobytes for 80 MiB), and its id.
    fn zeros_entry(len: usize) -> (ObjectId, Vec<u8>) {
        let d = scratch("zeros");
        git(&d, &["init", "-q"], None);
        let zeros = vec![0u8; len];
        let id = git_bytes(&d, &["hash-object", "-w", "--stdin"], &zeros);
        let id = String::from_utf8(id).unwrap();
        let pack = git_bytes(
            &d,
            &["pack-objects", "--stdout", "-q"],
            format!("{}\n", id.trim()).as_bytes(),
        );
        assert_eq!(&pack[8..12], &1u32.to_be_bytes(), "one entry");
        (
            ObjectId::from_hex(id.trim().as_bytes()).unwrap(),
            pack[12..pack.len() - 20].to_vec(),
        )
    }

    /// `entries` written as a pack into `dir`'s objects, indexed by git.
    fn add_pack(dir: &Path, entries: &[Vec<u8>]) {
        static N: AtomicUsize = AtomicUsize::new(0);
        let mut pack = b"PACK".to_vec();
        pack.extend_from_slice(&2u32.to_be_bytes());
        pack.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for e in entries {
            pack.extend_from_slice(e);
        }
        let mut h = gix::hash::hasher(gix::hash::Kind::Sha1);
        h.update(&pack);
        pack.extend_from_slice(h.try_finalize().unwrap().as_bytes());
        let p = dir.join(format!(
            ".git/objects/pack/crafted-{}.pack",
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&p, &pack).unwrap();
        git(dir, &["index-pack", p.to_str().unwrap()], None);
    }

    /// A page's source.
    fn page(title: &str) -> Vec<u8> {
        format!("---\ntitle: {title}\n---\n# {title}\n").into_bytes()
    }

    /// What every read made: nothing gix allocated for one object went
    /// over the limit, and every byte inflated was charged first.
    fn bounded(r: &Reads) {
        assert!(r.largest_buffer <= MAX_ALLOC_BYTES as u64, "{r:?}");
        assert!(r.inflated <= r.charged, "{r:?}");
    }

    /// 80 MiB: over the allocation limit by itself.
    const OVER: usize = MAX_ALLOC_BYTES + (16 << 20);

    #[test]
    fn a_page_delta_compressed_against_a_base_over_the_limit_refuses_the_build() {
        // H (scaled): pages each a ref-delta against
        // one blob of zeros. Each read rebuilt the whole base, uncharged
        // (the review's 300 pages against 1 GiB: 47 KB charged, 33.8 s,
        // 2.16 GB). Now gix refuses the first page before allocating, and
        // the build is refused, naming it.
        let o = Odb::new("crafted");
        let (base, base_entry) = zeros_entry(OVER);
        let home = o.blob(&page("H"));
        let pages: Vec<Vec<u8>> = (0..4).map(|i| page(&format!("P{i}"))).collect();
        let ids: Vec<ObjectId> = pages.iter().map(|p| blob_id(p)).collect();
        let mut entries = vec![base_entry];
        entries.extend(pages.iter().map(|p| ref_delta(base, OVER, p)));
        add_pack(&o.dir, &entries);
        let names: Vec<String> = (0..4).map(|i| format!("p{i}.md")).collect();
        let mut tree: Vec<(&str, &[u8], ObjectId)> = vec![("100644", b"home.md", home)];
        tree.extend(
            names
                .iter()
                .zip(&ids)
                .map(|(n, &i)| ("100644", n.as_bytes(), i)),
        );
        let c = o.commit(o.raw_tree(&tree), &[], 1_700_000_000, b"c\n");
        o.head(c);
        let g = open(&o.dir).unwrap();
        let (_, budget) = g.read_tree(c, None, &SiteConfig::default()).unwrap();
        let object: Arc<str> = format!("blob {} (p0.md)", hex(ids[0])).into();
        assert_eq!(
            budget.findings(),
            [rubrica_site::Finding::RepositoryOverBudget {
                what: Budget::DeltaBase { object },
                at_least: MAX_ALLOC_BYTES as u64 + 1,
                limit: MAX_ALLOC_BYTES as u64,
            }]
        );
        let r = g.reads();
        assert_eq!(r.blobs, 1, "home.md alone: {r:?}");
        bounded(&r);
    }

    #[test]
    fn a_feed_version_that_cannot_be_rebuilt_stops_the_history() {
        // The same shape in the history: the feed's older version is a
        // delta against the 80 MiB base. The walk stops there with a
        // warning, never an error (exit 2).
        let o = Odb::new("crafted-feed");
        let (base, base_entry) = zeros_entry(OVER);
        let old = page("Feed 1");
        add_pack(&o.dir, &[base_entry, ref_delta(base, OVER, &old)]);
        let home = o.blob(&page("H"));
        let new = o.blob(&page("Feed 2"));
        let c1 = o.commit(
            o.raw_tree(&[
                ("100644", b"feed.md", blob_id(&old)),
                ("100644", b"home.md", home),
            ]),
            &[],
            1_700_000_000,
            b"1\n",
        );
        let c2 = o.commit(
            o.raw_tree(&[("100644", b"feed.md", new), ("100644", b"home.md", home)]),
            &[c1],
            1_700_000_100,
            b"2\n",
        );
        o.head(c2);
        let g = open(&o.dir).unwrap();
        let w = g
            .history(c2, None, &set(&["feed.md", "home.md"]), Some("feed.md"))
            .unwrap();
        assert!(!w.history.complete);
        let why = w.why_incomplete.unwrap();
        assert!(
            why.contains(&hex(blob_id(&old)))
                && why.contains("cannot be rebuilt within the memory limit")
                && why.contains("(too large, or the pack is damaged: `git fsck` tells which)"),
            "{why}"
        );
        assert_eq!(w.history.feed_versions.len(), 1);
        bounded(&g.reads());
    }

    #[test]
    fn a_delta_chain_deeper_than_git_writes_is_refused_from_its_header() {
        // gix sets no depth; git writes at most 4095. A chain of 4096
        // deltas is refused by its header, unread; one of 4095 is read.
        let o = Odb::new("deep");
        let v = |i: usize| page(&format!("V{i}"));
        let mut entries = vec![blob_entry(&v(0))];
        for i in 1..=MAX_DELTA_DEPTH as usize + 1 {
            entries.push(ref_delta(blob_id(&v(i - 1)), v(i - 1).len(), &v(i)));
        }
        add_pack(&o.dir, &entries);
        let (ok, deep) = (
            blob_id(&v(MAX_DELTA_DEPTH as usize)),
            blob_id(&v(MAX_DELTA_DEPTH as usize + 1)),
        );
        let home = o.blob(&page("H"));
        let c = o.commit(
            o.raw_tree(&[
                ("100644", b"a.md", ok),
                ("100644", b"b.md", deep),
                ("100644", b"home.md", home),
            ]),
            &[],
            1_700_000_000,
            b"c\n",
        );
        o.head(c);
        let g = open(&o.dir).unwrap();
        let (tree, budget) = g.read_tree(c, None, &SiteConfig::default()).unwrap();
        assert_eq!(
            budget.findings(),
            [rubrica_site::Finding::RepositoryOverBudget {
                what: Budget::DeltaDepth {
                    object: format!("blob {} (b.md)", hex(deep)).into()
                },
                at_least: u64::from(MAX_DELTA_DEPTH) + 1,
                limit: u64::from(MAX_DELTA_DEPTH),
            }]
        );
        assert_eq!(tree.files[0].bytes, v(MAX_DELTA_DEPTH as usize));
        let r = g.reads();
        assert_eq!(r.blobs, 1, "a.md alone: {r:?}");
        bounded(&r);
    }

    /// `n` bytes of text, lines of words.
    fn text(n: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(n + 80);
        let mut x: u32 = 6;
        while out.len() < n {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let w = 2 + (x >> 16) as usize % 8;
            for k in 0..w {
                out.push(b'a' + ((x >> (k % 4 * 4)) % 26) as u8);
            }
            out.push(if x.is_multiple_of(11) { b'\n' } else { b' ' });
        }
        out.truncate(n);
        out
    }

    /// 's benign shape, scaled: a `big`-byte text in
    /// the first commit with `pages` pages sliced from it, removed in the
    /// second, packed as `git repack -adf --window=4095` packs it (each
    /// page a delta against the text). Returns the repository, the pages'
    /// sources and ids.
    fn sliced(tag: &str, big: usize, pages: usize) -> (Scratch, Vec<Vec<u8>>, Vec<ObjectId>) {
        let dir = scratch(tag);
        git(&dir, &["init", "-q", "-b", "main"], None);
        let t = text(big);
        std::fs::write(dir.join("big.txt"), &t).unwrap();
        std::fs::write(dir.join("home.md"), page("H")).unwrap();
        let step = big / pages;
        let mut bodies = Vec::new();
        for i in 0..pages {
            let mut b = format!("---\ntitle: S{i}\n---\n").into_bytes();
            b.extend_from_slice(&t[i * step..(i + 1) * step]);
            std::fs::write(dir.join(format!("s{i}.md")), &b).unwrap();
            bodies.push(b);
        }
        git(&dir, &["add", "-A"], None);
        git(&dir, &["commit", "-q", "-m", "with big.txt"], None);
        git(&dir, &["rm", "-q", "big.txt"], None);
        git(&dir, &["commit", "-q", "-m", "without"], None);
        git(&dir, &["repack", "-q", "-adf", "--window=4095"], None);
        let ids = bodies.iter().map(|b| blob_id(b)).collect();
        (dir, bodies, ids)
    }

    #[test]
    fn a_page_delta_compressed_against_a_large_historical_blob_is_refused_by_name() {
        // Decided: a page git itself delta-compressed against a
        // blob too large to rebuild within the limit (40 MB: its buffer is
        // twice that) is refused, by name, before anything is allocated
        // for it. `git -c core.bigFileThreshold=16m repack -adf` repacks it
        // with no such base (the build's message says so; tests/build.rs).
        let (dir, _, ids) = sliced("sliced-over", 40_000_000, 10);
        let g = open(&dir).unwrap();
        assert!(g.store.head(ids[0]).unwrap().2 >= 1, "a delta");
        let head = g.resolve("HEAD").unwrap();
        let (_, budget) = g.read_tree(head, None, &SiteConfig::default()).unwrap();
        assert!(
            matches!(
                &budget.findings()[..],
                [rubrica_site::Finding::RepositoryOverBudget {
                    what: Budget::DeltaBase { object },
                    ..
                }] if **object == *format!("blob {} (s0.md)", hex(ids[0]))
            ),
            "{:?}",
            budget.findings()
        );
        bounded(&g.reads());
    }

    #[test]
    fn a_page_delta_compressed_against_a_large_base_within_the_limit_is_read_once() {
        // The benign shape within the limit (a 24 MB text, 6 pages of
        // 4 MB): each page is rebuilt from the whole text once, and its
        // second read (its `.md` copy) is the object cache's.
        let (dir, bodies, ids) = sliced("sliced-within", 24_000_000, 6);
        let g = open(&dir).unwrap();
        assert!(g.store.head(ids[0]).unwrap().2 >= 1, "a delta");
        let head = g.resolve("HEAD").unwrap();
        let (tree, budget) = g.read_tree(head, None, &SiteConfig::default()).unwrap();
        assert!(budget.holds(), "{:?}", budget.findings());
        for (i, b) in bodies.iter().enumerate() {
            let f = tree
                .files
                .iter()
                .find(|f| f.path == format!("s{i}.md"))
                .unwrap();
            assert!(f.bytes == *b, "s{i}.md as written");
        }
        let before = g.reads().object_cache_hits;
        for (id, b) in ids.iter().zip(&bodies) {
            assert!(g.blob(&hex(*id)).unwrap() == *b);
        }
        let r = g.reads();
        assert_eq!(r.object_cache_hits - before, ids.len() as u64, "{r:?}");
        bounded(&r);
    }

    #[test]
    fn a_deep_chain_of_versions_starts_from_the_pack_cache() {
        // A page edited 300 times (it is the feed too), packed with deep
        // chains: each version is a delta on the next newer, which the
        // walk read just before; the pack cache holds it, so the chain is
        // not rebuilt from its base each time. The history is the loose
        // repository's.
        let mut s = Vec::new();
        let mut files = BTreeMap::new();
        let words = String::from_utf8(text(4 << 10)).unwrap();
        for c in 0..300 {
            let v = format!("{}{words}\nversion {c}\n", body("home.md", 0));
            files.insert("home.md".to_owned(), v);
            let parents: Vec<usize> = (c > 0).then_some(c).into_iter().collect();
            commit(
                &mut s,
                "main",
                c + 1,
                1_700_000_000 + c as i64,
                &parents,
                &files,
            );
        }
        let loose = import("chain-loose", &s);
        let packed = import("chain-packed", &s);
        git(
            &packed,
            &["repack", "-q", "-adf", "--depth=4095", "--window=300"],
            None,
        );
        let files = set(&["home.md"]);
        let walk = |dir: &Path| {
            let g = open(dir).unwrap();
            let head = g.resolve("HEAD").unwrap();
            let w = g.history(head, None, &files, Some("home.md")).unwrap();
            (w, g.reads())
        };
        let (wl, _) = walk(&loose);
        let (wp, r) = walk(&packed);
        assert_eq!(wp, wl);
        assert_eq!(wp.history.feed_versions.len(), 256);
        assert!(r.pack_cache_hits >= 200, "{r:?}");
        bounded(&r);
    }

    #[test]
    fn a_damaged_object_is_an_error_not_a_refusal_by_the_limit() {
        // Only gix's allocation-limit refusal is a refusal of ours: a page
        // whose header reads but whose body does not (its loose file cut
        // short) is an error, as before, never `Budget::DeltaBase`.
        let o = Odb::new("damaged");
        let body = text(64 << 10);
        let id = o.blob(&body);
        let h = hex(id);
        let path = o.dir.join(format!(".git/objects/{}/{}", &h[..2], &h[2..]));
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(&path, perm).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
        let home = o.blob(&page("H"));
        let c = o.commit(
            o.raw_tree(&[("100644", b"a.md", id), ("100644", b"home.md", home)]),
            &[],
            1_700_000_000,
            b"c\n",
        );
        o.head(c);
        let g = open(&o.dir).unwrap();
        assert_eq!(g.store.header(id).unwrap().1, body.len() as u64);
        let e = g
            .read_tree(c, None, &SiteConfig::default())
            .map(|(_, b)| b.findings())
            .unwrap_err();
        assert!(e.contains(&h) && e.contains("could not be read"), "{e}");
    }

    #[test]
    fn a_tag_over_its_cap_refuses_the_build() {
        // `--rev`'s tags are read as the commit built is: one over its cap
        // (or that cannot be rebuilt) refuses the build, not exit 2.
        let o = Odb::new("bigtag");
        let c = o.commit(
            o.raw_tree(&[("100644", b"home.md", o.blob(&page("H")))]),
            &[],
            1_700_000_000,
            b"c\n",
        );
        let mut raw = format!(
            "object {}\ntype commit\ntag big\ntagger T <t@e.invalid> 1700000000 +0000\n\n",
            hex(c)
        )
        .into_bytes();
        raw.extend(vec![b'a'; 2 << 20]);
        let tag = o.write(gix::object::Kind::Tag, &raw);
        git(&o.dir, &["update-ref", "refs/tags/big", &hex(tag)], None);
        let g = open(&o.dir).unwrap();
        match g.resolve("refs/tags/big") {
            Err(Unresolved::Refused(b)) => assert!(
                matches!(
                    b.findings()[..],
                    [rubrica_site::Finding::RepositoryOverBudget {
                        what: Budget::CommitBytes,
                        ..
                    }]
                ),
                "{:?}",
                b.findings()
            ),
            other => panic!("{other:?}"),
        }
        assert_eq!(g.reads().commits, 0);
    }
}
