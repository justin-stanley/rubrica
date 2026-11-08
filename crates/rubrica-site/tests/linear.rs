//! The corpus model is linear (or `n log n`) in pages, bytes and links:
//! operation counts on the link work, which a slow machine cannot blur, and
//! times on shapes that would be quadratic, each well under its limit.

use std::time::{Duration, Instant};

use rubrica_site::links::{ops, ops_reset};
use rubrica_site::{
    Finding, PagePath, Site, SiteConfig, SourceFile, MAX_PATH_BYTES, MAX_PATH_SEGMENTS,
    SUGGESTION_BUDGET_BASE, SUGGESTION_BUDGET_PER_BYTE,
};

fn md(path: &str, body: &str) -> SourceFile {
    SourceFile::regular(
        path.to_owned(),
        format!("---\ntitle: {path}\n---\n{body}").into_bytes(),
    )
}

fn total_bytes(files: &[SourceFile]) -> u64 {
    files
        .iter()
        .map(|f| f.bytes.len() as u64 + f.path.len() as u64)
        .sum()
}

fn timed<T>(limit: Duration, what: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    let took = t.elapsed();
    eprintln!("linear: {what}: {took:?} (limit {limit:?})");
    assert!(
        took < limit,
        "{what}: {took:?}, over {limit:?}: superlinear?"
    );
    out
}

/// `n` headings on one page that all share the word `x`, and `n` links to
/// fragments that match none of them but share `x`: check-anchors' word
/// search visits every heading for every link, `n²` postings.
fn shared_word_corpus(n: usize) -> Vec<SourceFile> {
    let mut target = String::new();
    for i in 0..n {
        target.push_str(&format!("## x h{i}\n"));
    }
    let mut links = String::new();
    for i in 0..n {
        links.push_str(&format!("[l](/target#x-q{i}-r{i}-s{i})\n"));
    }
    vec![md("target.md", &target), md("from.md", &links)]
}

#[test]
fn the_word_search_is_bounded_by_its_budget_and_says_so() {
    let n = 6_000;
    let files = shared_word_corpus(n);
    let bytes = total_bytes(&files);
    let budget = SUGGESTION_BUDGET_PER_BYTE * bytes + SUGGESTION_BUDGET_BASE;
    ops_reset();
    let site = timed(
        Duration::from_secs(8),
        "6,000 links × 6,000 headings sharing a word",
        || Site::from_sources(files, &SiteConfig::default()),
    );
    let spent = ops();
    // Every unit of link work counts; the word search can spend at most the
    // budget, everything else is linear in the bytes (a small constant).
    assert!(
        spent <= budget + 64 * bytes,
        "{spent} ops for {bytes} bytes (budget {budget})"
    );
    // n² would be 36,000,000 postings.
    assert!(spent < (n * n) as u64 / 2, "{spent}");
    let bounded: Vec<&Finding> = site
        .findings()
        .iter()
        .filter(|f| matches!(f, Finding::SuggestionsBounded { .. }))
        .collect();
    assert_eq!(bounded.len(), 1, "reported once");
    let anchors = site
        .findings()
        .iter()
        .filter(|f| matches!(f, Finding::BrokenAnchor { .. }))
        .count();
    assert_eq!(anchors, n, "every broken anchor is still reported");
}

#[test]
fn under_the_budget_the_word_search_is_exact() {
    let site = Site::from_sources(shared_word_corpus(200), &SiteConfig::default());
    assert!(!site
        .findings()
        .iter()
        .any(|f| matches!(f, Finding::SuggestionsBounded { .. })));
    // `x-q0-r0-s0` shares one word of four with each heading: 25%, under
    // check-anchors' 60%, so no suggestion.
    assert!(site
        .findings()
        .iter()
        .all(|f| matches!(f, Finding::BrokenAnchor { .. }) && f.fix().is_none()));
}

#[test]
fn link_work_is_linear_in_the_link_bytes() {
    // Long hrefs, deep dot segments, many fragments, many repeats of one
    // broken fragment (memoised), and prefix searches through a deep trie.
    let mut target = String::new();
    for i in 0..2_000 {
        target.push_str(&format!("## {} {i}\n", "word ".repeat(20)));
    }
    let mut links = String::new();
    let long = format!("/{}target", "a/../".repeat(100));
    for i in 0..2_000 {
        links.push_str(&format!(
            "[l]({long}) [m](/target#{}{i}) [n](/target#same)\n",
            "word-".repeat(40)
        ));
    }
    let files = vec![md("target.md", &target), md("from.md", &links)];
    let bytes = total_bytes(&files);
    ops_reset();
    let site = timed(Duration::from_secs(8), "6,000 long links", || {
        Site::from_sources(files, &SiteConfig::default())
    });
    let spent = ops();
    eprintln!("linear: link work {spent} ops for {bytes} bytes");
    assert!(spent <= 8 * bytes, "{spent} ops for {bytes} bytes");
    let from = site.page(&PagePath::new("from").unwrap()).unwrap();
    assert_eq!(from.links.len(), 6_000);
}

/// A site of `n` pages in a tree of directories, each linking to its parent
/// section and the home page, with an `@sec` marker each.
fn tree_corpus(n: usize) -> Vec<SourceFile> {
    let mut files = vec![md("home.md", "# Home\n")];
    for i in 0..n {
        // Up to 6 levels deep, 8 wide.
        let mut dir = String::new();
        let mut k = i;
        for _ in 0..(i % 6) {
            dir.push_str(&format!("dir{}/", k % 8));
            k /= 8;
        }
        let path = format!("{dir}page{i}");
        let parent = dir.trim_end_matches('/');
        let up = if parent.is_empty() {
            "/".to_owned()
        } else {
            format!("/{parent}#nope")
        };
        files.push(md(
            &format!("{path}.md"),
            &format!("<!-- @sec id=s/{i} -->\n# Page {i}\n## Part\n[up]({up}) [home](/) [me](/{path}#part)\n"),
        ));
    }
    files
}

#[test]
fn a_site_of_many_pages_builds_in_work_linear_in_its_pages() {
    // Operation counts (link work, and every directory, nav entry and
    // breadcrumb made), which a busy machine cannot blur; the time is a
    // backstop only.
    let small = tree_corpus(4_000);
    let big = tree_corpus(16_000);
    let (small_bytes, big_bytes) = (total_bytes(&small), total_bytes(&big));
    ops_reset();
    let s = Site::from_sources(small, &SiteConfig::default());
    let ops_small = ops();
    ops_reset();
    let t = Instant::now();
    let b = Site::from_sources(big, &SiteConfig::default());
    let t_big = t.elapsed();
    let ops_big = ops();
    eprintln!(
        "linear: 4,000 pages {ops_small} ops, 16,000 pages {ops_big} ops ({:.2}×), {t_big:?}",
        ops_big as f64 / ops_small as f64
    );
    assert_eq!(s.pages().len(), 4_001);
    assert_eq!(b.pages().len(), 16_001);
    assert_eq!(b.nav().pages().len(), 16_001);
    // 4× the pages (and about 4× the bytes): linear is 4×, quadratic 16×.
    let bytes_ratio = big_bytes as f64 / small_bytes as f64;
    assert!(
        (ops_big as f64) < (ops_small as f64) * bytes_ratio * 1.15,
        "4,000 pages {ops_small} ops, 16,000 pages {ops_big} ops"
    );
    assert!(
        ops_big <= 8 * big_bytes,
        "{ops_big} ops for {big_bytes} bytes"
    );
    assert!(t_big < Duration::from_secs(20), "{t_big:?}");
}

#[test]
fn deep_paths_are_bounded_and_flat_directories_are_fine() {
    // A path at the segment bound, and one past it.
    let ok = vec!["d"; 63].join("/") + "/page.md";
    let deep = vec!["d"; 64].join("/") + "/page.md";
    let ok_deep = format!("ddd{}", &ok[1..]);
    let s = Site::from_sources(
        vec![md(&ok_deep, ""), md(&format!("ddd{}", &deep[1..]), "")],
        &SiteConfig::default(),
    );
    assert_eq!(s.pages().len(), 1);
    assert_eq!(
        s.page(s.pages().keys().next().unwrap())
            .unwrap()
            .breadcrumbs
            .len(),
        64
    );
    assert!(matches!(
        s.findings(),
        [Finding::PathTooLong { segments: 65, .. }]
    ));
    // 20,000 pages in one directory.
    let mut files = Vec::new();
    for i in 0..20_000 {
        files.push(md(&format!("flat/p{i}.md"), ""));
    }
    let s = timed(
        Duration::from_secs(10),
        "20,000 pages in one directory",
        || Site::from_sources(files, &SiteConfig::default()),
    );
    assert_eq!(s.nav().pages().len(), 20_000);
}

#[test]
fn a_relative_link_costs_its_pages_path_and_the_count_says_so() {
    // `[x](y)` at depth 62: each one is resolved against the page's path
    // (about 1,000 bytes), so the work is the links times that path, which
    // the operation count must show (the crate's docs list the factor).
    let mut dir = "ddd".to_owned();
    for _ in 0..61 {
        dir.push_str("/sssssssssssssss");
    }
    let n = 40_000u64;
    let files = vec![md(&format!("{dir}/p.md"), &"[x](y)\n".repeat(n as usize))];
    let bytes = total_bytes(&files);
    let path = dir.len() as u64 + 2;
    ops_reset();
    let site = timed(
        Duration::from_secs(4),
        "40,000 relative links 62 deep",
        || Site::from_sources(files, &SiteConfig::default()),
    );
    let spent = ops();
    eprintln!("linear: relative link work {spent} ops for {bytes} bytes, path {path}");
    assert!(spent >= n * path, "{spent} ops: the base is not counted");
    assert!(
        spent <= n * (path + 1) + 8 * bytes,
        "{spent} ops for {bytes} bytes"
    );
    assert_eq!(site.pages().len(), 1);
}

#[test]
fn a_pages_relative_link_base_is_built_once_per_page() {
    // A 4 MiB `[](x)` page at a 1,024-byte, 64-segment path
    // took 3.1 s, against 1.1 s at a short path, because each link split
    // the page's directory again. The base is now built once per page (the
    // join of each link's path with it still costs the path, which the
    // operation count above shows).
    let mut dir = "ddd".to_owned();
    for _ in 0..61 {
        dir.push_str("/sssssssssssssss");
    }
    let files = vec![
        md(&format!("{dir}/p.md"), &"[x](y)\n".repeat(40_000)),
        md(&format!("{dir}/q.md"), "[x](y)\n[z](../w)\n"),
    ];
    rubrica_site::links::relative_bases_reset();
    let site = Site::from_sources(files, &SiteConfig::default());
    assert_eq!(site.pages().len(), 2);
    assert_eq!(
        rubrica_site::links::relative_bases(),
        2,
        "one base per page"
    );
}

/// `a/a/…/` (`aa/a/…/` for an odd length) then `tail`, `bytes` long.
fn deep_path(bytes: usize, tail: &str) -> String {
    let n = bytes - tail.len();
    let mut s = "a".repeat(n % 2) + &"a/".repeat(n / 2);
    s.push_str(tail);
    assert_eq!(s.len(), bytes);
    s
}

#[test]
fn a_path_over_the_limits_is_refused_before_any_other_work() {
    // Shapes from review: the directory fold hashed every prefix
    // of every repository path, ignored files and assets included, so one
    // path cost its length times its depth (a 1 MiB path, 91 s). A path
    // over either limit is now refused first and takes part in nothing:
    // no fold, no role, no work the operation count sees.
    let cfg = SiteConfig::default();
    let png = |path: String| SourceFile::regular(path, "x");
    let shapes: [(&str, Vec<SourceFile>); 3] = [
        (
            "one ignored 64 KiB a/a/…/x.png",
            vec![png(deep_path(64 << 10, "x.png"))],
        ),
        (
            "one 1 MiB a/a/…/x.md",
            vec![md(&deep_path(1 << 20, "x.md"), "")],
        ),
        (
            "1,024 × 4 KiB a/a/…/xNNNN.png",
            (0..1024)
                .map(|i| png(deep_path(4096, &format!("x{i:04}.png"))))
                .collect(),
        ),
    ];
    for (what, files) in shapes {
        let mut want: Vec<(usize, usize)> = files
            .iter()
            .map(|f| (f.path.len(), f.path.split('/').count()))
            .collect();
        let shown = files[0].path[..200].to_owned() + "…";
        ops_reset();
        let site = timed(limits().ceiling, what, || Site::from_sources(files, &cfg));
        let spent = ops();
        assert!(
            spent <= 64,
            "{what}: {spent} ops: an over-limit path was worked on"
        );
        let mut got: Vec<(usize, usize)> = site
            .findings()
            .iter()
            .map(|f| match f {
                Finding::PathTooLong {
                    file,
                    bytes,
                    segments,
                } => {
                    assert_eq!(**file, *shown, "{what}: the finding shows 200 bytes");
                    (*bytes, *segments)
                }
                other => panic!("{what}: {other:?}"),
            })
            .collect();
        want.sort();
        got.sort();
        assert_eq!(got, want, "{what}");
        assert_eq!(site.folded_paths().count(), 0, "{what}");
        assert_eq!(site.folded_dirs().count(), 0, "{what}");
        assert!(site.pages().is_empty() && site.assets().is_empty());
    }
}

#[test]
fn a_path_too_long_is_shown_with_an_ellipsis_only_when_it_was_cut() {
    // `…` was appended to every PathTooLong path, also to
    // one under 200 bytes (too deep, not too long), which then read as cut.
    let deep = vec!["d"; MAX_PATH_SEGMENTS + 1].join("/");
    let long = "a".repeat(MAX_PATH_BYTES + 1);
    let s = Site::from_sources(
        vec![
            SourceFile::regular(deep.clone(), "x"),
            SourceFile::regular(long.clone(), "x"),
        ],
        &SiteConfig::default(),
    );
    let mut shown: Vec<String> = s
        .findings()
        .iter()
        .map(|f| match f {
            Finding::PathTooLong { file, .. } => file.to_string(),
            other => panic!("{other:?}"),
        })
        .collect();
    shown.sort();
    assert_eq!(shown, [long[..200].to_owned() + "…", deep]);
}

#[test]
fn the_directory_check_counts_every_hash() {
    // The bound below holds for what is counted; this pins that every hash
    // of the directory check is: per file, its fold key in `dirs`
    // and in the file map; per directory, the file map, `dirs` and the
    // registration; a directory spelled otherwise than first, once more,
    // and the record of a reported spelling. Ignored files, so no page work.
    let s = |paths: &[&str]| {
        ops_reset();
        Site::from_sources(
            paths.iter().map(|p| SourceFile::regular(*p, "x")).collect(),
            &SiteConfig::default(),
        );
        ops()
    };
    // The site's own work with no file (the nav's root).
    let base = s(&[]);
    // "a/x": 3 + 3 for the key, 2 + 1 for the directory `a`.
    assert_eq!(s(&["A/x"]) - base, 9);
    // "a/y" spells `a` otherwise: 9, the check (1) and its record (1).
    assert_eq!(s(&["A/x", "a/y"]) - base, 9 + 11);
    // "a/z": that spelling again, checked (1), passed over, not recorded.
    assert_eq!(s(&["A/x", "a/y", "a/z"]) - base, 9 + 11 + 10);
    // A `.md` file adds its path once (the `.md` set).
    assert_eq!(s(&["A/x.md"]) - s(&["A/x.mx"]), 6);
}

#[test]
fn a_files_directory_work_is_at_most_the_documented_bound() {
    // The crate docs' bound: at most 10 × MAX_PATH_BYTES × MAX_PATH_SEGMENTS
    // bytes hashed per file (9 × before the count included the own fold
    // key's second hash and the second-spelling checks too). The tightest shape: one long
    // first directory whose every character folds to three times its bytes
    // (`ΐ`, 2 bytes, is 6 folded), so every one of the 63 directory prefixes
    // is long, then 62 one-letter directories and a one-letter file: both
    // limits used, and the work near the bound.
    let first = "ΐ".repeat((MAX_PATH_BYTES - 2 * (MAX_PATH_SEGMENTS - 2) - 2) / 2);
    let dirs = format!("{first}/{}", "a/".repeat(MAX_PATH_SEGMENTS - 2));
    let mut path = format!("{dirs}x");
    while path.len() < MAX_PATH_BYTES {
        path.insert(dirs.len(), 'x');
    }
    assert_eq!(
        (path.len(), path.split('/').count()),
        (MAX_PATH_BYTES, MAX_PATH_SEGMENTS)
    );
    let bound = 10 * (MAX_PATH_BYTES * MAX_PATH_SEGMENTS) as u64;
    // Per file: alone, then with a second file beside it (whose prefixes the
    // first one's are, so the conflict check walks it in full too).
    for files in [
        vec![SourceFile::regular(path.clone(), "x")],
        vec![
            SourceFile::regular(path.clone(), "x"),
            SourceFile::regular(format!("{dirs}b"), "x"),
        ],
    ] {
        let n = files.len() as u64;
        ops_reset();
        let site = Site::from_sources(files, &SiteConfig::default());
        let spent = ops();
        eprintln!(
            "linear: directory work at the limits, {n} file(s): {spent} ops, {:.1}% of {n} × the bound {bound}",
            100.0 * spent as f64 / (n * bound) as f64
        );
        assert!(spent <= n * bound, "{spent} ops, over {n} × {bound}");
        // The shape is the tight one: each file's work is near the bound
        // (89% of the 9 × the review measured against; the tenth is the
        // second-spelling checks, which this shape has none of).
        assert!(
            spent * 100 >= 78 * n * bound,
            "{spent} ops: under 78% of {n} × {bound}, so not the bound's shape (or the fold is not counted)"
        );
        assert_eq!(site.folded_paths().count() as u64, n);
        assert!(site.findings().is_empty(), "{:?}", site.findings());
    }
}

/// `fences` fenced blocks of `lang`, each `lines` lines of `line`.
fn fence_document(lang: &str, line: &str, lines: usize, fences: usize) -> String {
    let block = format!("```{lang}\n{}```\n\n", format!("{line}\n").repeat(lines));
    block.repeat(fences)
}

/// How strictly the linear-time rule is applied. Locally (and in the private gate) a
/// doubling may cost at most 2.5x, timed as the best of three runs. The hosted public CI
/// runners are shared and noisy, so there (`RUBRICA_LINEAR_CI=1`) the best of five runs may
/// cost up to 3.0x per doubling. Quadratic growth shows about 4x either way. The absolute
/// ceiling is a budget for known hardware: 2 s locally, 6 s on hosted CI, whose arm64 runner
/// is about 3.7x slower than the development machine.
struct Limits {
    ratio: f64,
    repeats: usize,
    ceiling: Duration,
}

/// The release-build limits with the hosted-CI switch `ci` on or off.
fn limits_for(ci: bool) -> Limits {
    if ci {
        Limits {
            ratio: 3.0,
            repeats: 5,
            ceiling: Duration::from_secs(6),
        }
    } else {
        Limits {
            ratio: 2.5,
            repeats: 3,
            ceiling: Duration::from_secs(2),
        }
    }
}

/// Whether `RUBRICA_LINEAR_CI=1` selects the hosted-CI limits.
fn hosted_ci() -> bool {
    std::env::var("RUBRICA_LINEAR_CI").is_ok_and(|v| v == "1")
}

/// The limits in force; debug builds, whose timing asserts are compiled out, run each
/// timing once.
fn limits() -> Limits {
    let ci = hosted_ci();
    Limits {
        repeats: if cfg!(debug_assertions) {
            1
        } else {
            limits_for(ci).repeats
        },
        ..limits_for(ci)
    }
}

#[test]
fn the_limits_follow_the_hosted_ci_switch() {
    let local = limits_for(false);
    assert_eq!((local.ratio, local.repeats), (2.5, 3));
    assert_eq!(local.ceiling, Duration::from_secs(2));
    let hosted = limits_for(true);
    assert_eq!((hosted.ratio, hosted.repeats), (3.0, 5));
    assert_eq!(hosted.ceiling, Duration::from_secs(6));
}

/// One row of the highlighting matrix: the document at each of `sizes` (fence counts)
/// rendered through the site's own options, `shape` checked on every output, and, in a
/// release build, the time per size against the ceiling and the ratio between neighbours
/// against the floor-and-ratio rule (at most 2.5 per doubling, 3.0 on hosted CI, 50 ms floor, 2 s ceiling; 6 s on hosted CI).
fn highlight_row(
    name: &str,
    lang: &str,
    line: &str,
    lines: usize,
    sizes: [usize; 3],
    shape: impl Fn(&rubrica_render::Rendered, usize),
) {
    let opts = rubrica_site::render_options(rubrica_render::Profile::Native);
    // Debug builds run each row's smallest size only: the shapes, no timing.
    let sizes: &[usize] = if cfg!(debug_assertions) {
        &sizes[..1]
    } else {
        &sizes
    };
    let mut previous: Option<Duration> = None;
    for &fences in sizes {
        let body = fence_document(lang, line, lines, fences);
        // Locally one timing; on hosted CI the best of five (the switch is `limits()`).
        let runs = if hosted_ci() { limits().repeats } else { 1 };
        let mut best: Option<(rubrica_render::Rendered, Duration)> = None;
        for _ in 0..runs {
            let t = Instant::now();
            let out = rubrica_render::render(&body, &opts);
            let took = t.elapsed();
            if best.as_ref().is_none_or(|(_, b)| took < *b) {
                best = Some((out, took));
            }
        }
        let (rendered, took) = best.unwrap();
        shape(&rendered, fences);
        eprintln!(
            "linear: highlighting row {name}: {fences} fences, {} bytes: {took:?}",
            body.len()
        );
        #[cfg(not(debug_assertions))]
        {
            assert!(
                took < limits().ceiling,
                "row {name}, {fences} fences: {took:?}, over the {:?} ceiling",
                limits().ceiling
            );
            if let Some(before) = previous {
                let floor = Duration::from_millis(50);
                let ratio = took.as_secs_f64() / before.max(floor).as_secs_f64();
                eprintln!(
                    "linear: highlighting row {name}: ratio {ratio:.2} ({before:?} -> {took:?})"
                );
                assert!(
                    ratio <= limits().ratio,
                    "row {name}, {fences} fences: ratio {ratio:.2} ({before:?} -> {took:?}): superlinear?"
                );
            }
        }
        previous = Some(took);
    }
    let _ = previous;
}

fn not_highlighted(rendered: &rubrica_render::Rendered) -> usize {
    rendered
        .findings
        .iter()
        .filter(|f| {
            matches!(
                f,
                rubrica_render::RenderFinding::CodeBlockNotHighlighted { .. }
            )
        })
        .count()
}

#[test]
fn highlighting_time_is_linear() {
    // Row A: json under both caps, highlighted (60,040 bytes a block).
    highlight_row(
        "A",
        "json",
        &"1234567890, ".repeat(125),
        40,
        [9, 17, 35],
        |r, _| {
            assert!(r.html.contains("<span class=\"hl-constant\">"));
            assert_eq!(not_highlighted(r), 0);
        },
    );
    // Row B: js, the pathological shape under both caps, highlighted. Half the size of
    // the other rows: at 2 MB this shape sat near the 2 s ceiling.
    highlight_row("B", "js", &"(".repeat(1999), 1, [125, 250, 500], |r, _| {
        assert!(r.html.contains("<span class=\"hl-"));
        assert_eq!(not_highlighted(r), 0);
    });
    // Row C: js over the line cap and under the block cap, written plain, one finding each.
    highlight_row("C", "js", &"(".repeat(60_000), 1, [9, 17, 35], |r, n| {
        assert!(!r.html.contains("<span class=\"hl-"));
        assert_eq!(not_highlighted(r), n);
    });
}
