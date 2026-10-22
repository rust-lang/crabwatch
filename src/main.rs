use anyhow::{Context, bail};
use clap::{ArgGroup, Parser, Subcommand};
use log::LevelFilter;
use std::path::PathBuf;

mod clone;
mod command;
mod config;
mod github;
mod scan;
mod security_fork;

/// Analyze CI and best practices across Rust project repos
#[derive(Parser)]
#[command(name = "crabwatch", version, about, long_about = None)]
struct Cli {
    /// Directory where Crabwatch cache files are stored.
    /// This includes repositories analyzed by crabwatch.
    #[arg(long, global = true)]
    cache_dir: Option<PathBuf>,

    /// GitHub token
    #[arg(long, env = "GITHUB_TOKEN", hide_env_values = true, global = true)]
    github_token: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze a repository or an organization
    #[command(group(ArgGroup::new("target").required(true).args(["repo", "org"])))]
    Analyze {
        /// Analyze a single repository
        #[arg(long)]
        repo: Option<String>,

        /// Analyze every repository in an organization
        #[arg(long)]
        org: Option<String>,

        /// Hide the names, zizmor output and errors of private repositories,
        /// and only count them. Meant for public logs, such as CI.
        /// Debug logs (`CRABWATCH_LOG=debug`) can still show their names.
        // Not `requires = "org"`: clap doesn't enforce it with `--repo`,
        // because `--org` can't be passed together with `--repo`.
        #[arg(long, conflicts_with = "repo")]
        redact_private: bool,

        /// Specific check to run (runs all if omitted)
        #[arg(long)]
        check: Option<String>,
    },
}

const LOG_ENV: &str = "CRABWATCH_LOG";

fn init_logging() -> anyhow::Result<()> {
    let raw_level = std::env::var(LOG_ENV).unwrap_or_else(|_| "info".to_owned());

    let level = raw_level
        .parse::<LevelFilter>()
        .with_context(|| format!("invalid {LOG_ENV} value `{raw_level}`"))?;

    if !matches!(level, LevelFilter::Info | LevelFilter::Debug) {
        bail!("invalid {LOG_ENV} value `{raw_level}`; expected one of: info, debug");
    }

    env_logger::Builder::new()
        .filter_level(LevelFilter::Off)
        .filter_module(env!("CARGO_CRATE_NAME"), level)
        .format(|buf, record| {
            use std::io::Write;
            writeln!(buf, "{}", record.args())
        })
        .try_init()
        .map_err(|error| anyhow::anyhow!("failed to initialize logging: {error}"))?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    init_logging()?;

    let github_token = cli
        .github_token
        .context("a GitHub token is required (--github-token or GITHUB_TOKEN env var)")?;

    match cli.command {
        Command::Analyze {
            repo,
            org,
            redact_private,
            check: _,
        } => {
            command::analyze::run(
                &github_token,
                repo,
                org,
                redact_private,
                cli.cache_dir.as_deref(),
            )
            .await?;
        }
    }
    Ok(())
}
