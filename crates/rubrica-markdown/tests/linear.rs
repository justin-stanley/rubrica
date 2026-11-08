//! The parser with the autolink scanner on 37 pathological shapes at 0.5, 1 and 2 MB
//! (the linear-time rule): time grows by at most 2.5× per doubling (a doubling whose smaller run is
//! under 50 ms is timer noise and not compared; a shape under the floor at 2 MB is re-run
//! at 4 and 8 MB so it has one real ratio, with a 100 ms floor there), and 2 MB takes under
//! 2 s; each timing is the best of three. The timing asserts
//! are compiled out of debug builds, where the shapes still run for panics; CI's `linear`
//! job runs `--release`. The shapes are parser-bench's, scaled to the size, less the two
//! private ones, plus four autolink shapes.

use std::time::{Duration, Instant};

use rubrica_markdown::{events, GFM};

const MB: usize = 1 << 20;
const FLOOR: Duration = Duration::from_millis(50);

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

/// The best of `limits().repeats` timings of `f`.
fn best_of<T>(mut f: impl FnMut() -> T) -> (T, Duration) {
    let mut best: Option<(T, Duration)> = None;
    for _ in 0..limits().repeats {
        let start = Instant::now();
        let out = f();
        let took = start.elapsed();
        if best.as_ref().is_none_or(|(_, t)| took < *t) {
            best = Some((out, took));
        }
    }
    best.unwrap()
}

/// The floor under which a doubling's smaller run is timer noise: 50 ms, and 100 ms for
/// the 4 and 8 MB re-runs.
#[cfg(not(debug_assertions))]
fn floor_at(n: usize) -> Duration {
    if n >= 2 * MB {
        Duration::from_millis(100)
    } else {
        FLOOR
    }
}

/// The shape `name` at about `size` bytes.
fn case(name: &str, size: usize) -> String {
    match name {
        // A long line of `:` with no `//`: cheap trigger check.
        "colons" => ":".repeat(size),
        // cmark's pathological_tests.py shapes.
        "nested-brackets" => format!("{}a{}", "[".repeat(size / 2), "]".repeat(size / 2)),
        "nested-strong-emph" => format!(
            "{}a{}",
            "*a **a ".repeat(size / 14),
            " a** a*".repeat(size / 14)
        ),
        "many-emph-closers" => "a_ ".repeat(size / 3),
        "many-emph-openers" => " _a".repeat(size / 3),
        "many-link-closers" => "a]".repeat(size / 2),
        "many-link-openers" => "[a".repeat(size / 2),
        "mismatched-openers-closers" => "*a_ ".repeat(size / 4),
        "link-openers-and-emph-closers" => "[ a_".repeat(size / 4),
        "pattern-brackets-emph" => "[a_".repeat(size / 3),
        "hard-link-emph" => "*a [a**".repeat(size / 7),
        "nested-blockquotes" => format!("{}a", "> ".repeat(size / 2)),
        "nested-lists" => format!("{}a", "- ".repeat(size / 2)),
        // `e` + n backticks + space for n up to k: about k²/2 bytes.
        "backticks" => {
            let k = ((2 * size) as f64).sqrt() as usize;
            (1..k).map(|n| format!("e{} ", "`".repeat(n))).collect()
        }
        "unclosed-links-a" => "[a](<b".repeat(size / 6),
        "unclosed-links-b" => "[a](b".repeat(size / 5),
        "unclosed-inline-html" => "<a href=\"".repeat(size / 9),
        "reference-collisions" => format!(
            "{}{}",
            "[a]: /url\n".repeat(size / 10),
            "[a][a]".repeat(size / 6)
        ),
        "many-references" => {
            (0..size / 12)
                .map(|i| format!("[{i}]: /u\n"))
                .collect::<String>()
                + "[0]"
        }
        "entities" => "&".repeat(size),
        "deeply-nested-emph" => format!("{}a{}", "*".repeat(size / 2), "*".repeat(size / 2)),
        "html-comments" => "<!--".repeat(size / 4),
        "code-spans" => "`a".repeat(size / 2),
        "tildes" => "~a".repeat(size / 2),
        "footnotes" => format!(
            "{}{}",
            (0..size / 16)
                .map(|i| format!("[^{i}]: n\n"))
                .collect::<String>(),
            (0..size / 16)
                .map(|i| format!("[^{i}]"))
                .collect::<String>()
        ),
        // GFM tables: cell count (markdown-rs issue 218), one row-pair of about size/6 cells.
        "table-wide" => {
            let n = size / 6;
            format!(
                "|{}\n|{}\n|{}\n",
                "a|".repeat(n),
                "-|".repeat(n),
                "b|".repeat(n)
            )
        }
        "table-tall" => format!("|a|b|\n|-|-|\n{}", "|c|d|\n".repeat(size / 6)),
        "table-escaped-pipes" => format!("|a|\n|-|\n|{}|\n", "\\|".repeat(size / 2)),
        "setext-underlines" => "a\n=\n".repeat(size / 4),
        "atx-empty" => "#\n".repeat(size / 2),
        "task-items" => "- [ ] a\n".repeat(size / 8),
        "plain-text" => "x".repeat(size),
        "paragraph-lines" => "a\n".repeat(size / 2),
        // Rubrica's autolink shapes.
        "at-signs" => "@".repeat(size),
        "emails" => "a@b.c ".repeat(size / 6),
        "www-fail" => "www.a ".repeat(size / 6),
        "parens" => "(www.a.com))))".repeat(size / 14),
        _ => panic!("unknown case {name}"),
    }
}

/// The 37 public shapes.
const CASES: [&str; 37] = [
    "colons",
    "nested-brackets",
    "nested-strong-emph",
    "many-emph-closers",
    "many-emph-openers",
    "many-link-closers",
    "many-link-openers",
    "mismatched-openers-closers",
    "link-openers-and-emph-closers",
    "pattern-brackets-emph",
    "hard-link-emph",
    "nested-blockquotes",
    "nested-lists",
    "backticks",
    "unclosed-links-a",
    "unclosed-links-b",
    "unclosed-inline-html",
    "reference-collisions",
    "many-references",
    "entities",
    "deeply-nested-emph",
    "html-comments",
    "code-spans",
    "tildes",
    "footnotes",
    "table-wide",
    "table-tall",
    "table-escaped-pipes",
    "setext-underlines",
    "atx-empty",
    "task-items",
    "plain-text",
    "paragraph-lines",
    "at-signs",
    "emails",
    "www-fail",
    "parens",
];

/// The linear-time rule on a shape's `(bytes, time)` series: per doubling `t(2x) <= 2.5 t(x)` (3.0 on hosted CI)
/// unless `t(x)` is under the floor (50 ms; 100 ms for the 4 and 8 MB re-runs); the 2 MB
/// run under the ceiling (2 s; 6 s on hosted CI).
#[cfg(not(debug_assertions))]
fn assert_linear(name: &str, series: &[(usize, Duration)]) {
    for w in series.windows(2) {
        let (n1, t1) = w[0];
        let (n2, t2) = w[1];
        if t1 >= floor_at(n1) {
            let ratio = t2.as_secs_f64() / t1.as_secs_f64();
            assert!(
                t2 <= t1.mul_f64(limits().ratio),
                "assertion failed: t2 <= {} * t1 (shape {name} {}→{} MB: ratio {ratio:.1})",
                limits().ratio,
                n1 as f64 / MB as f64,
                n2 as f64 / MB as f64
            );
        }
    }
    for &(n, t) in series {
        if n == 2 * MB {
            assert!(
                t <= limits().ceiling,
                "shape {name} at 2 MB took {:.3} s",
                t.as_secs_f64()
            );
        }
    }
}

/// `RUBRICA_LINEAR_SHARD=i/N` (1-based) runs only the shapes at index `k` of `CASES` with
/// `k % N == i - 1`; unset runs every shape. CI's `linear-arm64` job runs four shards. A debug
/// build runs each shape once, at 0.5 MB, with no timing assert.
#[test]
fn every_shape_is_linear_with_autolinks_on() {
    let shard = shard_from_env();
    let shapes = shard_shapes(shard);
    assert!(
        !shapes.is_empty(),
        "RUBRICA_LINEAR_SHARD {shard:?} selects none of the {} shapes",
        CASES.len()
    );
    eprintln!(
        "shard {shard:?}: {} of {} shapes",
        shapes.len(),
        CASES.len()
    );
    for name in shapes {
        let mut series = Vec::new();
        let mut sizes = sizes_for(cfg!(debug_assertions));
        while let Some(size) = sizes.first().copied() {
            sizes.remove(0);
            let md = case(name, size);
            let (count, took) = best_of(|| events(&md, GFM).count());
            eprintln!(
                "{name} {} B {:.3} s ({count} events)",
                md.len(),
                took.as_secs_f64()
            );
            // A shape under the floor at 2 MB is re-run at 4 and 8 MB for one real ratio.
            if cfg!(not(debug_assertions)) && size == 2 * MB && took < FLOOR {
                sizes.extend([4 * MB, 8 * MB]);
            }
            series.push((size, took));
        }
        #[cfg(not(debug_assertions))]
        assert_linear(name, &series);
        #[cfg(debug_assertions)]
        let _ = series;
    }
}

// ---- sharding and sizes of the matrix -----------------------------------------------------------

/// The matrix's shapes in their fixed order, which the shards are cut from: `CASES` as listed.
fn all_shapes() -> Vec<&'static str> {
    CASES.to_vec()
}

/// `RUBRICA_LINEAR_SHARD=i/N` (1-based) read as `(i, N)`; anything else is an error.
fn parse_shard(value: &str) -> Result<(usize, usize), String> {
    let malformed = || {
        format!(
            "RUBRICA_LINEAR_SHARD is {value:?}: want i/N with 1 <= i <= N, for example 1/4 \
             (unset runs every shape)"
        )
    };
    let (i, n) = value.split_once('/').ok_or_else(malformed)?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(i) || !digits(n) {
        return Err(malformed());
    }
    let (i, n) = (
        i.parse::<usize>().map_err(|_| malformed())?,
        n.parse::<usize>().map_err(|_| malformed())?,
    );
    if i == 0 || i > n {
        return Err(malformed());
    }
    Ok((i, n))
}

/// The shard `RUBRICA_LINEAR_SHARD` selects: none when it is unset. A malformed value fails
/// the test, so a typo never runs zero shapes, or all of them, in its place.
fn shard_from_env() -> Option<(usize, usize)> {
    match std::env::var("RUBRICA_LINEAR_SHARD") {
        Err(std::env::VarError::NotPresent) => None,
        Err(e) => panic!("RUBRICA_LINEAR_SHARD is not readable: {e}"),
        Ok(v) => Some(parse_shard(&v).unwrap_or_else(|e| panic!("{e}"))),
    }
}

/// The shapes of shard `shard`, in the fixed order: the shape at index `k` of `all_shapes()`
/// belongs to shard `i` of `N` when `k % N == i - 1`. No shard is all of them.
fn shard_shapes(shard: Option<(usize, usize)>) -> Vec<&'static str> {
    let Some((i, n)) = shard else {
        return all_shapes();
    };
    all_shapes()
        .into_iter()
        .enumerate()
        .filter(|(k, _)| k % n == i - 1)
        .map(|(_, name)| name)
        .collect()
}

/// The sizes one shape is run at: release runs 0.5, 1 and 2 MB (the matrix may extend them);
/// debug builds, whose timing asserts are compiled out, run the smallest only.
fn sizes_for(debug: bool) -> Vec<usize> {
    if debug {
        vec![MB / 2]
    } else {
        vec![MB / 2, MB, 2 * MB]
    }
}

#[test]
fn the_shards_partition_the_shapes() {
    let all = all_shapes();
    for n in 1..=6 {
        let mut got: Vec<&str> = Vec::new();
        for i in 1..=n {
            let shard = shard_shapes(Some((i, n)));
            assert!(!shard.is_empty(), "shard {i}/{n} is empty");
            got.extend(shard);
        }
        assert_eq!(
            got.len(),
            all.len(),
            "N={n}: a shape is in two shards or in none"
        );
        got.sort_unstable();
        let mut want = all.clone();
        want.sort_unstable();
        assert_eq!(got, want, "N={n}: the shards are not the shapes, each once");
    }
    assert_eq!(shard_shapes(None), all);
    // The documented rule: index % N == i - 1.
    assert_eq!(shard_shapes(Some((1, 4)))[1], all[4]);
    assert_eq!(shard_shapes(Some((2, 4)))[0], all[1]);
}

#[test]
fn a_malformed_shard_is_an_error_not_a_guess() {
    assert_eq!(parse_shard("1/4"), Ok((1, 4)));
    assert_eq!(parse_shard("4/4"), Ok((4, 4)));
    assert_eq!(parse_shard("1/1"), Ok((1, 1)));
    for bad in [
        "0/4", "5/4", "x", "1/0", "", "1/", "/4", "1/4/2", "-1/4", " 1/4", "1/4 ", "a/b",
    ] {
        let err = parse_shard(bad).expect_err(bad);
        assert!(
            err.contains("RUBRICA_LINEAR_SHARD") && err.contains("i/N"),
            "{bad}: {err}"
        );
    }
}

#[test]
fn debug_builds_run_each_shape_once_at_the_smallest_size() {
    assert_eq!(sizes_for(true), [MB / 2]);
    assert_eq!(sizes_for(false), [MB / 2, MB, 2 * MB]);
}
