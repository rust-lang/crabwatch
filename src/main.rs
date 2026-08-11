use anyhow::{Context, bail};
use clap::{ArgGroup, Parser, Subcommand};
use log::LevelFilter;
use std::path::PathBuf;

mod clone;
mod command;
mod github;
mod scan;

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

    match cli.command {
        Command::Analyze {
            repo,
            org,
            check: _,
        } => {
            command::analyze::run(
                repo,
                org,
                cli.cache_dir.as_deref(),
                cli.github_token.as_deref(),
            )
            .await?;
        }
    }
    Ok(())
}
