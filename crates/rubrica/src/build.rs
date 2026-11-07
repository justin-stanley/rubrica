//! `rubrica build`: the repository's git tree at a commit, built into
//! a static site in a new directory.
//!
//! The steps, each a separate failure: the configuration (`--config`, or
//! the neutral default), the repository and the commit (`--rev`), the tree at
//! it ([`crate::git`]: a snapshot, with modes, refused when it is over the
//! repository budget), the site ([`rubrica_site::Site::from_sources`]), the
//! findings it cannot be built with ([`rubrica_site::build::refusals`], no
//! page, no home page: exit 1, printed as `check` prints them, nothing
//! written), the history (incomplete is a warning, not a failure: a
//! shallow clone builds), and the output ([`crate::write`]: a temporary
//! directory renamed onto `out`; over [`build::MAX_OUTPUT_BYTES`], refused
//! as a finding too).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use rubrica_site::build::{self, BuildConfig, BuildError, ConfigFile};
use rubrica_site::{CheckOptions, Site, SiteConfig};

use crate::{git, report, write};

/// `rubrica build`'s arguments.
#[derive(Args)]
pub struct BuildArgs {
    /// The commit to build (a commit id, a branch, a tag; `HEAD` when
    /// absent).
    #[arg(long, default_value = "HEAD")]
    pub rev: String,
    /// Build this directory of the tree (`site/content`), not its root.
    #[arg(long)]
    pub subtree: Option<String>,
    /// The site settings (`site/site.toml`'s shape, `[site]` included); the
    /// neutral default when absent.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// The repository (a checkout or a bare repository).
    pub repo_dir: PathBuf,
    /// Where to write the site: a directory that does not exist yet, or is
    /// empty.
    pub out_dir: PathBuf,
}

fn fail(what: &str) -> ExitCode {
    eprintln!("rubrica build: {}", report::escape(what));
    ExitCode::from(2)
}

/// Exit 1, `refused` printed as `check` prints findings.
fn refuse(mut refused: Vec<report::Item>) -> ExitCode {
    report::sort(&mut refused);
    print!("{}", report::refusal_text(&refused));
    ExitCode::from(1)
}

/// `budget`'s findings, as `check` prints them.
fn over_budget(
    budget: &rubrica_site::RepoBudget,
    site_cfg: &SiteConfig,
    opts: CheckOptions,
) -> Vec<report::Item> {
    let none = Site::from_sources(Vec::new(), site_cfg);
    budget
        .findings()
        .iter()
        .map(|f| report::describe(f, &none, opts))
        .collect()
}

/// The output budget: [`build::MAX_OUTPUT_BYTES`], or, in debug builds
/// only, `RUBRICA_TEST_MAX_OUTPUT_BYTES` (for the tests).
fn max_output() -> u64 {
    if cfg!(debug_assertions) {
        if let Some(n) = std::env::var("RUBRICA_TEST_MAX_OUTPUT_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            return n;
        }
    }
    build::MAX_OUTPUT_BYTES
}

/// The configuration `--config` names, or the neutral defaults: the
/// build's half and the site's half of one `site.toml`. `build` and `check`
/// both read it through this function.
pub fn load_config(path: Option<&Path>) -> Result<(BuildConfig, SiteConfig), String> {
    let Some(path) = path else {
        return Ok((BuildConfig::default(), SiteConfig::default()));
    };
    let shown = path.display();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
    let file: ConfigFile = toml::from_str(&text).map_err(|e| format!("{shown}: {e}"))?;
    file.into_configs().map_err(|e| format!("{shown}: {e}"))
}

/// Exit 0 when the site is written, 1 when the repository has findings it
/// cannot be built with (printed), 2 when it cannot be read or `out` cannot
/// be written (or the arguments are wrong).
pub fn run(args: &BuildArgs) -> ExitCode {
    let (cfg, site_cfg) = match load_config(args.config.as_deref()) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    let repo = match git::open(&args.repo_dir) {
        Ok(r) => r,
        Err(e) => return fail(&format!("{}: {e}", args.repo_dir.display())),
    };
    let opts = CheckOptions::default();
    let commit = match repo.resolve(&args.rev) {
        Ok(c) => c,
        Err(git::Unresolved::Refused(budget)) => {
            return refuse(over_budget(&budget, &site_cfg, opts))
        }
        Err(git::Unresolved::Bad(e)) => return fail(&e),
    };
    let subtree = args.subtree.as_deref();
    let (tree, budget) = match repo.read_tree(commit, subtree, &site_cfg) {
        Ok(t) => t,
        Err(e) => return fail(&e),
    };
    let mut refused: Vec<report::Item> =
        tree.problems.iter().map(report::describe_problem).collect();
    if !budget.holds() {
        // The read stopped at the budget: nothing else of it is looked at.
        refused.extend(over_budget(&budget, &site_cfg, opts));
        return refuse(refused);
    }
    let site = Site::from_sources(tree.files, &site_cfg);
    refused.extend(
        build::refusals(&site)
            .into_iter()
            .map(|f| report::describe(f, &site, opts)),
    );
    if site.pages().is_empty() && !report::over_budget(&site) {
        refused.push(report::no_pages());
    } else if !site.pages().is_empty() && !site.pages().keys().any(|p| p.is_home()) {
        refused.push(report::no_home_page());
    }
    if !refused.is_empty() {
        return refuse(refused);
    }
    let files: BTreeSet<String> = site.pages().values().map(|p| p.file.to_string()).collect();
    let feed = cfg.feed.as_ref().map(|f| f.source.as_str());
    let walked = match repo.history(commit, subtree, &files, feed) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let history = walked.history;
    let mut out = match write::OutDir::create(&args.out_dir) {
        Ok(o) => o,
        Err(e) => return fail(&e.to_string()),
    };
    let built = build::build_bounded(
        &site,
        &cfg,
        &history,
        &mut |page| {
            let id = page
                .blob_id
                .as_deref()
                .ok_or_else(|| std::io::Error::other(format!("{}: no blob id", page.file)))?;
            repo.blob(id).map_err(std::io::Error::other)
        },
        &mut |path, bytes| out.write(path, bytes),
        max_output(),
    );
    let built = match built {
        Ok(b) => b,
        // `out` is dropped here, and its temporary directory removed.
        Err(BuildError::OutputOverBudget { limit }) => {
            return refuse(vec![report::output_over_budget(limit)])
        }
        Err(BuildError::Io(e)) => return fail(&e.to_string()),
        Err(e) => return fail(&e.to_string()),
    };
    if let Err(e) = out.commit() {
        return fail(&e.to_string());
    }
    let others = site.findings().len();
    println!(
        "rubrica build: {} page(s), {} file(s), {} bytes, from {} into {}",
        built.pages,
        built.files,
        built.bytes,
        history.commit.id,
        report::escape(&args.out_dir.display().to_string())
    );
    if others > 0 {
        eprintln!("rubrica build: {others} finding(s) that do not stop a build; rubrica check reports them");
    }
    if let Some(why) = &walked.why_incomplete {
        eprintln!(
            "rubrica build: warning: the history is incomplete: {}; a page whose last change it did not reach has no date, and recent changes stop there",
            report::escape(why)
        );
    }
    ExitCode::SUCCESS
}
