//! Smoke test for the binary itself.

use std::process::Command;

#[test]
fn version_prints_the_crate_name_and_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .arg("--version")
        .output()
        .expect("run rubrica --version");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "rubrica --version exited with {}; stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout.trim(),
        format!("rubrica {}", env!("CARGO_PKG_VERSION")),
        "unexpected --version output"
    );
}

fn help_of(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_rubrica"))
        .args(args)
        .output()
        .expect("run rubrica --help");
    let mut help = String::from_utf8_lossy(&out.stdout).into_owned();
    help.push_str(&String::from_utf8_lossy(&out.stderr));
    help.to_lowercase()
}

#[test]
fn the_binary_help_names_no_site() {
    let help = help_of(&["--help"]);
    assert!(help.contains("a wiki in a git repository"));
    assert!(!help.contains("wiki."));
    let help = help_of(&["build", "--help"]);
    assert!(!help.contains("wiki."));
}

mod same_options {
    //! `check` and `build` render a page through the same options.

    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    fn git(dir: &Path, args: &[&str]) {
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

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn the_check_and_build_commands_render_with_the_same_options() {
        let base = scratch("same-options");
        let repo = base.join("repo");
        let cwd = base.join("cwd");
        let out_dir = base.join("out");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(
            repo.join("home.md"),
            format!(
                "---\ntitle: Home\n---\n```rust\n// {}\n```\n",
                "x".repeat(1998)
            ),
        )
        .unwrap();
        git(&repo, &["add", "-A", "."]);
        git(&repo, &["commit", "-q", "--no-verify", "-m", "one"]);

        let r = repo.to_str().unwrap();
        let checked = rubrica(&["check", "--format", "json", r], &cwd);
        let stdout = String::from_utf8_lossy(&checked.stdout).into_owned();
        assert!(stdout.contains("code-block-not-highlighted"), "{stdout}");
        assert_eq!(checked.status.code(), Some(0), "{stdout}");
        let json: serde_json::Value = serde_json::from_str(&stdout).expect("the JSON report");
        assert_eq!(json["warnings"], 1, "{stdout}");
        let findings = json["findings"].as_array().expect("findings");
        assert_eq!(findings.len(), 1, "{stdout}");
        assert_eq!(findings[0]["kind"], "code-block-not-highlighted");

        let built = rubrica(&["build", r, out_dir.to_str().unwrap()], &cwd);
        let stderr = String::from_utf8_lossy(&built.stderr).into_owned();
        assert_eq!(built.status.code(), Some(0), "{stderr}");
        let line = stderr
            .lines()
            .find(|l| {
                l.starts_with("rubrica build: ")
                    && l.contains("finding(s) that do not stop a build")
            })
            .unwrap_or_else(|| panic!("no finding line in build's stderr: {stderr}"));
        let n: usize = line
            .trim_start_matches("rubrica build: ")
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .expect("N");
        assert_eq!(n, 1, "{line}");
        assert_eq!(n as u64, json["warnings"].as_u64().unwrap());
        let _ = std::fs::remove_dir_all(&base);
    }
}
