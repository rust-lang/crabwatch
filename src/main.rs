use clap::{ArgGroup, Parser, Subcommand};
use std::path::PathBuf;

mod command;

/// Analyze CI and best practices across Rust project repos
#[derive(Parser)]
#[command(name = "crabwatch", version, about, long_about = None)]
struct Cli {
    /// Increase logging verbosity
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Directory where downloaded workflows are cached
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze {
            repo,
            org,
            check: _,
        } => {
            command::analyze::run(repo, org)?;
        }
    }
    Ok(())
}
