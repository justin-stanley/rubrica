# Fuzzing

The fuzz crate is its own workspace. `check.sh` builds every target on the pinned nightly, replays `regressions/` (inputs that once crashed a target) and runs each target on `corpus/spec`.

The `autolinks` and `differential` targets compare with upstream pulldown-cmark 0.13.6 and allow exactly one documented divergence: a table whose header row does not start with a `|` may interrupt a paragraph here, as in cmark-gfm, and not upstream. The same divergence reached through a lazy line is allowed too: a table inside a block quote whose header row is the quote paragraph's lazy continuation line (its source lacks the quote's `>` markers), as in `>|\n||\n>-|`; cmark-gfm opens that table, upstream never lets a table interrupt a paragraph through a lazy line. Inputs where our parse opens either kind of table are skipped by the comparison with tables on (`common.rs`); the allowance is listed in `docs/spec/deviations.md`.
