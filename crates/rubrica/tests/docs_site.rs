//! The project's own documentation site (`docs/`), built with the binary.
//!
//! The repository root is the first directory, walking up from the crate's directory, that
//! holds both `docs/home.md` and `Cargo.toml`. No path climbs out of the crate with a
//! literal `..` (the crate-paths gate refuses one), and the test fails rather than skips
//! when no such directory exists: the documentation site is part of what the tests check.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A directory under the system temp dir, removed when dropped.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rubrica-docs-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The real path: a symbolic link in it (macOS's /var) is refused.
    Scratch(dir.canonicalize().unwrap())
}

/// `git` in `dir`, isolated from the user's and the system's config.
fn git_at(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git")
}

/// The repository root: the first ancestor of the crate's directory (itself included) that
/// holds both `docs/home.md` and `Cargo.toml`.
fn repo_root() -> PathBuf {
    let start = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join("docs/home.md").is_file() && d.join("Cargo.toml").is_file() {
            return d.to_path_buf();
        }
        dir = d.parent();
    }
    panic!("no directory holding docs/home.md and Cargo.toml above {start:?}");
}

/// Copies the directory `from` (files and directories only) to `to`.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn rubrica(args: &[&std::ffi::OsStr]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .args(args)
        .output()
        .expect("run rubrica");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// `check --strict docs` is clean, and `build --subtree docs` (of a commit of a copy of it) writes the home page, the
/// install page and the spec index.
#[test]
fn the_docs_site_builds_and_checks_clean() {
    let root = repo_root();
    let docs = root.join("docs");
    let out = rubrica(&["check".as_ref(), "--strict".as_ref(), docs.as_os_str()]);
    assert!(
        out.code == 0,
        "check --strict docs: {}\n{}{}",
        out.code,
        out.stdout,
        out.stderr
    );
    // `build` reads a commit, not the working directory: build a commit of a copy of `docs/`,
    // so the test needs no commit of the checkout itself.
    let site = scratch("site");
    let repo = site.0.join("repo");
    copy_dir(&docs, &repo.join("docs"));
    let dest = site.0.join("out");
    for args in [
        &["init", "-q"][..],
        &["add", "-A", "."][..],
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "-m",
            "docs",
        ][..],
    ] {
        let out = git_at(&repo, args);
        assert!(out.status.success(), "git {args:?} failed");
    }
    let out = rubrica(&[
        "build".as_ref(),
        "--subtree".as_ref(),
        "docs".as_ref(),
        repo.as_os_str(),
        dest.as_os_str(),
    ]);
    assert!(
        out.code == 0,
        "build --subtree docs: {}\n{}{}",
        out.code,
        out.stdout,
        out.stderr
    );
    for page in ["home.html", "install.html", "spec/home.html"] {
        assert!(dest.join(page).is_file(), "{page} was not built");
    }
}
