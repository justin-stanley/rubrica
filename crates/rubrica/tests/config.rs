//! `site.toml`'s `[site]` section, read by `rubrica build --config` and
//! `rubrica check --config`: the profile, the ignore lists, the wiki's own
//! origin and Wiki.js's limits, and the refusal of a key it does not know.
//! The repositories are made with the `git` command, as `tests/build.rs`
//! makes them.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use rubrica_site::build::ConfigFile;
use rubrica_site::SiteConfig;

/// A directory under the system temp dir, removed (with all it holds) when
/// dropped, so a passing run leaves nothing in TMPDIR.
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

fn scratch(tag: &str) -> Scratch {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rubrica-config-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The real path: a symbolic link in it (macOS's /var) is refused.
    Scratch(dir.canonicalize().unwrap())
}

/// `git` in `dir`, isolated from the user's and the system's config.
fn git_at(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .expect("git");
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
    scratch: Scratch,
}

impl Repo {
    fn new(tag: &str) -> Self {
        let base = scratch(tag);
        let dir = base.join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        git_at(&dir, &["init", "-q", "-b", "main"]);
        Self { dir, scratch: base }
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

    /// A `site.toml` outside the repository, so it is no page and no
    /// finding; its path. Each call writes a file of its own.
    fn config(&self, text: &str) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = self
            .scratch
            .join(format!("site-{}.toml", N.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&p, text).unwrap();
        p
    }

    fn commit(&self) {
        git_at(&self.dir, &["add", "-A", "."]);
        git_at(
            &self.dir,
            &["commit", "-q", "--no-verify", "--allow-empty", "-m", "x"],
        );
    }

    /// `rubrica build --config <config>` into a fresh directory; the
    /// output and where the site went.
    fn build(&self, config: &Path, tag: &str) -> (Output, PathBuf) {
        let out = self.scratch.join(format!("out-{tag}"));
        let o = rubrica(&[
            "build",
            "--config",
            config.to_str().unwrap(),
            self.dir.to_str().unwrap(),
            out.to_str().unwrap(),
        ]);
        (o, out)
    }

    /// `rubrica check --config <config> <extra…> --format json`.
    fn check(&self, config: &Path, extra: &[&str]) -> Output {
        let mut args = vec!["check", "--config", config.to_str().unwrap()];
        args.extend_from_slice(extra);
        args.extend_from_slice(&["--format", "json", self.dir.to_str().unwrap()]);
        rubrica(&args)
    }
}

fn rubrica(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .args(args)
        .current_dir(env!("CARGO_TARGET_TMPDIR"))
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .expect("rubrica runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn json(o: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(o)).unwrap_or_else(|e| panic!("{e}: {}", stdout(o)))
}

/// The two header links `BuildConfig::default()` has; a `site.toml` that
/// says them gives the default build half.
const HEADER: &str = "name = \"Wiki\"\n\n[[header_links]]\nlabel = \"Recent changes\"\nhref = \"/_recent\"\n\n[[header_links]]\nlabel = \"Search\"\nhref = \"/_search\"\n";

fn site_toml(site: &str) -> String {
    format!("{HEADER}\n[site]\n{site}")
}

#[test]
fn site_toml_selects_the_profile_and_the_site_config() {
    let repo = Repo::new("profile");
    repo.page("home.md", "Home", "~x~\n")
        .page("notes.md", "Notes", "notes\n")
        .write("README.md", "# readme\n")
        .commit();

    // Compat unstrikes a single tilde; native strikes it. Both name the profile.
    let compat = repo.config(&site_toml("render = \"wikijs-compat\"\n"));
    let (o, out) = repo.build(&compat, "compat");
    assert!(o.status.success(), "{}", stderr(&o));
    let html = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(html.contains("~x~"), "{html}");
    assert!(!html.contains("<del>x</del>"), "{html}");

    let native = repo.config(&site_toml("render = \"native\"\n"));
    let (o, out) = repo.build(&native, "native");
    assert!(o.status.success(), "{}", stderr(&o));
    let html = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert!(html.contains("<del>x</del>"), "{html}");

    // `ignore_files` replaces the default list: notes.md is no page.
    let ignoring = repo.config(&site_toml(
        "render = \"native\"\nignore_files = [\"README.md\", \"notes.md\"]\n",
    ));
    let (o, out) = repo.build(&ignoring, "ignoring");
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(out.join("index.html").exists());
    assert!(!out.join("notes.html").exists());
    // Without it, notes.md is a page.
    let (o, out) = repo.build(&native, "notes");
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(out.join("notes.html").exists());

    // `wiki_origin` reads `<origin>/…` as the internal link `/…`.
    let linked = Repo::new("origin");
    linked
        .page("home.md", "Home", "[x](https://wiki.example.org/missing)\n")
        .commit();
    let without = linked.config(&site_toml("render = \"native\"\n"));
    let o = linked.check(&without, &[]);
    assert!(o.status.success(), "{}", stderr(&o));
    let kinds = |o: &Output| -> Vec<String> {
        json(o)["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["kind"].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(
        !kinds(&o).iter().any(|k| k == "broken-page-link"),
        "{:?}",
        kinds(&o)
    );
    let with = linked.config(&site_toml(
        "render = \"native\"\nwiki_origin = \"https://wiki.example.org\"\n",
    ));
    let o = linked.check(&with, &[]);
    assert!(
        kinds(&o).iter().any(|k| k == "broken-page-link"),
        "{:?} {}",
        kinds(&o),
        stderr(&o)
    );
}

#[test]
fn an_unknown_or_removed_config_key_is_refused_by_name() {
    let repo = Repo::new("unknown");
    repo.page("home.md", "Home", "text\n").commit();

    let bogus = repo.config(&site_toml("bogus = 1\n"));
    let (o, out) = repo.build(&bogus, "bogus");
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).contains("bogus"), "{}", stderr(&o));
    assert!(!out.exists());
    let o = repo.check(&bogus, &[]);
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    assert!(stderr(&o).contains("bogus"), "{}", stderr(&o));

    let bad = repo.config(&site_toml("render = \"markdown-it\"\n"));
    let (o, _) = repo.build(&bad, "bad");
    assert_eq!(o.status.code(), Some(2), "{}", stderr(&o));
    let err = stderr(&o);
    assert!(err.contains("native"), "{err}");
    assert!(err.contains("wikijs-compat"), "{err}");
}

#[test]
fn check_accepts_the_same_config_file() {
    let repo = Repo::new("check");
    repo.page("home.md", "Home", "[o](/other#nope)\n")
        .page("other.md", "Other", "text\n")
        .commit();
    let config = repo.config(&site_toml("render = \"wikijs-compat\"\n"));

    let (b, _) = repo.build(&config, "check");
    assert!(b.status.success(), "{}", stderr(&b));
    let line = stderr(&b)
        .lines()
        .find(|l| l.starts_with("rubrica build: ") && l.contains("finding(s)"))
        .unwrap_or_else(|| panic!("no findings line: {}", stderr(&b)))
        .to_owned();
    let n: u64 = line
        .trim_start_matches("rubrica build: ")
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(n > 0, "{line}");

    let o = repo.check(&config, &[]);
    assert!(o.status.success(), "{}", stderr(&o));
    let r = json(&o);
    assert_eq!(r["options"]["wikijs_limits"], false, "{r}");
    assert_eq!(
        r["errors"].as_u64().unwrap() + r["warnings"].as_u64().unwrap(),
        n,
        "{r}"
    );

    let o = repo.check(&config, &["--wikijs-limits"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(json(&o)["options"]["wikijs_limits"], true);
}

#[test]
fn site_defaults_equal_site_config_default() {
    let parsed = toml::from_str::<ConfigFile>("name = \"Wiki\"\n\n[site]\n");
    assert!(parsed.is_ok(), "{parsed:?}");
    let (_, site) = parsed.unwrap().into_configs().unwrap();
    assert_eq!(site, SiteConfig::default());
}
