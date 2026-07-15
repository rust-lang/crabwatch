use crate::{clone, github};
use anyhow::{Context as _, bail};

use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
pub struct ParsedRepo {
    pub org: String,
    pub repo: String,
}

pub fn parse_repo(input: &str) -> anyhow::Result<ParsedRepo> {
    let parts: Vec<&str> = input.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        bail!("--repo must be in the form owner/name");
    }
    Ok(ParsedRepo {
        org: parts[0].to_string(),
        repo: parts[1].to_string(),
    })
}

pub fn cache_path(
    repo: &ParsedRepo,
    cache_dir_override: Option<&Path>,
    sha: &str,
) -> Option<PathBuf> {
    let base = match cache_dir_override {
        Some(path) => path.to_path_buf(),
        None => dirs::cache_dir()?.join("crabwatch"),
    };
    Some(
        base.join("repos")
            .join(&repo.org)
            .join(&repo.repo)
            .join(sha),
    )
}

pub async fn run(
    repo_arg: Option<String>,
    org_arg: Option<String>,
    cache_dir_override: Option<&Path>,
    token: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(repo_arg) = repo_arg {
        let parsed = parse_repo(&repo_arg)?;

        let token =
            token.context("a GitHub token is required (--github-token or GITHUB_TOKEN env var)")?;

        let client = reqwest::Client::new();
        let sha = github::fetch_head_commit(&client, &parsed.org, &parsed.repo, token).await?;

        println!("HEAD commit: {sha}");

        let path = cache_path(&parsed, cache_dir_override, &sha)
            .context("no cache directory available; try passing --cache-dir")?;

        if path.exists() {
            println!("cache hit: {}", path.display());
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .context("failed to create cache parent directory")?;
            }
            println!("cloning into: {}", path.display());
            clone::clone_repo(&parsed.org, &parsed.repo, &path, &sha)?;
        }
    } else if org_arg.is_some() {
        bail!("--org is not yet supported");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_repo() -> ParsedRepo {
        ParsedRepo {
            org: "rust-lang".to_string(),
            repo: "crabwatch".to_string(),
        }
    }

    #[test]
    fn parses_valid_input() {
        let parsed = parse_repo("rust-lang/crabwatch").unwrap();
        assert_eq!(parsed, test_repo());
    }

    #[test]
    fn rejects_input_without_slash() {
        assert!(parse_repo("rust-lang").is_err());
    }

    #[test]
    fn rejects_empty_owner() {
        assert!(parse_repo("/crabwatch").is_err());
    }

    #[test]
    fn rejects_empty_name() {
        assert!(parse_repo("rust-lang/").is_err());
    }

    #[test]
    fn rejects_too_many_parts() {
        assert!(parse_repo("a/b/c").is_err());
    }

    #[test]
    fn cache_path_default_uses_cache_dir() {
        let repo = test_repo();
        let sha = "abc123";
        let path = cache_path(&repo, None, sha).unwrap();
        let expected = dirs::cache_dir()
            .unwrap()
            .join("crabwatch")
            .join("repos")
            .join("rust-lang")
            .join("crabwatch")
            .join(sha);
        assert_eq!(path, expected);
    }

    #[test]
    fn cache_path_override_replaces_base() {
        let repo = test_repo();
        let sha = "abc123";
        let override_dir = Path::new("/tmp/test-cache");
        let path = cache_path(&repo, Some(override_dir), sha).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/tmp/test-cache/repos/rust-lang/crabwatch").join(sha)
        );
    }
}
