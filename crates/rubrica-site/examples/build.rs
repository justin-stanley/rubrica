//! Builds a small site from three in-memory pages into a temporary directory and lists
//! the files written.
//!
//! The library never touches the file system: the caller hands it the source files and a
//! `write` callback that receives each output file. Run it with
//! `cargo run -p rubrica-site --example build`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use rubrica_site::build::{self, BuildConfig, Commit, History};
use rubrica_site::{Site, SiteConfig, SourceFile};

/// A page: YAML front matter with a title, then the Markdown body.
fn page(path: &str, title: &str, body: &str) -> SourceFile {
    SourceFile::regular(path, format!("---\ntitle: {title}\n---\n{body}"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The files of a wiki repository, as a caller would read them from a git tree.
    let files = vec![
        page(
            "home.md",
            "Home",
            "# Home\n\nStart at the [notes](/notes).\n",
        ),
        page(
            "notes.md",
            "Notes",
            "# Notes\n\nSee [the owls](/notes/owls#habitat).\n",
        ),
        page(
            "notes/owls.md",
            "Owls",
            "# Owls\n\n## Habitat\n\nForests and fields.\n",
        ),
    ];
    let sources: BTreeMap<String, Vec<u8>> = files
        .iter()
        .map(|f| (f.path.clone(), f.bytes.clone()))
        .collect();

    // The corpus: URLs, navigation, the link graph and every finding.
    let site = Site::from_sources(files, &SiteConfig::default());
    for finding in build::refusals(&site) {
        eprintln!("refused: {finding:?}");
    }

    // What the pages show about git, supplied by the caller (here, one commit).
    let history = History {
        commit: Commit {
            id: "a".repeat(40),
            time: 1_700_000_000,
            offset: 0,
            summary: "Add the notes".into(),
        },
        last: BTreeMap::new(),
        recent: Vec::new(),
        feed_versions: Vec::new(),
        complete: true,
    };

    let out: PathBuf =
        std::env::temp_dir().join(format!("rubrica-site-example-{}", std::process::id()));
    let mut written = Vec::new();
    let built = build::build(
        &site,
        &BuildConfig::default(),
        &history,
        // Each page's source, published beside it as `<path>.md`.
        &mut |page| Ok(sources[&*page.file].clone()),
        // Each output file, once: a path relative to the site root, and its bytes.
        &mut |path, bytes| {
            let target = out.join(path);
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir)?;
            }
            fs::write(&target, bytes)?;
            written.push((path.to_owned(), bytes.len()));
            Ok(())
        },
    )
    .map_err(|e| e.to_string())?;

    println!(
        "{} pages, {} files, {} bytes in {}",
        built.pages,
        built.files,
        built.bytes,
        out.display()
    );
    written.sort();
    for (path, len) in &written {
        println!("{len:>8}  {path}");
    }
    fs::remove_dir_all(&out)?;
    Ok(())
}
