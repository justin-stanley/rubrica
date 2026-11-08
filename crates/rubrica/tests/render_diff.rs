//! `tools/render-diff.sh`: two binaries build one repository, and the outputs are compared.
//!
//! The repository root is `git rev-parse --show-toplevel` from the crate's directory; outside
//! a checkout (a packaged crate) the tests skip, since the scripts are not shipped with it.

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
        "rubrica-rd-{tag}-{}-{}",
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
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .expect("git")
}

/// The checkout holding the crate, or `None` outside one.
fn repo_root() -> Option<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let top = String::from_utf8(out.stdout).ok()?;
    Some(PathBuf::from(top.trim_end_matches('\n')))
}

/// A committed repository of three pages.
fn three_page_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for (name, title) in [("home", "Home"), ("alpha", "Alpha"), ("beta", "Beta")] {
        std::fs::write(
            dir.join(format!("{name}.md")),
            format!("---\ntitle: {title}\n---\n# {title}\n\nSome text.\n"),
        )
        .unwrap();
    }
    for args in [
        &["init", "-q"][..],
        &["add", "-A"],
        &["commit", "-q", "-m", "pages"],
    ] {
        let out = git_at(dir, args);
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }
}

/// Runs `tools/render-diff.sh` from the repository root with `args`.
fn render_diff(root: &Path, args: &[&str]) -> Output {
    let script = root.join("tools/render-diff.sh");
    assert!(script.is_file(), "no such file: {}", script.display());
    Command::new(&script)
        .current_dir(root)
        .args(args)
        .output()
        .expect("run tools/render-diff.sh")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn render_diff_of_a_binary_with_itself_is_empty() {
    let Some(root) = repo_root() else {
        eprintln!("no checkout above the crate: render-diff skipped");
        return;
    };
    let tmp = scratch("same");
    let repo = tmp.0.join("repo");
    three_page_repo(&repo);
    let bin = env!("CARGO_BIN_EXE_rubrica");
    let out = render_diff(&root, &[bin, bin, repo.to_str().unwrap()]);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(all.contains("render-diff: 0 files differ"), "{all}");
}

#[test]
fn render_diff_of_two_different_outputs_is_reported() {
    let Some(root) = repo_root() else {
        eprintln!("no checkout above the crate: render-diff skipped");
        return;
    };
    let tmp = scratch("diff");
    let repo = tmp.0.join("repo");
    three_page_repo(&repo);
    let bin = env!("CARGO_BIN_EXE_rubrica");
    // The second "binary" builds with the real one, then appends a byte to home.html. The
    // output directory is the third argument (`build <repo> <out> ...`).
    let wrapper = tmp.0.join("wrapper.sh");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\n'{bin}' \"$@\" || exit $?\nprintf x >> \"$3/home.html\"\n"),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let out = render_diff(
        &root,
        &[bin, wrapper.to_str().unwrap(), repo.to_str().unwrap()],
    );
    let all = text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(all.contains("render-diff: 1 files differ"), "{all}");
    assert!(all.contains("home.html"), "{all}");
}

#[test]
fn render_diff_refuses_a_bad_argument() {
    let Some(root) = repo_root() else {
        eprintln!("no checkout above the crate: render-diff skipped");
        return;
    };
    let tmp = scratch("args");
    let repo = tmp.0.join("repo");
    three_page_repo(&repo);
    let repo = repo.to_str().unwrap();
    let bin = env!("CARGO_BIN_EXE_rubrica");
    let missing = tmp.0.join("missing");
    let missing = missing.to_str().unwrap();
    let not_exec = tmp.0.join("plain.txt");
    std::fs::write(&not_exec, "x").unwrap();
    let not_exec = not_exec.to_str().unwrap();
    let failing = tmp.0.join("failing.sh");
    std::fs::write(&failing, "#!/bin/sh\nexit 3\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&failing, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let failing = failing.to_str().unwrap();
    let cases: Vec<(Vec<&str>, &str)> = vec![
        (vec![], "usage"),
        (vec![bin], "usage"),
        (vec![bin, bin], "usage"),
        (vec![bin, bin, repo, "extra"], "usage"),
        (vec![bin, bin, repo, "--bogus", "x"], "unknown option"),
        (vec![bin, bin, repo, "--subtree"], "needs a value"),
        (vec![bin, bin, repo, "--config"], "needs a value"),
        (
            vec![bin, bin, repo, "--subtree", "a", "--subtree", "b"],
            "twice",
        ),
        (
            vec![bin, bin, repo, "--config", "a", "--config", "b"],
            "twice",
        ),
        (vec![bin, bin, repo, "--config", missing], "not a file"),
        (vec![missing, bin, repo], "not an executable"),
        (vec![bin, not_exec, repo], "not an executable"),
        (vec![bin, bin, missing], "not a directory"),
        (vec![bin, bin, repo, "--subtree", "nope"], "build failed"),
        (vec![failing, bin, repo], "build failed"),
    ];
    for (args, message) in cases {
        let out = render_diff(&root, &args);
        let all = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {all}");
        assert!(all.contains(message), "{args:?}: {all}");
        assert!(!all.contains("files differ"), "{args:?}: {all}");
    }
}

#[test]
fn the_spec_repo_has_one_page_per_example() {
    let Some(root) = repo_root() else {
        eprintln!("no checkout above the crate: render-diff skipped");
        return;
    };
    let script = root.join("tools/render-diff-spec-repo.sh");
    assert!(script.is_file(), "no such file: {}", script.display());
    let tmp = scratch("spec");
    let repo = tmp.0.join("spec");
    let run = || {
        Command::new(&script)
            .current_dir(&root)
            .arg(&repo)
            .output()
            .expect("run render-diff-spec-repo.sh")
    };
    let out = run();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let mut pages = 0;
    for dir in ["", "commonmark", "gfm"] {
        for entry in std::fs::read_dir(repo.join(dir)).unwrap() {
            let name = entry.unwrap().file_name();
            if name.to_string_lossy().ends_with(".md") {
                pages += 1;
            }
        }
    }
    assert_eq!(pages, 1_324 + 1);
    assert!(repo.join("home.md").is_file());
    let head = git_at(&repo, &["rev-parse", "HEAD"]);
    assert!(head.status.success(), "{head:?}");
    // Every page renders with no finding but the dead links the examples hold on purpose
    // (they point at pages that do not exist). --strict reports the same findings here.
    let check = Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .args(["check", "--strict", "--format", "json"])
        .arg(&repo)
        .output()
        .expect("run rubrica check");
    let report: serde_json::Value = serde_json::from_slice(&check.stdout)
        .unwrap_or_else(|e| panic!("check printed no JSON ({e}): {}", text(&check)));
    assert_eq!(report["pages"], 1_325, "{report}");
    let mut other = std::collections::BTreeSet::new();
    for f in report["findings"].as_array().expect("findings") {
        let kind = f["kind"].as_str().unwrap_or("?");
        if kind != "broken-page-link" {
            other.insert(kind.to_string());
        }
    }
    assert!(other.is_empty(), "findings of other kinds: {other:?}");
    assert_eq!(report["warnings"], 0, "{report}");
    // Deterministic: a second run writes the same commit.
    let repo2 = tmp.0.join("spec2");
    let out2 = Command::new(&script)
        .current_dir(&root)
        .arg(&repo2)
        .output()
        .expect("run render-diff-spec-repo.sh");
    assert_eq!(out2.status.code(), Some(0), "{}", text(&out2));
    let head2 = git_at(&repo2, &["rev-parse", "HEAD"]);
    assert_eq!(
        head.stdout, head2.stdout,
        "the spec repo is not deterministic"
    );
    // A non-empty target is refused.
    let again = run();
    assert_eq!(again.status.code(), Some(2), "{}", text(&again));
}

// ---------- tools/upgrade-check.sh ----------

/// A committed repository the upgrade check can run in: a docs page, a changelog with
/// `changelog` as the notes of 0.2.0, and a release-notes script.
fn upgrade_repo(dir: &Path, changelog: &str) {
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(
        dir.join("docs/home.md"),
        "---\ntitle: Home\n---\n# Home\n\nSome text.\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("CHANGELOG.md"),
        format!("# Changelog\n\n## [0.2.0] - 2026-12-01\n\n{changelog}\n"),
    )
    .unwrap();
    // The notes script the check calls: the body of the version's section.
    let notes = dir.join("scripts/release-notes.sh");
    std::fs::write(
        &notes,
        "#!/bin/sh\nawk -v h=\"## [$1]\" 'index($0, h) == 1 { on = 1; next } on && /^## / { exit } on' CHANGELOG.md\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&notes, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for args in [
        &["init", "-q"][..],
        &["add", "-A"],
        &["commit", "-q", "-m", "docs"],
    ] {
        let out = git_at(dir, args);
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }
}

/// Runs `tools/upgrade-check.sh` in `dir`; `env` is the test mode.
fn upgrade_check(root: &Path, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let script = root.join("tools/upgrade-check.sh");
    assert!(script.is_file(), "no such file: {}", script.display());
    Command::new(&script)
        .current_dir(dir)
        .args(args)
        .envs(env.iter().copied())
        .output()
        .expect("run tools/upgrade-check.sh")
}

#[test]
fn upgrade_check_with_no_previous_release_passes_and_says_so() {
    let Some(root) = repo_root() else {
        eprintln!("no checkout above the crate: upgrade-check skipped");
        return;
    };
    let tmp = scratch("upgrade-none");
    let repo = tmp.0.join("repo");
    upgrade_repo(&repo, "- A change.");
    let bin = env!("CARGO_BIN_EXE_rubrica");
    let out = upgrade_check(&root, &repo, &[bin, "0.2.0"], &[("GH_NO_NETWORK", "1")]);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(all.contains("upgrade-check: no previous release"), "{all}");
    assert!(!all.contains("files differ"), "{all}");
}

#[test]
fn upgrade_check_fails_on_a_render_change_without_a_changelog_line() {
    let Some(root) = repo_root() else {
        eprintln!("no checkout above the crate: upgrade-check skipped");
        return;
    };
    let bin = env!("CARGO_BIN_EXE_rubrica");
    let tmp = scratch("upgrade-diff");
    // The "previous release" builds with the real binary, then appends to the stylesheet.
    // The output directory is the third argument (`build <repo> <out> ...`).
    let wrapper = tmp.0.join("previous.sh");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\n'{bin}' \"$@\" || exit $?\nprintf x >> \"$3/_assets/rubrica.css\"\n"),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let previous = wrapper.to_str().unwrap();
    let env = [("GH_NO_NETWORK", "1"), ("UPGRADE_PREVIOUS_BIN", previous)];

    // No `Render change:` line: the check fails and says what to write.
    let repo = tmp.0.join("without");
    upgrade_repo(&repo, "- A change.");
    let out = upgrade_check(&root, &repo, &[bin, "0.2.0", "v0.1.0"], &env);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(1), "{all}");
    assert!(all.contains("rubrica.css"), "{all}");
    assert!(all.contains("Render change:"), "{all}");
    // The test mode is never silent: the skipped download is printed.
    assert!(
        all.contains("upgrade-check: skipping the download"),
        "{all}"
    );

    // With the line, the same difference passes, with a notice.
    let repo = tmp.0.join("with");
    upgrade_repo(&repo, "- Render change: the stylesheet changed.");
    let out = upgrade_check(&root, &repo, &[bin, "0.2.0", "v0.1.0"], &env);
    let all = text(&out);
    assert_eq!(out.status.code(), Some(0), "{all}");
    assert!(
        all.contains("upgrade-check: render change declared"),
        "{all}"
    );

    // The previous binary alone, without the test mode, is refused: CI never skips the download.
    let out = upgrade_check(
        &root,
        &repo,
        &[bin, "0.2.0", "v0.1.0"],
        &[("UPGRADE_PREVIOUS_BIN", previous)],
    );
    let all = text(&out);
    assert_eq!(out.status.code(), Some(2), "{all}");
    assert!(all.contains("GH_NO_NETWORK"), "{all}");
}
