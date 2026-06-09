use clap::{Parser, Subcommand};
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
    /// List the Rust project repos
    ListRepos,
    /// Download workflow files for a repo
    Fetch {
        /// Repo in the form owner/name
        repo: String,
    },
    /// Run zizmor on a repo's cached workflows
    Analyze {
        /// Repo in the form owner/name
        repo: String,
    },
    /// List, fetch, and analyze every repo
    Run,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::ListRepos => println!("list-repos: not implemented yet"),
        Command::Fetch { repo } => println!("fetch {repo}: not implemented yet"),
        Command::Analyze { repo } => println!("analyze {repo}: not implemented yet"),
        Command::Run => println!("run: not implemented yet"),
    }
}
