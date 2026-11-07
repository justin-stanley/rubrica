//! The `rubrica` binary: `check` and `build`; `parity` and
//! later `serve` arrive later.

mod build;
mod escape_table;
mod git;
mod read;
mod report;
mod write;

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use rubrica_render::frontmatter::Severity;
use rubrica_site::{CheckOptions, Finding, Site};

/// A static wiki renderer: `check` and `build` a Markdown repository.
#[derive(Parser)]
#[command(name = "rubrica", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a wiki repository: links, anchors, front matter, `@sec` ids, paths.
    Check(CheckArgs),
    /// Build a wiki repository's git tree, at a commit, into a static site.
    Build(build::BuildArgs),
}

/// `rubrica check`'s arguments.
#[derive(Args)]
struct CheckArgs {
    /// A dead anchor is an error, and any warning fails the check.
    #[arg(long)]
    strict: bool,
    /// Report dead same-page `](#anchor)` links too.
    #[arg(long)]
    same_page: bool,
    /// Wiki.js's limits: varchar(255), its front-matter reading, its paths.
    #[arg(long)]
    wikijs_limits: bool,
    /// A `site.toml`, as `build` reads it: its `[site]` section sets the
    /// profile, the ignore lists, the wiki's origin and the limits.
    #[arg(long)]
    config: Option<PathBuf>,
    /// The report's format.
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    /// The repository's directory.
    repo_dir: PathBuf,
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
}

fn main() -> ExitCode {
    let Cli { command } = Cli::parse();
    match command {
        Command::Check(args) => check(&args),
        Command::Build(args) => build::run(&args),
    }
}

/// `rubrica check`: exit 0 when clean (or only warnings, without
/// `--strict`), 1 on any error (or any warning, with `--strict`), 2 when
/// the repository cannot be read or the arguments are wrong (clap's).
fn check(args: &CheckArgs) -> ExitCode {
    let cfg = match build::load_config(args.config.as_deref()) {
        Ok((_, mut site_cfg)) => {
            site_cfg.wikijs_limits |= args.wikijs_limits;
            site_cfg
        }
        Err(e) => {
            eprintln!("rubrica check: {}", report::escape(&e));
            return ExitCode::from(2);
        }
    };
    let tree = match read::read_tree(&args.repo_dir, &cfg) {
        Ok(t) => t,
        Err(e) => {
            eprintln!(
                "rubrica check: {}: {}",
                report::escape(&args.repo_dir.to_string_lossy()),
                report::escape(&e.to_string())
            );
            return ExitCode::from(2);
        }
    };
    let site = Site::from_sources(tree.files, &cfg);
    let opts = CheckOptions {
        strict: args.strict,
    };
    let mut findings: Vec<report::Item> =
        tree.problems.iter().map(report::describe_problem).collect();
    let mut same_page_unchecked = 0;
    for f in site.findings() {
        if !args.same_page
            && matches!(
                f,
                Finding::BrokenAnchor {
                    same_page: true,
                    ..
                }
            )
        {
            same_page_unchecked += 1;
            continue;
        }
        findings.push(report::describe(f, &site, opts));
    }
    if site.pages().is_empty() && !report::over_budget(&site) {
        findings.push(report::no_pages());
    }
    report::sort(&mut findings);
    let count = |s: Severity| findings.iter().filter(|f| f.severity == s).count();
    let (errors, warnings) = (count(Severity::Error), count(Severity::Warning));
    let failed = errors > 0 || (args.strict && warnings > 0);
    let r = report::Report {
        format: 1,
        options: report::Options {
            strict: args.strict,
            same_page: args.same_page,
            wikijs_limits: cfg.wikijs_limits,
        },
        pages: site.pages().len(),
        links: site.pages().values().map(|p| p.links.len()).sum(),
        errors,
        warnings,
        same_page_unchecked,
        failed,
        findings,
    };
    let out = match args.format {
        Format::Text => report::text(&r),
        Format::Json => report::json(&r),
    };
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
    {
        eprintln!("rubrica check: writing the report: {e}");
        return ExitCode::from(2);
    }
    ExitCode::from(u8::from(failed))
}
