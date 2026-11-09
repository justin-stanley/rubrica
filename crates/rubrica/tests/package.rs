//! What crates.io would receive. For each published crate, in publish order: the committed
//! `PACKAGE-FILES` exists, `cargo package` succeeds (so every path dependency carries a
//! version), and the packaged file list is exactly the committed one. A change to what a
//! crate ships is a diff to review in `PACKAGE-FILES`, never a surprise at release.
//!
//! `rubrica-site` embeds `rubrica-wasm-bundle`, whose files exist only after
//! `scripts/build-wasm.sh --bundle`; the bundle crate's own list is checked by the gate
//! (scripts/ci.sh), through `scripts/package-check.sh`.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace root: this crate's directory is `<root>/crates/rubrica`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate is two levels below the workspace root")
        .to_path_buf()
}

/// The workspace crates another published crate depends on. Before they are on crates.io,
/// `cargo package` cannot find them in the registry index, so each is patched to its path
/// (the packaged manifest is unchanged: it still says `version = "0.1"`), and `--offline`
/// keeps the test off the network.
const SIBLINGS: [&str; 5] = [
    "rubrica-markdown",
    "rubrica-render",
    "rubrica-diagram",
    "rubrica-wasm-bundle",
    "rubrica-site",
];

/// `cargo package FLAGS -p KRATE` in the workspace root.
fn package(flags: &[&str], krate: &str) -> std::process::Output {
    let root = workspace_root();
    let mut c = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    c.current_dir(&root).arg("package").args(flags);
    c.args(["--allow-dirty", "--locked", "--offline"]);
    for s in SIBLINGS.into_iter().filter(|s| *s != krate) {
        let path = root.join("crates").join(s).to_string_lossy().into_owned();
        // A TOML basic string.
        let quoted = format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""));
        c.arg("--config")
            .arg(format!("patch.crates-io.{s}.path={quoted}"));
    }
    c.args(["-p", krate]);
    c.output().expect("run cargo package")
}

#[test]
fn every_crate_packages_and_its_file_list_is_the_committed_one() {
    // Run from an unpacked .crate (a downstream packager's `cargo test`), the workspace is
    // not two levels up: there is nothing to compare against.
    if !workspace_root().join("crates/rubrica/Cargo.toml").is_file() {
        eprintln!("package: not in the rubrica workspace; skipped");
        return;
    }
    for krate in [
        "rubrica-markdown",
        "rubrica-render",
        "rubrica-diagram",
        "rubrica-site",
        "rubrica",
    ] {
        let committed_path = workspace_root()
            .join("crates")
            .join(krate)
            .join("PACKAGE-FILES");
        let committed = std::fs::read_to_string(&committed_path)
            .unwrap_or_else(|e| panic!("{}: {e}", committed_path.display()));

        let out = package(&["--no-verify"], krate);
        assert!(
            out.status.success(),
            "cargo package -p {krate} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let out = package(&["--list"], krate);
        assert!(
            out.status.success(),
            "cargo package --list -p {krate} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut listed: Vec<&str> = std::str::from_utf8(&out.stdout)
            .expect("the list is UTF-8")
            .lines()
            .collect();
        listed.sort_unstable();
        let mut want: Vec<&str> = committed.lines().collect();
        want.sort_unstable();
        assert_eq!(
            listed, want,
            "{krate}: `cargo package --list` is not the committed PACKAGE-FILES"
        );
    }
}
