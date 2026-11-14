//! The diagram crate's linear-time tests, run in release by the `linear` and `linear-arm64`
//! jobs: the flowchart layout's shape matrix, and the limits it holds.
//!
//! Locally a doubling may cost at most 2.5x, timed as the best of three runs, within a 2 s
//! ceiling. The hosted runners (`RUBRICA_LINEAR_CI=1`) are shared and noisy: the best of five
//! runs, at most 3.0x per doubling, within 6 s. Quadratic growth shows about 4x either way.
//! A doubling whose smaller time is under the 50 ms floor is timer noise and is not compared.
//! A debug or coverage build runs each shape once at the smallest size and times nothing.

use rubrica_diagram::render;
use std::time::Duration;
#[cfg(not(debug_assertions))]
use std::time::Instant;

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

#[test]
fn the_limits_follow_the_hosted_ci_switch() {
    let local = limits_for(false);
    assert_eq!((local.ratio, local.repeats), (2.5, 3));
    assert_eq!(local.ceiling, Duration::from_secs(2));
    let hosted = limits_for(true);
    assert_eq!((hosted.ratio, hosted.repeats), (3.0, 5));
    assert_eq!(hosted.ceiling, Duration::from_secs(6));
}

/// Under this, the smaller time of a doubling is noise and the pair is not compared.
#[cfg(not(debug_assertions))]
const FLOOR: Duration = Duration::from_millis(50);

/// Renders per timing: one render of a 300-node flowchart is well under a millisecond.
#[cfg(not(debug_assertions))]
const R: usize = 100;

/// Whether `RUBRICA_LINEAR_CI=1` selects the hosted-CI limits.
#[cfg(not(debug_assertions))]
fn hosted_ci() -> bool {
    std::env::var("RUBRICA_LINEAR_CI").is_ok_and(|v| v == "1")
}

/// The best of `repeats` timings of `R` renders of `src`.
#[cfg(not(debug_assertions))]
fn best_of(src: &str, repeats: usize) -> Duration {
    (0..repeats)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..R {
                std::hint::black_box(render(std::hint::black_box(src)).ok());
            }
            start.elapsed()
        })
        .min()
        .unwrap()
}

const SHAPES: [&str; 6] = ["chain", "fan", "tree", "ladder", "labelled", "clusters"];

/// The flowchart of shape `name` with `n` nodes (`graph TD`).
fn shape(name: &str, n: usize) -> String {
    let mut s = String::from("graph TD\n");
    let mut line = |l: String| {
        s.push_str(&l);
        s.push('\n');
    };
    match name {
        "chain" => (0..n - 1).for_each(|i| line(format!("n{i} --> n{}", i + 1))),
        "fan" => (0..n - 1).for_each(|i| line(format!("hub --> n{i}"))),
        "tree" => {
            for i in 0..n {
                for c in [2 * i + 1, 2 * i + 2] {
                    if c < n {
                        line(format!("n{i} --> n{c}"));
                    }
                }
            }
        }
        "ladder" => {
            for i in 0..n / 2 - 1 {
                line(format!("a{i} --> a{}", i + 1));
                line(format!("b{i} --> b{}", i + 1));
            }
            (0..n / 2).for_each(|i| line(format!("a{i} --> b{i}")));
        }
        "labelled" => (0..n - 1).for_each(|i| line(format!("n{i} -->|e{i}| n{}", i + 1))),
        "clusters" => {
            for k in 0..n.div_ceil(10) {
                line(format!("subgraph g{k}"));
                (k * 10..(k * 10 + 10).min(n)).for_each(|i| line(format!("n{i}")));
                line("end".to_string());
            }
            (0..n - 1).for_each(|i| line(format!("n{i} --> n{}", i + 1)));
        }
        _ => panic!("unknown shape {name}"),
    }
    s
}

/// Each shape at n = 75, 150 and 300 nodes, timed as the best of `repeats` runs of `R`
/// renders: per doubling `t(2n) <= ratio * t(n)` unless `t(n)` is under the floor, and every
/// `t(300)` under the ceiling. Every output is asserted first.
#[test]
fn layout_time_is_linear_in_nodes() {
    #[cfg_attr(debug_assertions, allow(unused_mut))]
    let mut failures: Vec<String> = Vec::new();
    let sizes: &[usize] = if cfg!(debug_assertions) {
        &[75]
    } else {
        &[75, 150, 300]
    };
    for name in SHAPES {
        #[cfg_attr(debug_assertions, allow(unused_mut, unused_variables))]
        let mut series: Vec<(usize, Duration)> = Vec::new();
        for &n in sizes {
            let src = shape(name, n);
            let nodes = rubrica_diagram::flowchart::parse(&src).map(|f| f.nodes.len());
            assert_eq!(nodes, Ok(n), "{name} n={n}: the shape's nodes");
            if let Err(e) = render(&src) {
                panic!("{name} n={n}: render failed: {e:?}");
            }
            #[cfg(not(debug_assertions))]
            {
                let t = best_of(&src, limits_for(hosted_ci()).repeats);
                eprintln!("{name} n={n}: {:.3} s for {R} renders", t.as_secs_f64());
                series.push((n, t));
            }
        }
        #[cfg(not(debug_assertions))]
        {
            let limits = limits_for(hosted_ci());
            for w in series.windows(2) {
                let ((n1, t1), (n2, t2)) = (w[0], w[1]);
                if t1 < FLOOR {
                    eprintln!(
                        "{name} {n1}->{n2}: under the floor ({:.3} s)",
                        t1.as_secs_f64()
                    );
                    continue;
                }
                let ratio = t2.as_secs_f64() / t1.as_secs_f64();
                eprintln!("{name} {n1}->{n2}: ratio {ratio:.2}");
                if ratio > limits.ratio {
                    failures.push(format!(
                        "{name} {n1}->{n2}: ratio {ratio:.2} over {}",
                        limits.ratio
                    ));
                }
            }
            if let Some(&(n, t)) = series.last() {
                if t > limits.ceiling {
                    failures.push(format!(
                        "{name} n={n}: {:.3} s over the ceiling",
                        t.as_secs_f64()
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
