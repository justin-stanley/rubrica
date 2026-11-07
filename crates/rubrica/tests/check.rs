//! `rubrica check`, run as the binary on small trees the tests write: exit codes, the text
//! report (which mirrors `check-anchors.cjs`'s), the JSON report, and what it reads from a
//! repository directory and what it never reads.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures")).join(name)
}

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// `rubrica <args>`, with a time limit (a read that blocks or a file read
/// whole would run past it).
fn rubrica(args: &[&str], dir: &Path) -> Out {
    rubrica_within(args, dir, Duration::from_secs(60))
}

fn rubrica_within(args: &[&str], dir: &Path, limit: Duration) -> Out {
    rubrica_env(args, dir, limit, &[])
}

fn rubrica_env(args: &[&str], dir: &Path, limit: Duration, env: &[(&str, &str)]) -> Out {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .args(args)
        .envs(env.iter().copied())
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run rubrica");
    let start = Instant::now();
    loop {
        if child.try_wait().expect("wait").is_some() {
            break;
        }
        if start.elapsed() > limit {
            child.kill().ok();
            panic!("rubrica {args:?} {} ran past {limit:?}", dir.display());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = child.wait_with_output().expect("output");
    Out {
        code: out.status.code().expect("an exit code, not a signal"),
        stdout: String::from_utf8(out.stdout).expect("UTF-8 stdout"),
        stderr: String::from_utf8(out.stderr).expect("UTF-8 stderr"),
    }
}

fn check(args: &[&str], dir: &Path) -> Out {
    let mut all = vec!["check"];
    all.extend_from_slice(args);
    rubrica(&all, dir)
}

/// A fresh, empty directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("check-{name}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear the scratch dir");
    }
    std::fs::create_dir_all(&dir).expect("make the scratch dir");
    dir
}

fn write(dir: &Path, rel: &str, src: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, src).unwrap();
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Report {
    format: u32,
    options: Options,
    pages: usize,
    links: usize,
    errors: usize,
    warnings: usize,
    same_page_unchecked: usize,
    failed: bool,
    findings: Vec<ReportFinding>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct Options {
    strict: bool,
    same_page: bool,
    wikijs_limits: bool,
}

#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ReportFinding {
    file: String,
    line: usize,
    severity: String,
    kind: String,
    href: Option<String>,
    message: String,
    fix: Option<String>,
    hint: Option<String>,
    other_file: Option<String>,
    other_line: Option<usize>,
}

fn report(out: &Out) -> Report {
    serde_json::from_str(&out.stdout)
        .unwrap_or_else(|e| panic!("not the JSON report ({e}):\n{}", out.stdout))
}

fn kinds(r: &Report) -> Vec<String> {
    r.findings
        .iter()
        .map(|f| format!("{}:{}:{}:{}", f.file, f.line, f.severity, f.kind))
        .collect()
}

#[test]
fn a_clean_tree_exits_zero_and_prints_counts() {
    let out = check(&[], &fixture("check/clean"));
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert_eq!(
        out.stdout,
        "rubrica check: OK — 3 page(s), 4 link(s); no findings.\n"
    );
    let out = check(
        &[
            "--strict",
            "--same-page",
            "--wikijs-limits",
            "--format",
            "json",
        ],
        &fixture("check/clean"),
    );
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let r = report(&out);
    assert_eq!(
        (r.format, r.pages, r.links, r.errors, r.warnings, r.failed),
        (1, 3, 4, 0, 0, false)
    );
    assert!(r.options.strict && r.options.same_page && r.options.wikijs_limits);
    assert!(r.findings.is_empty());
}

#[test]
fn a_tree_with_no_page_is_an_error() {
    // `rubrica check` on an empty directory (a
    // wrong path, a checkout that failed) passed. No published page is an
    // error: drafts and files that are no pages do not count.
    let dir = scratch("no-pages");
    let out = check(&[], &dir);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout.contains("  .:1: error no-pages\n"),
        "{}",
        out.stdout
    );
    write(&dir, "README.md", "# Readme\n");
    write(&dir, "notes.txt", "x");
    write(&dir, "draft.md", "---\ntitle: D\npublished: false\n---\n");
    let out = check(&["--format", "json"], &dir);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    let r = report(&out);
    assert_eq!(kinds(&r), [".:1:error:no-pages"]);
    assert_eq!((r.pages, r.errors, r.failed), (0, 1, true));
    // One page is enough.
    write(&dir, "home.md", "---\ntitle: Home\n---\n");
    assert_eq!(check(&[], &dir).code, 0);
}

#[test]
fn a_dead_anchor_exits_one_under_strict_and_zero_without() {
    let dir = fixture("check/dead-anchor");
    // Warn-only by default, as check-anchors.cjs is: reported, exit 0. The
    // same-page one is not checked without --same-page, and the report
    // says how many were left out.
    let out = check(&[], &dir);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout.contains(
            "  home.md:6: warning broken-anchor\n    ](/alpha#capacity--memory)\n    no heading on /alpha has id \"capacity--memory\"\n    fix: ](/alpha#capacity-memory)\n    (heading is \"Capacity & memory\" (line 6))\n"
        ),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("NOTE — 1 dead same-page ](#anchor) link(s) not reported; run with --same-page to include them.\n"),
        "{}",
        out.stdout
    );
    assert!(out
        .stdout
        .ends_with("rubrica check: OK with 1 warning(s); --strict fails on them.\n"));
    // Strict: an error, exit 1.
    let out = check(&["--strict"], &dir);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.contains("  home.md:6: error broken-anchor\n"));
    assert!(out
        .stdout
        .ends_with("rubrica check FAILED: 1 error(s), 0 warning(s).\n"));
    // --same-page: both.
    let r = report(&check(&["--same-page", "--format", "json"], &dir));
    assert_eq!(
        kinds(&r),
        [
            "alpha.md:8:warning:broken-anchor",
            "home.md:6:warning:broken-anchor"
        ]
    );
    assert_eq!((r.same_page_unchecked, r.failed), (0, false));
    let r = report(&check(&["--format", "json"], &dir));
    assert_eq!(kinds(&r), ["home.md:6:warning:broken-anchor"]);
    assert_eq!(r.same_page_unchecked, 1);
    let out = check(&["--strict", "--same-page", "--format", "json"], &dir);
    assert_eq!(out.code, 1);
    let r = report(&out);
    assert_eq!((r.errors, r.warnings, r.failed), (2, 0, true));
}

#[test]
fn a_warning_fails_under_strict() {
    // Not only dead anchors: any warning fails the check under --strict.
    let dir = scratch("warning-strict");
    write(
        &dir,
        "home.md",
        "---\ntitle: Home\n---\n[draft](/draft) and [pic](/p.png)\n",
    );
    write(
        &dir,
        "draft.md",
        "---\ntitle: Draft\npublished: false\n---\n",
    );
    let out = check(&[], &dir);
    assert_eq!(out.code, 0, "{}", out.stdout);
    let out = check(&["--strict"], &dir);
    assert_eq!(out.code, 1, "{}", out.stdout);
    assert!(out
        .stdout
        .ends_with("rubrica check FAILED (--strict): 0 error(s), 2 warning(s).\n"));
}

#[test]
fn a_relative_link_to_an_existing_page_is_an_error_with_the_absolute_fix() {
    let out = check(&[], &fixture("check/relative"));
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout.contains(
            "  home.md:6: error relative-link\n    ](beta)\n    a relative link to beta.md: Wiki.js resolves it against the page as a directory, a browser against the page's directory\n    fix: ](/beta)\n"
        ),
        "{}",
        out.stdout
    );
    // A relative link to no file is not a finding (check-frontmatter.cjs's
    // rule), and an absolute one is fine.
    let r = report(&check(&["--format", "json"], &fixture("check/relative")));
    assert_eq!(kinds(&r), ["home.md:6:error:relative-link"]);
    assert_eq!(r.findings[0].fix.as_deref(), Some("/beta"));
}

#[test]
fn a_duplicate_sec_id_names_both_files() {
    let out = check(&[], &fixture("check/duplicate-sec"));
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout.contains(
            "  beta.md:6: error duplicate-sec-id\n    @sec id \"ops/shared\" is already used at alpha.md:4\n"
        ),
        "{}",
        out.stdout
    );
    let r = report(&check(
        &["--format", "json"],
        &fixture("check/duplicate-sec"),
    ));
    let f = &r.findings[0];
    assert_eq!(
        (
            f.file.as_str(),
            f.line,
            f.other_file.as_deref(),
            f.other_line
        ),
        ("beta.md", 6, Some("alpha.md"), Some(4))
    );
}

#[test]
fn wikijs_limits_flags_a_256_char_description_counted_in_code_points() {
    // The em-dash case of the wiki's pre-receive hook: 256 em-dashes are 768
    // bytes and 256 characters, and 256 is one over varchar(255); 255 pass.
    let dir = fixture("check/wikijs-limits");
    let out = check(&[], &dir);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.starts_with("rubrica check: OK"));
    let out = check(&["--wikijs-limits"], &dir);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout.contains(
            "  long.md:3: error too-long-for-wikijs\n    description is 256 chars (max 255): Wiki.js would drop the page\n"
        ),
        "{}",
        out.stdout
    );
    let r = report(&check(&["--wikijs-limits", "--format", "json"], &dir));
    assert_eq!(kinds(&r), ["long.md:3:error:too-long-for-wikijs"]);
}

#[test]
fn json_output_is_stable() {
    // The golden is the format: field names, order, kinds, messages. Two runs
    // are byte-identical.
    let dir = fixture("check/mixed");
    let want = std::fs::read_to_string(fixture("check/mixed.json")).unwrap();
    let a = check(&["--same-page", "--format", "json"], &dir);
    let b = check(&["--format", "json", "--same-page"], &dir);
    assert_eq!(a.code, 1, "{}", a.stderr);
    assert_eq!(a.stdout, b.stdout, "two runs differ");
    if a.stdout != want {
        panic!(
            "JSON report differs from fixtures/check/mixed.json:\n--- want\n{want}\n--- got\n{}",
            a.stdout
        );
    }
    // Every finding is reported once, sorted by file and line.
    let r = report(&a);
    let mut sorted = r.findings.clone();
    sorted.sort_by(|x, y| (&x.file, x.line).cmp(&(&y.file, y.line)));
    assert_eq!(sorted, r.findings);
    assert!(a.stderr.is_empty(), "{}", a.stderr);
}

#[test]
fn text_output_mirrors_check_anchors() {
    let dir = fixture("check/mixed");
    let want = std::fs::read_to_string(fixture("check/mixed.txt")).unwrap();
    let out = check(&["--same-page"], &dir);
    assert_eq!(out.code, 1);
    if out.stdout != want {
        panic!(
            "text report differs from fixtures/check/mixed.txt:\n--- want\n{want}\n--- got\n{}",
            out.stdout
        );
    }
}

#[test]
fn usage_and_io_errors_exit_two() {
    let missing = scratch("io").join("nothing-here");
    let out = check(&[], &missing);
    assert_eq!(out.code, 2, "{}{}", out.stdout, out.stderr);
    assert!(out.stdout.is_empty());
    assert!(out.stderr.contains("rubrica check:"), "{}", out.stderr);
    let file = scratch("io-file").join("a.md");
    std::fs::write(&file, "x").unwrap();
    assert_eq!(check(&[], &file).code, 2);
    assert_eq!(
        check(&["--format", "yaml"], &fixture("check/clean")).code,
        2
    );
    assert_eq!(check(&["--bogus"], &fixture("check/clean")).code, 2);
    let out = rubrica(&["check"], Path::new("--strict"));
    assert_eq!(out.code, 2, "a missing <repo-dir> is a usage error");
}

/// The characters the text report must never print raw:
/// C0 and C1 controls, DEL, the bidi embeddings, overrides and isolates,
/// and the line and paragraph separators.
fn hostile(c: char) -> bool {
    matches!(c, '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}')
        || invisible(c)
}

/// The invisible format characters the text report escapes too: zero-width space, non-joiner and joiner, the word joiner
/// and invisible operators, the deprecated format controls, the BOM, the
/// tag characters.
fn invisible(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200d}' | '\u{2060}'..='\u{2064}' | '\u{206a}'..='\u{206f}' | '\u{feff}' | '\u{e0000}'..='\u{e007f}')
}

#[test]
fn text_output_escapes_hostile_characters_and_json_keeps_them_raw() {
    let dir = scratch("hostile");
    let evil = "evil\u{1b}[2J\u{7}\u{202e}\u{2028}\u{85}\u{7f}\u{2066}\u{200b}\u{feff}\u{e0041}";
    // A file name (no page, so its path is never character-checked: only
    // printed), a link's href and a heading's text, each with every class.
    std::fs::create_dir_all(dir.join(format!("{evil}.d"))).unwrap();
    std::os::unix::fs::symlink("/nowhere", dir.join(format!("{evil}.md"))).unwrap();
    write(
        &dir,
        "home.md",
        &format!("---\ntitle: Home\n---\n## Evil {evil} heading\n\n[x](</{evil}>) [y](/home#evil-heading)\n"),
    );
    let out = check(&["--same-page"], &dir);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    let raw: Vec<char> = out
        .stdout
        .chars()
        .filter(|&c| c != '\n' && hostile(c))
        .collect();
    assert!(raw.is_empty(), "printed raw: {raw:?}\n{}", out.stdout);
    for escaped in [
        "\\u{1b}",
        "\\u{7}",
        "\\u{202e}",
        "\\u{2028}",
        "\\u{85}",
        "\\u{7f}",
        "\\u{2066}",
        "\\u{200b}",
        "\\u{feff}",
        "\\u{e0041}",
    ] {
        assert!(
            out.stdout.contains(escaped),
            "no {escaped}:\n{}",
            out.stdout
        );
    }
    assert!(
        out.stdout.contains(&format!(
            "  {}.md:1: error not-a-regular-file\n",
            evil.chars()
                .map(|c| if hostile(c) {
                    format!("\\u{{{:x}}}", c as u32)
                } else {
                    c.to_string()
                })
                .collect::<String>()
        )),
        "{}",
        out.stdout
    );
    assert!(out.stderr.chars().all(|c| c == '\n' || !hostile(c)));
    // JSON keeps them: what serde_json reads back is the file name, the
    // href and the heading text as they are.
    let r = report(&check(&["--same-page", "--format", "json"], &dir));
    let files: Vec<&str> = r.findings.iter().map(|f| f.file.as_str()).collect();
    assert!(files.contains(&format!("{evil}.md").as_str()), "{files:?}");
    let hrefs: Vec<&str> = r
        .findings
        .iter()
        .filter_map(|f| f.href.as_deref())
        .collect();
    assert!(hrefs.contains(&format!("/{evil}").as_str()), "{hrefs:?}");
    assert!(
        r.findings
            .iter()
            .filter_map(|f| f.hint.as_deref())
            .any(|h| h.contains(&format!("Evil {evil} heading"))),
        "{:?}",
        r.findings
    );
}

#[test]
fn symlinks_are_never_followed_and_special_files_never_read() {
    let root = scratch("links");
    let dir = root.join("repo");
    let outside = root.join("outside");
    write(
        &outside,
        "secret.md",
        "---\ntitle: SECRET\n---\n[leak](/leaked-target)\n<!-- @sec id=leaked/id -->\n## LEAKED\n",
    );
    write(
        &outside,
        "pages/inner.md",
        "---\ntitle: INNER\n---\n[leak](/leaked-inner)\n",
    );
    write(&dir, "home.md", "---\ntitle: Home\n---\n# Home\n");
    std::os::unix::fs::symlink(outside.join("secret.md"), dir.join("leak.md")).unwrap();
    std::os::unix::fs::symlink(outside.join("pages"), dir.join("linked")).unwrap();
    std::os::unix::fs::symlink(outside.join("pages"), dir.join("assets")).unwrap();
    // A FIFO: opening it to read would block forever.
    let fifo = dir.join("fifo.md");
    let st = Command::new("mkfifo").arg(&fifo).status().expect("mkfifo");
    assert!(st.success());
    // A socket: bound at a short path (a socket's path has a length limit),
    // then moved in, if the file system lets it move.
    let short = std::env::temp_dir().join(format!("rubrica-sock-{}", std::process::id()));
    let _ = std::fs::remove_file(&short);
    // (A long TMPDIR is past that limit: no socket case then.)
    let sock = std::os::unix::net::UnixListener::bind(&short);
    let socket = sock.is_ok() && std::fs::rename(&short, dir.join("sock.md")).is_ok();
    if !socket {
        std::fs::remove_file(&short).ok();
        eprintln!("check: no socket could be bound and moved into the scratch dir; no socket case");
    }
    let out = rubrica_within(
        &["check", "--format", "json"],
        &dir,
        Duration::from_secs(20),
    );
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    let r = report(&out);
    // A symlinked directory is not walked; `linked` and `assets`, where
    // pages and assets could be, are reported: what
    // they would hold is never read.
    let mut want = vec![
        "assets:1:error:not-a-regular-file",
        "fifo.md:1:error:not-a-regular-file",
        "leak.md:1:error:not-a-regular-file",
        "linked:1:error:not-a-regular-file",
    ];
    if socket {
        want.push("sock.md:1:error:not-a-regular-file");
    }
    assert_eq!(kinds(&r), want);
    let linked = r.findings.iter().find(|f| f.file == "linked").unwrap();
    assert!(
        linked.message.contains("nothing under it is read"),
        "{}",
        linked.message
    );
    assert_eq!(r.pages, 1);
    for leaked in ["leaked", "LEAKED", "SECRET", "INNER", "inner"] {
        assert!(
            !out.stdout.contains(leaked),
            "{leaked} was read:\n{}",
            out.stdout
        );
    }
}

#[test]
fn files_over_the_cap_are_refused_unread() {
    // Sparse files of 1 TiB: read whole, they would take hours and the
    // memory; at 8 GiB a fast machine reads three in seconds, which proves nothing.
    // A page or an asset over the cap is an error, never read; a file the
    // site reads nothing of (not `.md`, not under `assets/`) is not read
    // either, whatever its size, and is no finding.
    let dir = scratch("big");
    write(&dir, "home.md", "---\ntitle: Home\n---\n[big](/big)\n");
    let sparse = |rel: &str| {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(&path)
            .unwrap()
            .set_len(1 << 40)
            .unwrap();
    };
    sparse("big.md");
    sparse("assets/huge.png");
    sparse("data/huge.bin");
    sparse("node_modules/x/huge.md");
    let start = Instant::now();
    let out = rubrica_within(
        &["check", "--format", "json"],
        &dir,
        Duration::from_secs(20),
    );
    let took = start.elapsed();
    assert!(
        took < Duration::from_secs(5),
        "{took:?}: a big file was read"
    );
    let r = report(&out);
    assert_eq!(
        kinds(&r),
        [
            "assets/huge.png:1:error:file-too-large",
            "big.md:1:error:file-too-large",
            "home.md:4:error:broken-page-link",
        ]
    );
    // A page at the cap is read.
    let dir = scratch("at-cap");
    let head = "---\ntitle: Home\n---\n";
    let cap = 4 * 1024 * 1024 + 64 * 1024;
    write(
        &dir,
        "home.md",
        &format!("{head}{}", "a".repeat(cap - head.len())),
    );
    let r = report(&check(&["--format", "json"], &dir));
    assert_eq!(r.pages, 1, "{:?}", kinds(&r));
    write(
        &dir,
        "home.md",
        &format!("{head}{}", "a".repeat(cap + 1 - head.len())),
    );
    let r = report(&check(&["--format", "json"], &dir));
    // Its only page refused, the tree has none.
    assert_eq!(
        kinds(&r),
        [".:1:error:no-pages", "home.md:1:error:file-too-large"]
    );
}

/// The files `rubrica check` opened on `dir`, sorted: the test build
/// names each on stderr when `RUBRICA_TRACE_OPENS` is set.
fn opened(dir: &Path) -> Vec<String> {
    let out = rubrica_env(
        &["check", "--format", "json"],
        dir,
        Duration::from_secs(60),
        &[("RUBRICA_TRACE_OPENS", "1")],
    );
    let mut v: Vec<String> = out
        .stderr
        .lines()
        .filter_map(|l| l.strip_prefix("rubrica: open "))
        .map(str::to_owned)
        .collect();
    v.sort();
    v
}

#[test]
fn a_file_the_site_reads_nothing_of_is_never_opened() {
    // Only what the site or check-packages.cjs uses is opened: `.md` files
    // outside `node_modules`, `.git` and hidden directories (pages,
    // `README.md`), and assets. Any other file
    // is a path only. The test build names every file it opens, so this
    // holds as root (CI), where a mode-000 file opens anyway.
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("unopened");
    write(&dir, "home.md", "---\ntitle: Home\n---\n");
    write(&dir, "README.md", "# Readme\n");
    write(&dir, "scripts/notes.md", "---\ntitle: Notes\n---\nnotes\n");
    write(&dir, "assets/p.png", "png");
    write(&dir, "data/locked.bin", "x");
    write(&dir, "packages.yaml", "members: []\n");
    write(&dir, "scripts/node_modules/x/locked.js", "x");
    write(&dir, "scripts/node_modules/x/README.md", "x");
    write(&dir, "node_modules/y/z.md", "x");
    write(&dir, ".github/notes.md", "x");
    if cfg!(debug_assertions) {
        assert_eq!(
            opened(&dir),
            ["README.md", "assets/p.png", "home.md", "scripts/notes.md"]
        );
    } else {
        eprintln!("check: a release build has no open trace; the mode-000 half only");
    }
    // And one that cannot be opened (mode 000) is no error, so it was not
    // opened: 276 such .md files sit under the wiki's scripts/node_modules.
    for f in [
        "data/locked.bin",
        "scripts/node_modules/x/locked.js",
        "scripts/node_modules/x/README.md",
        "node_modules/y/z.md",
    ] {
        std::fs::set_permissions(dir.join(f), std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    let out = check(&[], &dir);
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    // A page that cannot be opened is an I/O error: exit 2, not a pass.
    write(&dir, "locked.md", "---\ntitle: L\n---\n");
    std::fs::set_permissions(
        dir.join("locked.md"),
        std::fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    let out = check(&[], &dir);
    let root = std::fs::File::open(dir.join("locked.md")).is_ok();
    if !root {
        assert_eq!(out.code, 2, "{}{}", out.stdout, out.stderr);
        assert!(out.stderr.contains("rubrica check:"), "{}", out.stderr);
        assert!(out.stderr.contains("locked.md: "), "{}", out.stderr);
    }
}

#[test]
fn the_root_git_directory_is_skipped_and_a_nested_checkout_is_not_read() {
    let dir = scratch("git");
    write(
        &dir,
        "home.md",
        "---\ntitle: Home\n---\n[v](/vendor/sub/page)\n",
    );
    write(&dir, ".git/HEAD", "ref: refs/heads/main\n");
    write(&dir, ".git/x.md", "not front matter\n");
    write(&dir, "vendor/sub/.git", "gitdir: ../../.git/modules/sub\n");
    write(&dir, "vendor/sub/page.md", "---\ntitle: Sub\n---\n");
    // Walked, the root's .git would be a path past the limits.
    write(&dir, &format!(".git/{}/x", vec!["o"; 70].join("/")), "x");
    let r = report(&check(&["--format", "json"], &dir));
    assert_eq!(r.pages, 1);
    // The nested checkout hides its pages, so it is reported, and the link into it is broken.
    assert_eq!(
        kinds(&r),
        [
            "home.md:4:error:broken-page-link",
            "vendor/sub:1:error:not-a-regular-file"
        ]
    );
}

#[test]
fn paths_over_the_limits_are_reported_once_and_not_walked() {
    // A directory chain past MAX_PATH_SEGMENTS: the first directory over
    // the limit is reported, as a path too long, and nothing under it is
    // read; a file at the limit is a page.
    let dir = scratch("deep");
    let at = vec!["ddd"; 63].join("/");
    write(&dir, &format!("{at}/page.md"), "---\ntitle: Deep\n---\n");
    let over = vec!["e"; 70].join("/");
    write(
        &dir,
        &format!("{over}/page.md"),
        "---\ntitle: Deeper\n---\n",
    );
    let r = report(&check(&["--format", "json"], &dir));
    assert_eq!(r.pages, 1, "{:?}", kinds(&r));
    let over65 = vec!["e"; 65].join("/");
    assert_eq!(kinds(&r), [format!("{over65}:1:error:path-too-long")]);
}

#[test]
fn non_utf8_paths_are_reported_and_not_read() {
    use std::os::unix::ffi::OsStrExt;
    let dir = scratch("not-utf8");
    write(&dir, "home.md", "---\ntitle: Home\n---\n");
    // A finding that sorts before them: the reader's and the site's are one
    // list, sorted by file and line.
    write(&dir, "aaa.md", "---\ntitle: A\n---\n[x](/nowhere)\n");
    // A name holding the four characters `\xfe`: printed with its
    // backslash doubled, never as the byte 0xfe is.
    std::os::unix::fs::symlink("/nowhere", dir.join("lit\\xfe.md")).unwrap();
    let out = check(&[], &dir);
    assert!(
        out.stdout
            .contains("  lit\\\\xfe.md:1: error not-a-regular-file\n"),
        "{}",
        out.stdout
    );
    let name = std::ffi::OsStr::from_bytes(b"bad\xffname.md");
    let sub = std::ffi::OsStr::from_bytes(b"d\xfe");
    if let Err(e) = std::fs::write(dir.join(name), "---\ntitle: X\n---\n[l](/leak)\n") {
        // APFS refuses names that are not UTF-8 (EILSEQ); Linux keeps them.
        eprintln!("check: this file system refuses a non-UTF-8 name ({e}); the reader's escaping is unit-tested");
        return;
    }
    std::fs::create_dir(dir.join(sub)).unwrap();
    std::fs::write(
        dir.join(sub).join("p.md"),
        "---\ntitle: P\n---\n[l](/leak)\n",
    )
    .unwrap();
    // A file system may keep such a name but not its bytes (HFS+ writes
    // `%FF`; some normalise or substitute): the names read back must be the
    // names written, or the test has nothing to check here.
    let listed: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    if !listed.iter().any(|n| n == name) || !listed.iter().any(|n| n == sub) {
        eprintln!("check: this file system changed a non-UTF-8 name's bytes (read back {listed:?}); the reader's escaping is unit-tested");
        return;
    }
    let out = check(&[], &dir);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(
        out.stdout
            .contains("  bad\\xffname.md:1: error path-not-utf8\n"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("  d\\xfe:1: error path-not-utf8\n"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("  lit\\\\xfe.md:1: error not-a-regular-file\n"),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains("leak"));
    let r = report(&check(&["--format", "json"], &dir));
    // In JSON the reader's path is `lossless_display`'s (a byte as `\xNN`,
    // a backslash doubled); the site's paths are raw.
    assert_eq!(
        kinds(&r),
        [
            "aaa.md:4:error:broken-page-link",
            "bad\\xffname.md:1:error:path-not-utf8",
            "d\\xfe:1:error:path-not-utf8",
            "lit\\xfe.md:1:error:not-a-regular-file"
        ]
    );
}

#[test]
fn a_repository_over_the_byte_budget_is_reported_and_its_walk_stopped() {
    // The repository budget: 33 assets of 4 MiB are more than the
    // 128 MiB of file content a site is made from. The reader stops at the
    // first file over it, which the trace shows, and the site reports it.
    let dir = scratch("byte-budget");
    write(&dir, "home.md", "---\ntitle: Home\n---\n# Home\n");
    let chunk = vec![b'x'; 4 << 20];
    for i in 0..40 {
        let p = dir.join(format!("assets/{i:02}.png"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, &chunk).unwrap();
    }
    let out = rubrica_env(
        &["check", "--format", "json"],
        &dir,
        Duration::from_secs(60),
        &[("RUBRICA_TRACE_OPENS", "1")],
    );
    assert_eq!(out.code, 1, "{}", out.stdout);
    let r = report(&out);
    // The budget, and nothing else: no `no-pages`, which would send the
    // reader looking for the wrong mistake.
    assert_eq!(kinds(&r), [".:1:error:repository-over-budget"]);
    if cfg!(debug_assertions) {
        let opened = out.stderr.lines().filter(|l| l.contains("assets/")).count();
        assert!(
            opened <= 33,
            "{opened} assets opened: the walk did not stop"
        );
    }
}

#[test]
fn check_reports_the_highlighting_finding_as_a_warning() {
    let dir = scratch("highlight-cap");
    write(
        &dir,
        "home.md",
        &format!(
            "---\ntitle: Home\n---\n```rust\n// {}\n```\n",
            "x".repeat(1998)
        ),
    );
    let out = check(&["--format", "json"], &dir);
    eprintln!("{}", out.stdout);
    assert!(
        out.stdout.contains("code-block-not-highlighted"),
        "{}{}",
        out.stdout,
        out.stderr
    );
    assert_eq!(out.code, 0, "{}{}", out.stdout, out.stderr);
    let r = report(&out);
    assert_eq!((r.errors, r.warnings, r.failed), (0, 1, false));
    let f = &r.findings[0];
    assert_eq!(
        (
            f.file.as_str(),
            f.line,
            f.severity.as_str(),
            f.kind.as_str()
        ),
        ("home.md", 4, "warning", "code-block-not-highlighted")
    );
    assert_eq!(
        f.message,
        "code block (2001 bytes, longest line 2001) is over the highlighting cap (65536 bytes per block, 2000 bytes per line) and is written plain"
    );
    // A warning, so --strict fails on it like on any other.
    let out = check(&["--strict"], &dir);
    assert_eq!(out.code, 1, "{}{}", out.stdout, out.stderr);
    assert!(out
        .stdout
        .ends_with("rubrica check FAILED (--strict): 0 error(s), 1 warning(s).\n"));
}

#[test]
fn the_highlighting_message_names_no_line() {
    // The finding carries its line (the report prints the file's); the message does not
    // repeat it, as the body-relative number would disagree with the file's.
    let dir = scratch("highlight-message");
    write(
        &dir,
        "home.md",
        &format!(
            "---\ntitle: Home\n---\n```rust\n// {}\n```\n",
            "x".repeat(1998)
        ),
    );
    let out = check(&["--format", "json"], &dir);
    let r = report(&out);
    let f = &r.findings[0];
    assert_eq!(f.kind, "code-block-not-highlighted");
    assert_eq!(f.line, 4);
    assert!(!f.message.contains("at line"), "{}", f.message);
}
