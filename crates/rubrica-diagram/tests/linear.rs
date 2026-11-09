//! The limits the diagram crate's linear-time tests hold, run in release by the `linear` and
//! `linear-arm64` jobs. The shape matrices arrive with the code they time; this file holds the
//! limits, so the jobs have a test to run and the limits are fixed before any row uses them.
//!
//! Locally a doubling may cost at most 2.5x, timed as the best of three runs, within a 2 s
//! ceiling. The hosted runners (`RUBRICA_LINEAR_CI=1`) are shared and noisy: the best of five
//! runs, at most 3.0x per doubling, within 6 s. Quadratic growth shows about 4x either way.

use std::time::Duration;

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
