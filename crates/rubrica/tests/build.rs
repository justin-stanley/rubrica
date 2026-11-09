//! `rubrica build` run as a binary on git repositories the tests
//! make with the `git` command (isolated from any user or system config),
//! into directories under the system temp dir. What the output holds is
//! `crates/rubrica-site/tests/build.rs`'s; here: reading the tree at a
//! commit with its modes, the history, and writing `out/` safely.

use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A directory under the system temp dir, removed (with all it holds) when
/// dropped, passing or panicking: a passing run leaves nothing in TMPDIR
/// (`scripts/ci.sh` checks).
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
        "rubrica-build-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The real path: a symbolic link in it (macOS's /var) is refused.
    Scratch(dir.canonicalize().unwrap())
}

/// `git` in `dir`, isolated from the user's and the system's config, with
/// fixed identities; `date` (seconds, offset) for a commit.
fn git_at(dir: &Path, args: &[&str], date: Option<&str>, stdin: Option<&[u8]>) -> String {
    let mut cmd = Command::new("git");
    cmd.current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid");
    if let Some(d) = date {
        cmd.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
    }
    let out = if let Some(input) = stdin {
        use std::io::Write as _;
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("git");
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    } else {
        cmd.output().expect("git")
    };
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

struct Repo {
    dir: PathBuf,
    // Holds `dir`'s scratch directory: removed with the repository.
    _scratch: Scratch,
}

impl Repo {
    fn new(tag: &str) -> Self {
        let base = scratch(tag);
        let dir = base.join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        git_at(&dir, &["init", "-q", "-b", "main"], None, None);
        Self {
            dir,
            _scratch: base,
        }
    }

    fn write(&self, path: &str, body: &str) -> &Self {
        let p = self.dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
        self
    }

    fn page(&self, path: &str, title: &str, body: &str) -> &Self {
        self.write(path, &format!("---\ntitle: {title}\n---\n{body}"))
    }

    /// An index entry of any mode (a link, a submodule, a case collision,
    /// a name the worktree cannot hold), its blob `bytes`.
    fn entry(&self, mode: &str, path: &str, bytes: &[u8]) -> &Self {
        let id = if mode == "160000" {
            // A gitlink names a commit of another repository: any id.
            "1234567890123456789012345678901234567890".to_owned()
        } else {
            git_at(
                &self.dir,
                &["hash-object", "-w", "--stdin"],
                None,
                Some(bytes),
            )
        };
        git_at(
            &self.dir,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("{mode},{id},{path}"),
            ],
            None,
            None,
        );
        self
    }

    fn commit(&self, msg: &str, date: &str) -> String {
        git_at(&self.dir, &["add", "-A", "."], None, None);
        self.commit_index(msg, date)
    }

    /// Commit the index as it is (entries made by [`Repo::entry`] whose
    /// files the worktree does not have stay).
    fn commit_index(&self, msg: &str, date: &str) -> String {
        git_at(
            &self.dir,
            &["commit", "-q", "--no-verify", "--allow-empty", "-m", msg],
            Some(date),
            None,
        );
        git_at(&self.dir, &["rev-parse", "HEAD"], None, None)
    }
}

fn rubrica(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .expect("rubrica runs")
}

/// The empty directory `rubrica build` runs in: made once per test process,
/// under cargo's target tmp dir (CARGO_TARGET_TMPDIR, not TMPDIR, since a
/// static is never dropped), so a run leaves nothing in TMPDIR and makes no
/// directory per build. It is shared by every test process (nextest runs
/// each test in its own, at the same time), so it is created, never removed
/// and recreated: a process doing that would pull it from under the
/// builds of the others. `rubrica build` writes nothing to its working
/// directory.
fn cwd() -> &'static Path {
    static CWD: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    CWD.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("build-cwd");
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    })
}

fn build(repo: &Repo, out: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["build"];
    args.extend_from_slice(extra);
    let r = repo.dir.to_str().unwrap();
    let o = out.to_str().unwrap();
    args.push(r);
    args.push(o);
    rubrica(&args, cwd())
}

fn ok(o: &Output) {
    assert!(
        o.status.success(),
        "exit {:?}\n{}\n{}",
        o.status.code(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

fn code(o: &Output) -> i32 {
    o.status.code().expect("an exit code")
}

fn all(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// Every entry under `dir`: its relative path, and its bytes (`None` for a
/// directory), never following a link (a link fails the test).
fn tree(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, rel)) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let name = e.file_name().into_string().unwrap();
            let r = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            let meta = std::fs::symlink_metadata(e.path()).unwrap();
            assert!(!meta.file_type().is_symlink(), "{r} is a symbolic link");
            if meta.is_dir() {
                assert_eq!(meta.mode() & 0o777, 0o755, "{r}: a directory's mode");
                out.insert(r.clone(), None);
                stack.push((e.path(), r));
            } else {
                assert!(meta.is_file(), "{r} is no regular file");
                assert_eq!(meta.mode() & 0o777, 0o644, "{r}: a file's mode");
                out.insert(r, Some(std::fs::read(e.path()).unwrap()));
            }
        }
    }
    out
}

fn read(out: &Path, path: &str) -> String {
    std::fs::read_to_string(out.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

// 2026-10-20 at 10:00:01, 11:00:01 and 12:00:01, -07:00.
const D1: &str = "1792515601 -0700";
const D2: &str = "1792519201 -0700";
const D3: &str = "1792522801 -0700";

// ---------- the build ----------

#[test]
fn recent_and_last_updated_come_from_the_commits() {
    let repo = Repo::new("history");
    repo.page("home.md", "Home", "# Home\n")
        .page("alpha.md", "Alpha page", "# Alpha\n");
    let c1 = repo.commit("first: home and alpha", D1);
    repo.page("alpha.md", "Alpha page", "# Alpha\n\nmore\n");
    let c2 = repo.commit("second: alpha", D2);
    repo.write("scripts/x.sh", "echo\n");
    repo.page("beta.md", "Beta page", "---\n# Beta\n");
    let c3 = repo.commit("third: beta and a script", D3);
    repo.write("scripts/y.sh", "echo\n");
    repo.commit("fourth: no page", "1792526401 -0700");
    let out_scratch = scratch("history-out");
    let out = out_scratch.join("out");
    ok(&build(&repo, &out, &[]));
    let recent = read(&out, "_recent.html");
    let at = |s: &str| {
        recent
            .find(s)
            .unwrap_or_else(|| panic!("{s} not in {recent}"))
    };
    // Newest first; a commit that changed no page is not listed.
    assert!(at("third: beta and a script") < at("second: alpha"));
    assert!(at("second: alpha") < at("first: home and alpha"));
    assert!(!recent.contains("fourth: no page"));
    for c in [&c1, &c2, &c3] {
        assert!(recent.contains(&c[..7]), "{c}");
    }
    let third = &recent[at("third: beta")..at("second: alpha")];
    assert!(third.contains("href=\"/beta\""), "{third}");
    assert!(!third.contains("href=\"/alpha\""));
    // Last updated: the last commit that changed each file.
    assert!(read(&out, "alpha.html")
        .contains("<time datetime=\"2026-10-20T11:00:01-07:00\">2026-10-20</time>"));
    assert!(read(&out, "index.html").contains("<time datetime=\"2026-10-20T10:00:01-07:00\">"));
}

#[test]
fn the_changelog_feed_dates_come_from_git() {
    let repo = Repo::new("feed");
    repo.page("home.md", "Home", "# Home\n");
    repo.page("changelog.md", "Changelog", "## 0.1\n\nFirst.\n");
    repo.commit("0.1", D1);
    repo.page(
        "changelog.md",
        "Changelog",
        "## 0.2\n\nSecond.\n\n## 0.1\n\nFirst.\n",
    );
    repo.commit("0.2", D2);
    repo.page(
        "changelog.md",
        "Changelog",
        "## 0.2\n\nSecond, fixed.\n\n## 0.1\n\nFirst.\n",
    );
    repo.commit("a fix", D3);
    let dir = scratch("feed-cfg");
    let cfg = dir.join("site.toml");
    std::fs::write(
        &cfg,
        "name = \"Rubrica\"\nbase_url = \"https://rubrica.wiki\"\nfeed_url = \"/changelog.xml\"\nfeed_source = \"changelog.md\"\n\n[site]\nrender = \"wikijs-compat\"\n",
    )
    .unwrap();
    let out_scratch = scratch("feed-out");
    let out = out_scratch.join("out");
    ok(&build(&repo, &out, &["--config", cfg.to_str().unwrap()]));
    let xml = read(&out, "changelog.xml");
    let e02 = xml
        .find("<id>https://rubrica.wiki/changelog#h-02</id>")
        .expect(&xml);
    let e01 = xml
        .find("<id>https://rubrica.wiki/changelog#h-01</id>")
        .unwrap();
    assert!(
        xml[e02..e01].contains("<updated>2026-10-20T11:00:01-07:00</updated>"),
        "{xml}"
    );
    assert!(
        xml[e01..].contains("<updated>2026-10-20T10:00:01-07:00</updated>"),
        "{xml}"
    );
    assert!(xml.contains("<title>Rubrica 0.2</title>"));
    // A bad config is an error before anything is written.
    std::fs::write(&cfg, "name = \"x\"\nbase_url = \"javascript:alert(1)\"\n").unwrap();
    let out2_scratch = scratch("feed-out2");
    let out2 = out2_scratch.join("out");
    let o = build(&repo, &out2, &["--config", cfg.to_str().unwrap()]);
    assert_eq!(code(&o), 2, "{}", all(&o));
    assert!(!out2.exists());
}

#[test]
fn a_feed_heading_is_dated_by_its_earliest_version_as_git_has_them() {
    // `## v4` added on two branches, the side branch's clock
    // behind (its second commit older than its first). The heading's date
    // is the earliest committer time among `git log -- changelog.md`'s
    // versions that have it.
    let repo = Repo::new("feed-branches");
    let log = |body: &str| format!("## v4\n\n{body}\n\n## v3\n\nThree.\n");
    repo.page("home.md", "Home", "# Home\n");
    repo.page("changelog.md", "Changelog", "## v3\n\nThree.\n");
    repo.commit("v3", "1792000100 -0700");
    git_at(&repo.dir, &["checkout", "-q", "-b", "side"], None, None);
    repo.page("changelog.md", "Changelog", &log("Four, on the side."));
    repo.commit("v4 on the side", "1792000900 -0700");
    repo.page(
        "changelog.md",
        "Changelog",
        &log("Four, on the side, fixed."),
    );
    let s2 = repo.commit("a fix on the side", "1792000200 -0700");
    git_at(&repo.dir, &["checkout", "-q", "main"], None, None);
    repo.page("changelog.md", "Changelog", &log("Four, on main."));
    let a1 = repo.commit("v4 on main", "1792000400 -0700");
    repo.page("changelog.md", "Changelog", &log("Four, merged."));
    git_at(&repo.dir, &["add", "-A", "."], None, None);
    let tree = git_at(&repo.dir, &["write-tree"], None, None);
    let merge = git_at(
        &repo.dir,
        &["commit-tree", &tree, "-p", &a1, "-p", &s2, "-m", "merge"],
        Some("1792001000 -0700"),
        None,
    );
    git_at(
        &repo.dir,
        &["update-ref", "refs/heads/main", &merge],
        None,
        None,
    );
    // git's answer.
    let mut want: Option<(i64, String)> = None;
    for line in git_at(
        &repo.dir,
        &["log", "--format=%H %ct %cI", "main", "--", "changelog.md"],
        None,
        None,
    )
    .lines()
    {
        let mut f = line.split(' ');
        let (c, ct, iso) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        let v = git_at(
            &repo.dir,
            &["show", &format!("{c}:changelog.md")],
            None,
            None,
        );
        let ct: i64 = ct.parse().unwrap();
        if v.contains("## v4") && want.as_ref().is_none_or(|(t, _)| ct < *t) {
            want = Some((ct, iso.to_owned()));
        }
    }
    let want = want.unwrap().1;
    let dir = scratch("feed-branches-cfg");
    let cfg = dir.join("site.toml");
    std::fs::write(
        &cfg,
        "name = \"Rubrica\"\nbase_url = \"https://rubrica.wiki\"\nfeed_url = \"/changelog.xml\"\nfeed_source = \"changelog.md\"\n",
    )
    .unwrap();
    let out_scratch = scratch("feed-branches-out");
    let out = out_scratch.join("out");
    ok(&build(
        &repo,
        &out,
        &["--config", cfg.to_str().unwrap(), "--rev", "main"],
    ));
    let xml = read(&out, "changelog.xml");
    let v4 = &xml[xml.find("changelog#v4</id>").expect(&xml)..];
    let v4 = &v4[..v4.find("</entry>").unwrap()];
    assert!(
        v4.contains(&format!("<updated>{want}</updated>")),
        "{want}: {v4}"
    );
}

#[test]
fn assets_are_embedded_so_the_binary_is_self_contained() {
    let repo = Repo::new("assets");
    repo.page(
        "home.md",
        "Home",
        "# Home\n\n```mermaid\ngraph TD; A-->B\n```\n",
    );
    repo.commit("home", D1);
    let out_scratch = scratch("assets-out");
    let out = out_scratch.join("out");
    // Run from an empty directory, with no environment but PATH.
    ok(&build(&repo, &out, &[]));
    // The binary carries its own assets: the stylesheet and the icon and, with the search
    // bundle, its loader and module; nothing else (no other script, no vendored package).
    let theme = rubrica_site::build::theme_assets();
    let mut names: Vec<String> = std::fs::read_dir(out.join("_assets"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    #[cfg(feature = "search-wasm")]
    assert_eq!(
        names,
        [
            "icon.svg",
            "rubrica-search.js",
            "rubrica-search_bg.wasm",
            "rubrica.css"
        ]
    );
    #[cfg(not(feature = "search-wasm"))]
    assert_eq!(names, ["icon.svg", "rubrica.css"]);
    let theme_names: Vec<&String> = names
        .iter()
        .filter(|n| !n.starts_with("rubrica-search"))
        .collect();
    let mut embedded: Vec<&str> = theme
        .iter()
        .map(|(path, _)| path.strip_prefix("_assets/").unwrap())
        .collect();
    embedded.sort_unstable();
    assert_eq!(
        embedded, theme_names,
        "the embedded theme assets are the written ones"
    );
    for (path, bytes) in theme {
        assert_eq!(std::fs::read(out.join(path)).unwrap(), *bytes, "{path}");
    }
    #[cfg(feature = "search-wasm")]
    {
        assert_eq!(
            std::fs::read(out.join("_assets/rubrica-search.js")).unwrap(),
            rubrica_wasm_bundle::LOADER.as_bytes()
        );
        assert_eq!(
            std::fs::read(out.join("_assets/rubrica-search_bg.wasm")).unwrap(),
            rubrica_wasm_bundle::WASM
        );
    }
}

// ---------- the tree, as git has it ----------

#[test]
fn a_committed_symlink_is_refused_and_never_read() {
    let repo = Repo::new("symlink");
    repo.page("home.md", "Home", "# Home\n");
    repo.commit("home", D1);
    let secret_scratch = scratch("secret");
    let secret = secret_scratch.join("secret.md");
    std::fs::write(&secret, "---\ntitle: SECRET-CONTENT\n---\nSECRET-CONTENT\n").unwrap();
    repo.entry("120000", "leak.md", secret.to_str().unwrap().as_bytes());
    repo.entry(
        "120000",
        "linked",
        secret.parent().unwrap().to_str().unwrap().as_bytes(),
    );
    repo.commit_index("links", D2);
    let out_scratch = scratch("symlink-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &[]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    let report = all(&o);
    assert!(
        report.contains("leak.md") && report.contains("not-a-regular-file"),
        "{report}"
    );
    assert!(report.contains("linked"), "{report}");
    assert!(!report.contains("SECRET-CONTENT"));
    assert!(!out.exists(), "a refused build writes nothing");
}

#[test]
fn a_submodule_is_refused() {
    let repo = Repo::new("submodule");
    repo.page("home.md", "Home", "# Home\n");
    repo.commit("home", D1);
    repo.entry("160000", "notes", b"");
    repo.commit_index("a submodule", D2);
    let out_scratch = scratch("submodule-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &[]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    assert!(all(&o).contains("notes"));
}

#[test]
fn an_executable_page_is_written_without_its_mode() {
    let repo = Repo::new("exec");
    repo.page("home.md", "Home", "# Home\n");
    repo.page("tool.md", "Tool", "# Tool\n");
    std::fs::set_permissions(
        repo.dir.join("tool.md"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    repo.commit("an executable page", D1);
    let ls = git_at(&repo.dir, &["ls-tree", "HEAD", "tool.md"], None, None);
    assert!(ls.starts_with("100755"), "{ls}");
    let out_scratch = scratch("exec-out");
    let out = out_scratch.join("out");
    ok(&build(&repo, &out, &[]));
    // `tree` checks every mode: 0644 files, 0755 directories.
    let t = tree(&out);
    assert!(t.contains_key("tool.html") && t.contains_key("tool.md"));
}

#[test]
fn untracked_and_uncommitted_changes_are_not_built() {
    let repo = Repo::new("snapshot");
    repo.page("home.md", "Home", "# Home\n\ncommitted\n");
    repo.commit("home", D1);
    repo.page("home.md", "Home", "# Home\n\nUNCOMMITTED\n");
    repo.page("draft.md", "Draft", "# Untracked\n");
    let out_scratch = scratch("snapshot-out");
    let out = out_scratch.join("out");
    ok(&build(&repo, &out, &[]));
    let home = read(&out, "index.html");
    assert!(home.contains("committed") && !home.contains("UNCOMMITTED"));
    assert!(!out.join("draft.html").exists());
}

#[test]
fn rev_and_subtree_choose_what_is_built() {
    let repo = Repo::new("rev");
    repo.page("home.md", "Home", "# Home\n\none\n");
    repo.page("site/content/home.md", "Site home", "# Site\n");
    repo.page("site/content/docs.md", "Docs", "# Docs\n");
    let first = repo.commit("one", D1);
    repo.page("home.md", "Home", "# Home\n\ntwo\n");
    repo.commit("two", D2);
    let out_scratch = scratch("rev-out");
    let out = out_scratch.join("out");
    ok(&build(&repo, &out, &["--rev", &first[..10]]));
    assert!(read(&out, "index.html").contains("one"));
    assert!(read(&out, "_health").contains(&first));
    let out_scratch = scratch("subtree-out");
    let out = out_scratch.join("out");
    ok(&build(&repo, &out, &["--subtree", "site/content"]));
    assert!(read(&out, "index.html").contains("Site home"));
    assert!(out.join("docs.html").exists());
    assert!(!out.join("site").exists());
    // A rev or subtree that names nothing is an error.
    let out_scratch = scratch("rev-bad");
    let out = out_scratch.join("out");
    assert_eq!(code(&build(&repo, &out, &["--rev", "no-such-branch"])), 2);
    assert_eq!(code(&build(&repo, &out, &["--subtree", "nowhere"])), 2);
    assert!(!out.exists());
}

#[test]
fn findings_about_files_and_paths_refuse_the_build() {
    let repo = Repo::new("refuse");
    repo.page("home.md", "Home", "# Home\n");
    repo.commit("home", D1);
    let page = b"---\ntitle: X\n---\n# X\n";
    repo.entry("100644", "Alpha.md", page)
        .entry("100644", "alpha.md", page);
    repo.entry("100644", "_hidden.md", page);
    repo.commit_index("collisions", D2);
    let out_scratch = scratch("refuse-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &[]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    let report = all(&o);
    for kind in ["duplicate-url", "reserved-page-path"] {
        assert!(report.contains(kind), "{kind}: {report}");
    }
    assert!(!out.exists());
}

#[test]
fn a_name_that_is_not_utf8_refuses_the_build() {
    let repo = Repo::new("utf8");
    repo.page("home.md", "Home", "# Home\n");
    repo.commit("home", D1);
    // `git mktree` takes any bytes for a name.
    let blob = git_at(
        &repo.dir,
        &["hash-object", "-w", "--stdin"],
        None,
        Some(b"x"),
    );
    let home = git_at(&repo.dir, &["rev-parse", "HEAD:home.md"], None, None);
    let mut listing = Vec::new();
    listing.extend_from_slice(format!("100644 blob {home}\thome.md\n").as_bytes());
    listing.extend_from_slice(format!("100644 blob {blob}\tbad").as_bytes());
    listing.extend_from_slice(b"\xff.md\n");
    let tree = git_at(&repo.dir, &["mktree"], None, Some(&listing));
    let commit = git_at(
        &repo.dir,
        &["commit-tree", &tree, "-m", "bad name"],
        Some(D2),
        None,
    );
    let out_scratch = scratch("utf8-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &["--rev", &commit]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    assert!(all(&o).contains("path-not-utf8"), "{}", all(&o));
    assert!(!out.exists());
}

// ---------- writing out/ ----------

#[test]
fn out_must_be_new_or_empty_and_reached_by_no_symlink() {
    let repo = Repo::new("out");
    repo.page("home.md", "Home", "# Home\n");
    repo.commit("home", D1);
    // An empty directory is replaced.
    let empty_scratch = scratch("out-empty");
    let empty = empty_scratch.join("out");
    std::fs::create_dir(&empty).unwrap();
    ok(&build(&repo, &empty, &[]));
    assert!(empty.join("index.html").exists());
    // A directory with anything in it is left alone.
    let full_scratch = scratch("out-full");
    let full = full_scratch.join("out");
    std::fs::create_dir(&full).unwrap();
    std::fs::write(full.join("keep"), "mine").unwrap();
    let o = build(&repo, &full, &[]);
    assert_eq!(code(&o), 2, "{}", all(&o));
    // Refused up front, by name (a rename onto it would fail too, after
    // all the work).
    assert!(all(&o).contains("exists and is not empty"), "{}", all(&o));
    assert_eq!(std::fs::read_dir(&full).unwrap().count(), 1);
    // `out` itself a symbolic link: refused, the target untouched.
    let base = scratch("out-link");
    std::fs::create_dir(base.join("target")).unwrap();
    std::os::unix::fs::symlink(base.join("target"), base.join("out")).unwrap();
    let o = build(&repo, &base.join("out"), &[]);
    assert_eq!(code(&o), 2, "{}", all(&o));
    // Refused by name, before any work (a rename onto the link would fail
    // too: a directory cannot replace a non-directory).
    assert!(
        all(&o).contains("is a symbolic link: refused"),
        "{}",
        all(&o)
    );
    assert_eq!(std::fs::read_dir(base.join("target")).unwrap().count(), 0);
    // A symbolic link anywhere on the way to it: refused.
    std::os::unix::fs::symlink(base.join("target"), base.join("via")).unwrap();
    let o = build(&repo, &base.join("via").join("out"), &[]);
    assert_eq!(code(&o), 2, "{}", all(&o));
    assert!(!base.join("target").join("out").exists());
    // A file where `out` would be, or a missing parent: refused.
    std::fs::write(base.join("file"), "x").unwrap();
    assert_eq!(code(&build(&repo, &base.join("file"), &[])), 2);
    assert_eq!(
        code(&build(&repo, &base.join("missing").join("out"), &[])),
        2
    );
    // Nothing of a build is left beside `out`.
    let names: Vec<String> = std::fs::read_dir(&base)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(names.iter().all(|n| !n.contains("rubrica")), "{names:?}");
}

#[test]
fn a_tree_over_the_file_budget_refuses_the_build() {
    // The repository budget: a tree of more entries than a site is made
    // from (250,000), all one blob, made with `git mktree`.
    let repo = Repo::new("budget");
    repo.page("home.md", "Home", "# Home\n");
    repo.commit("home", D1);
    let blob = git_at(
        &repo.dir,
        &["hash-object", "-w", "--stdin"],
        None,
        Some(b"x"),
    );
    let home = git_at(&repo.dir, &["rev-parse", "HEAD:home.md"], None, None);
    let mut listing = format!("100644 blob {home}\thome.md\n");
    for i in 0..250_001 {
        listing.push_str(&format!("100644 blob {blob}\tf{i:06}\n"));
    }
    let tree = git_at(&repo.dir, &["mktree"], None, Some(listing.as_bytes()));
    let commit = git_at(
        &repo.dir,
        &["commit-tree", &tree, "-m", "many"],
        Some(D2),
        None,
    );
    let out_scratch = scratch("budget-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &["--rev", &commit]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    assert!(all(&o).contains("repository-over-budget"), "{}", all(&o));
    assert!(!out.exists());
}

#[test]
fn a_tree_over_the_byte_budget_stops_the_read() {
    // 40 assets of 4 MiB (one blob, 40 names): over the 128 MiB a site is
    // made from at the 33rd, where the reader stops (debug builds name
    // each blob read).
    let repo = Repo::new("bytes");
    repo.page("home.md", "Home", "# Home\n");
    repo.commit("home", D1);
    let blob = git_at(
        &repo.dir,
        &["hash-object", "-w", "--stdin"],
        None,
        Some(&vec![b'x'; 4 << 20]),
    );
    let mut assets = String::new();
    for i in 0..40 {
        assets.push_str(&format!("100644 blob {blob}\t{i:02}.png\n"));
    }
    let assets = git_at(&repo.dir, &["mktree"], None, Some(assets.as_bytes()));
    let home = git_at(&repo.dir, &["rev-parse", "HEAD:home.md"], None, None);
    let root = format!("040000 tree {assets}\tassets\n100644 blob {home}\thome.md\n");
    let tree = git_at(&repo.dir, &["mktree"], None, Some(root.as_bytes()));
    let commit = git_at(
        &repo.dir,
        &["commit-tree", &tree, "-m", "big"],
        Some(D2),
        None,
    );
    let out_scratch = scratch("bytes-out");
    let out = out_scratch.join("out");
    let o = Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .args([
            "build",
            "--rev",
            &commit,
            repo.dir.to_str().unwrap(),
            out.to_str().unwrap(),
        ])
        .env("RUBRICA_TRACE_OPENS", "1")
        .output()
        .unwrap();
    assert_eq!(code(&o), 1, "{}", all(&o));
    assert!(all(&o).contains("repository-over-budget"), "{}", all(&o));
    if cfg!(debug_assertions) {
        let read = String::from_utf8_lossy(&o.stderr)
            .lines()
            .filter(|l| l.starts_with("rubrica: open assets/"))
            .count();
        assert!(read <= 33, "{read} blobs read: the reader did not stop");
    }
    assert!(!out.exists());
}

// ---------- the readers' bounds ----------

/// A commit of `root`, with `home.md`, made with `git commit-tree`.
fn commit_of(repo: &Repo, entries: &str) -> String {
    let home = git_at(
        &repo.dir,
        &["hash-object", "-w", "--stdin"],
        None,
        Some(b"---\ntitle: Home\n---\n# Home\n"),
    );
    let root = format!("{entries}100644 blob {home}\thome.md\n");
    let tree = git_at(&repo.dir, &["mktree"], None, Some(root.as_bytes()));
    git_at(
        &repo.dir,
        &["commit-tree", &tree, "-m", "shape"],
        Some(D1),
        None,
    )
}

/// The first review's tree bomb: `depth` levels, each a tree whose `a`
/// and `b` both name the level below, over `leaf`: 2^depth paths from
/// depth + 2 objects.
fn bomb(repo: &Repo, depth: usize, leaf: &str) -> String {
    let mut t = leaf.to_owned();
    for _ in 0..depth - 1 {
        let listing = format!("040000 tree {t}\ta\n040000 tree {t}\tb\n");
        t = git_at(&repo.dir, &["mktree"], None, Some(listing.as_bytes()));
    }
    commit_of(repo, &format!("040000 tree {t}\ta\n040000 tree {t}\tb\n"))
}

fn timed_build(repo: &Repo, commit: &str) -> (Output, std::time::Duration) {
    let out_scratch = scratch("bomb-out");
    let out = out_scratch.join("out");
    let start = std::time::Instant::now();
    let o = build(repo, &out, &["--rev", commit]);
    let took = start.elapsed();
    assert!(!out.exists());
    (o, took)
}

#[test]
fn a_tree_bomb_is_refused_quickly() {
    // The reviewer's shape at depth 40 (an empty leaf: no file
    // is ever handed over); before, depth 20 took 18.5 s and depth 40
    // never finished. Every tree read is reached by an entry, which is
    // counted, so the entry budget stops it (no tree count).
    let repo = Repo::new("bomb");
    let empty = git_at(&repo.dir, &["mktree"], None, Some(b""));
    let c = bomb(&repo, 40, &empty);
    let (o, took) = timed_build(&repo, &c);
    assert_eq!(code(&o), 1, "{}", all(&o));
    assert!(all(&o).contains("repository-over-budget"), "{}", all(&o));
    // The entries counted, or their paths' bytes (deep paths), stop it.
    assert!(
        all(&o).contains("entries (files") || all(&o).contains("bytes of paths"),
        "{}",
        all(&o)
    );
    assert!(took.as_secs() < 30, "{took:?}");
}

#[test]
fn a_tree_bomb_of_names_that_are_not_utf8_is_refused_with_its_problems_bounded() {
    // Each non-UTF-8 name is an entry too, so the problems a
    // bomb repeats are bounded by the budget.
    let repo = Repo::new("bomb-utf8");
    let blob = git_at(
        &repo.dir,
        &["hash-object", "-w", "--stdin"],
        None,
        Some(b"x"),
    );
    let mut raw = b"100644 \xff.md\0".to_vec();
    raw.extend_from_slice(
        &(0..20)
            .map(|i| u8::from_str_radix(&blob[2 * i..2 * i + 2], 16).unwrap())
            .collect::<Vec<u8>>(),
    );
    let leaf = git_at(
        &repo.dir,
        &["hash-object", "-t", "tree", "-w", "--literally", "--stdin"],
        None,
        Some(&raw),
    );
    let c = bomb(&repo, 40, &leaf);
    let (o, took) = timed_build(&repo, &c);
    assert_eq!(code(&o), 1, "{}", all(&o));
    let report = all(&o);
    assert!(report.contains("repository-over-budget"), "{report}");
    let problems = report.matches("path-not-utf8").count();
    assert!(
        problems > 0 && problems <= rubrica_site::MAX_REPO_FILES,
        "{problems}"
    );
    assert!(took.as_secs() < 30, "{took:?}");
}

#[test]
fn an_entry_of_an_unknown_mode_is_refused_as_one() {
    // `123456`'s type bits are a link's; it is no link.
    let repo = Repo::new("mode");
    let blob = git_at(
        &repo.dir,
        &["hash-object", "-w", "--stdin"],
        None,
        Some(b"---\ntitle: X\n---\n"),
    );
    let mut raw = b"123456 x.md\0".to_vec();
    raw.extend_from_slice(
        &(0..20)
            .map(|i| u8::from_str_radix(&blob[2 * i..2 * i + 2], 16).unwrap())
            .collect::<Vec<u8>>(),
    );
    let sub = git_at(
        &repo.dir,
        &["hash-object", "-t", "tree", "-w", "--literally", "--stdin"],
        None,
        Some(&raw),
    );
    let c = commit_of(&repo, &format!("040000 tree {sub}\td\n"));
    let out_scratch = scratch("mode-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &["--rev", &c]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    assert!(all(&o).contains("unknown mode"), "{}", all(&o));
    assert!(!all(&o).contains("symbolic link"), "{}", all(&o));
}

#[test]
fn a_site_with_no_home_page_is_refused() {
    // Refused (`no-home-page`), not given a generated
    // index: `/` would otherwise be a 404.
    let repo = Repo::new("nohome");
    repo.page("alpha.md", "Alpha", "# Alpha\n");
    repo.commit("no home", D1);
    let out_scratch = scratch("nohome-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &[]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    assert!(all(&o).contains("no-home-page"), "{}", all(&o));
    assert!(!out.exists());
}

// ---------- shallow and damaged histories ----------

/// Five commits: home, alpha and beta; alpha; beta; gamma (new); alpha.
fn five(repo: &Repo) -> Vec<String> {
    repo.page("home.md", "Home", "# Home\n")
        .page("alpha.md", "Alpha", "# Alpha\n")
        .page("beta.md", "Beta", "# Beta\n");
    let mut ids = vec![repo.commit("one", "1792515601 -0700")];
    repo.page("alpha.md", "Alpha", "# Alpha\n\n2\n");
    ids.push(repo.commit("two: alpha", "1792519201 -0700"));
    repo.page("beta.md", "Beta", "# Beta\n\n3\n");
    ids.push(repo.commit("three: beta", "1792522801 -0700"));
    repo.page("gamma.md", "Gamma", "# Gamma\n");
    ids.push(repo.commit("four: gamma", "1792526401 -0700"));
    repo.page("alpha.md", "Alpha", "# Alpha\n\n5\n");
    ids.push(repo.commit("five: alpha", "1792530001 -0700"));
    ids
}

fn dated(out: &Path, page: &str) -> Option<String> {
    let html = read(out, page);
    let at = html.find("<time datetime=\"")?;
    Some(html[at + 16..at + 41].to_owned())
}

fn shallow_clone(repo: &Repo, depth: &str) -> Repo {
    let base = scratch("shallow");
    let dir = base.join("clone");
    let url = format!("file://{}", repo.dir.display());
    git_at(
        dir.parent().unwrap(),
        &["clone", "-q", "--depth", depth, &url, "clone"],
        None,
        None,
    );
    Repo {
        dir,
        _scratch: base,
    }
}

#[test]
fn a_shallow_clone_builds_with_what_it_has() {
    // `actions/checkout`'s default depth is 1; a missing
    // parent made the build exit 2.
    let repo = Repo::new("full");
    five(&repo);
    let d3 = shallow_clone(&repo, "3");
    let out_scratch = scratch("shallow-out");
    let out = out_scratch.join("out");
    let o = build(&d3, &out, &[]);
    ok(&o);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("shallow"), "{err}");
    // What changed within the clone's depth is dated as in the full
    // history; what changed at its boundary or before is not dated.
    assert_eq!(
        dated(&out, "alpha.html").as_deref(),
        Some("2026-10-20T14:00:01-07:00")
    );
    assert_eq!(
        dated(&out, "gamma.html").as_deref(),
        Some("2026-10-20T13:00:01-07:00")
    );
    assert_eq!(dated(&out, "beta.html"), None);
    assert_eq!(dated(&out, "home.html"), None);
    let recent = read(&out, "_recent.html");
    assert!(recent.contains("five: alpha") && recent.contains("four: gamma"));
    assert!(!recent.contains("three: beta"), "{recent}");
    // Depth 1: nothing is dated, and the build still succeeds.
    let d1 = shallow_clone(&repo, "1");
    let out_scratch = scratch("shallow-out1");
    let out = out_scratch.join("out");
    let o = build(&d1, &out, &[]);
    ok(&o);
    assert!(String::from_utf8_lossy(&o.stderr).contains("shallow"));
    for p in ["alpha.html", "beta.html", "gamma.html", "index.html"] {
        assert_eq!(dated(&out, p), None, "{p}");
    }
}

#[test]
fn a_damaged_historical_commit_stops_the_walk_not_the_build() {
    // An unreadable parent (its loose object overwritten).
    let repo = Repo::new("damaged");
    let ids = five(&repo);
    let obj = repo
        .dir
        .join(".git/objects")
        .join(&ids[1][..2])
        .join(&ids[1][2..]);
    std::fs::set_permissions(&obj, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::write(&obj, b"not a zlib stream").unwrap();
    let out_scratch = scratch("damaged-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &[]);
    ok(&o);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("could not be read"), "{err}");
    assert_eq!(
        dated(&out, "alpha.html").as_deref(),
        Some("2026-10-20T14:00:01-07:00")
    );
    assert_eq!(
        dated(&out, "gamma.html").as_deref(),
        Some("2026-10-20T13:00:01-07:00")
    );
    assert_eq!(
        dated(&out, "beta.html"),
        None,
        "beta's change is against the damaged commit"
    );
}

#[test]
fn a_commit_whose_time_cannot_be_read_stops_the_walk_not_the_build() {
    let repo = Repo::new("badtime");
    repo.page("home.md", "Home", "# Home\n");
    let c1 = repo.commit("one", D1);
    let tree = git_at(&repo.dir, &["rev-parse", "HEAD^{tree}"], None, None);
    let raw = format!(
        "tree {tree}\nparent {c1}\nauthor T <t@e> 1792515601 +0000\ncommitter T <t@e> notatime +0000\n\nbad\n"
    );
    let bad = git_at(
        &repo.dir,
        &[
            "hash-object",
            "-t",
            "commit",
            "-w",
            "--literally",
            "--stdin",
        ],
        None,
        Some(raw.as_bytes()),
    );
    repo.page("home.md", "Home", "# Home\n\nnew\n");
    git_at(&repo.dir, &["add", "-A", "."], None, None);
    let tree2 = git_at(&repo.dir, &["write-tree"], None, None);
    let top = git_at(
        &repo.dir,
        &["commit-tree", &tree2, "-p", &bad, "-m", "top"],
        Some(D3),
        None,
    );
    let out_scratch = scratch("badtime-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &["--rev", &top]);
    ok(&o);
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("incomplete"),
        "{}",
        all(&o)
    );
    // Whether the top commit changed home is a comparison with the bad
    // one, which is not walked: home is not dated.
    assert_eq!(dated(&out, "index.html"), None);
}

/// A tree object of `n` entries (`mode`, the name `prefix` then the
/// entry's number, one object `id`), written byte by byte.
fn raw_tree(repo: &Repo, n: usize, mode: &str, prefix: &[u8], id: &str) -> String {
    let id: Vec<u8> = (0..20)
        .map(|i| u8::from_str_radix(&id[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    let mut raw = Vec::new();
    for i in 0..n {
        raw.extend_from_slice(mode.as_bytes());
        raw.push(b' ');
        raw.extend_from_slice(prefix);
        raw.extend_from_slice(format!("{i:06}").as_bytes());
        raw.push(0);
        raw.extend_from_slice(&id);
    }
    git_at(
        &repo.dir,
        &["hash-object", "-t", "tree", "-w", "--literally", "--stdin"],
        None,
        Some(&raw),
    )
}

#[test]
fn every_entry_counts_directories_and_names_that_are_not_utf8_too() {
    // 250,001 entries of one tree, each an (empty) directory,
    // or each a name that is not UTF-8, are over the entry budget at the
    // 250,001st, before any of the directories is read.
    let repo = Repo::new("entries");
    let empty = git_at(&repo.dir, &["mktree"], None, Some(b""));
    let dirs = raw_tree(&repo, 250_001, "40000", b"d", &empty);
    let c = commit_of(&repo, &format!("040000 tree {dirs}\tdirs\n"));
    let out_scratch = scratch("entries-out");
    let out = out_scratch.join("out");
    let o = build(&repo, &out, &["--rev", &c]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    assert!(all(&o).contains("more than 250000 entries"), "{}", all(&o));
    let blob = git_at(
        &repo.dir,
        &["hash-object", "-w", "--stdin"],
        None,
        Some(b"x"),
    );
    let names = raw_tree(&repo, 250_001, "100644", b"\xff", &blob);
    let c = commit_of(&repo, &format!("040000 tree {names}\tnames\n"));
    let o = build(&repo, &out, &["--rev", &c]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    let report = all(&o);
    assert!(
        report.contains("more than 250000 entries"),
        "{}",
        &report[..2000]
    );
    assert!(report.matches("path-not-utf8").count() <= rubrica_site::MAX_REPO_FILES);
}

// ---------- objects rebuilt from their delta chains ----------

/// A clone of `repo`, packed by `args` (`gc --aggressive`, `repack ...`).
fn packed_copy(repo: &Repo, tag: &str, args: &[&str]) -> Repo {
    let base = scratch(tag);
    let dir = base.join("clone");
    let url = format!("file://{}", repo.dir.display());
    git_at(
        dir.parent().unwrap(),
        &["clone", "-q", &url, "clone"],
        None,
        None,
    );
    git_at(&dir, args, None, None);
    Repo {
        dir,
        _scratch: base,
    }
}

#[test]
fn a_page_delta_compressed_against_a_blob_too_large_to_rebuild_is_refused_by_name() {
    // Decided: a page git delta-compressed against a 40 MB blob in
    // the history (its buffer is twice that, over the 64 MiB limit) is
    // refused, exit 1, naming it and how to repack; never exit 2. Repacked
    // so, it builds.
    let repo = Repo::new("bigbase");
    let mut text = String::new();
    let mut x: u32 = 7;
    while text.len() < 40_000_000 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        text.push_str(["alpha ", "beta ", "gamma ", "delta\n", "epsilon "][(x >> 16) as usize % 5]);
        text.push_str(&format!("{} ", x % 9973));
    }
    repo.write("big.txt", &text);
    repo.page("home.md", "Home", "# Home\n");
    let step = text.len() / 10;
    for i in 0..10 {
        repo.page(
            &format!("s{i}.md"),
            &format!("S{i}"),
            &text[i * step..(i + 1) * step],
        );
    }
    repo.commit("with big.txt", D1);
    git_at(&repo.dir, &["rm", "-q", "big.txt"], None, None);
    repo.commit("without", D2);
    let packed = packed_copy(
        &repo,
        "bigbase-packed",
        &["repack", "-q", "-adf", "--window=4095"],
    );
    let out_scratch = scratch("bigbase-out");
    let out = out_scratch.join("out");
    let o = build(&packed, &out, &[]);
    assert_eq!(code(&o), 1, "{}", all(&o));
    let s0 = git_at(&packed.dir, &["rev-parse", "HEAD:s0.md"], None, None);
    let text = all(&o);
    assert!(
        text.contains(&format!(
            "blob {s0} (s0.md) cannot be rebuilt within the memory limit of 67108864 bytes (too large, or the pack is damaged)"
        )) && text.contains("Run `git fsck`")
            && text.contains("`git config core.bigFileThreshold 16m && git repack -adf` in the repository the build clones from (for CI, the repository CI clones")
            && text.contains("`-a -d` drops unreachable packed objects"),
        "{text}"
    );
    assert!(!out.exists());
    // The remedy as the refusal gives it: persistent, then the repack.
    git_at(
        &packed.dir,
        &["config", "core.bigFileThreshold", "16m"],
        None,
        None,
    );
    git_at(&packed.dir, &["repack", "-q", "-adf"], None, None);
    ok(&build(&packed, &out, &[]));
}
