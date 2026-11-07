//! Every vendored file states its terms and its origin; UPSTREAM names the version.

use std::path::Path;

/// The nine vendored files. parse.rs first: with no vendored tree the failure names it.
const VENDORED: [&str; 9] = [
    "parse.rs",
    "firstpass.rs",
    "scanners.rs",
    "strings.rs",
    "linklabel.rs",
    "entities.rs",
    "tree.rs",
    "puncttable.rs",
    "lib.rs",
];

fn read(rel: &Path) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e} ({})", rel.display()))
}

#[test]
fn vendored_files_keep_their_upstream_headers() {
    for name in VENDORED {
        let text = read(&Path::new("src").join(name));
        let head: Vec<&str> = text.lines().take(20).collect();
        let head = head.join("\n");
        assert!(
            head.contains("Permission is hereby granted"),
            "{name}: the first 20 lines hold no MIT permission notice"
        );
        assert!(
            head.contains("Vendored by Rubrica from pulldown-cmark 0.13.6"),
            "{name}: the first 20 lines hold no provenance line"
        );
    }
}

#[test]
fn upstream_file_names_the_version() {
    let text = read(Path::new("UPSTREAM"));
    assert_eq!(text.lines().next(), Some("pulldown-cmark 0.13.6"));
}
