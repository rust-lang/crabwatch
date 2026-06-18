use anyhow::{bail, Result};
use clap::{ArgGroup, Parser, Subcommand};
use std::path::PathBuf;

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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze { repo: repo_arg, org, check } => {
            if let Some(repo_arg) = repo_arg {
                let parts: Vec<&str> = repo_arg.split('/').collect();
                if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
                    bail!("--repo must be in the form owner/name");
                }
                let (org, repo) = (parts[0], parts[1]);
                println!("parsed org={org} repo={repo}");
            }
        }
    }
    Ok(())
}
