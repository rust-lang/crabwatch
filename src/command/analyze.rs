use crate::{clone, github, scan};
use anyhow::{Context as _, anyhow, bail};
use futures::stream::StreamExt;
use std::fmt::Write;
use std::path::{Path, PathBuf};

const MAX_CONCURRENT_REPOS: usize = 8;

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

fn crabwatch_dir(cache_dir_override: Option<&Path>) -> anyhow::Result<PathBuf> {
    match cache_dir_override {
        Some(path) => Ok(path.to_path_buf()),
        None => dirs::cache_dir()
            .map(|path| path.join("crabwatch"))
            .context("no cache directory available; try passing --cache-dir"),
    }
}

fn incomplete_org_scan_error(
    org: &str,
    total_repos: usize,
    failures: &[(ParsedRepo, String)],
) -> anyhow::Error {
    let details = failures
        .iter()
        .map(|(repo, error)| format!("  {}/{}: {error}", repo.org, repo.repo))
        .collect::<Vec<_>>()
        .join("\n");

    anyhow!(
        "organization scan for {org} incomplete: {} of {total_repos} repositories failed:\n{details}",
        failures.len()
    )
}

pub fn cache_path(repo: &ParsedRepo, crabwatch_dir: &Path, sha: &str) -> PathBuf {
    crabwatch_dir
        .join("repos")
        .join(&repo.org)
        .join(&repo.repo)
        .join(sha)
}

async fn analyze_repo(
    client: &reqwest::Client,
    parsed: &ParsedRepo,
    crabwatch_dir: &Path,
    zizmor_config: &Path,
    token: &str,
) -> anyhow::Result<String> {
    let mut out = String::new();

    let sha = github::fetch_head_commit(client, &parsed.org, &parsed.repo, token).await?;
    writeln!(out, "HEAD commit: {sha}")?;

    let path = cache_path(parsed, crabwatch_dir, &sha);

    if path.exists() {
        writeln!(out, "cache hit: {}", path.display())?;
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("failed to create cache parent directory")?;
        }
        writeln!(out, "cloning into: {}", path.display())?;
        clone::clone_repo(&parsed.org, &parsed.repo, &path, &sha).await?;
    }
    let scan_output = scan::scan_workflows(&path, zizmor_config, token).await?;
    writeln!(out, "{}", scan_output.trim_end())?;

    Ok(out)
}

pub async fn run(
    repo_arg: Option<String>,
    org_arg: Option<String>,
    cache_dir_override: Option<&Path>,
    token: Option<&str>,
) -> anyhow::Result<()> {
    let token =
        token.context("a GitHub token is required (--github-token or GITHUB_TOKEN env var)")?;
    let client = reqwest::Client::new();
    let crabwatch_dir = crabwatch_dir(cache_dir_override)?;
    let zizmor_config = scan::sync_zizmor_config(&crabwatch_dir)?;

    if let Some(repo_arg) = repo_arg {
        let parsed = parse_repo(&repo_arg)?;
        let output = analyze_repo(&client, &parsed, &crabwatch_dir, &zizmor_config, token).await?;
        print!("{output}");
    } else if let Some(org) = org_arg {
        let repos = github::list_org_repos(&client, &org, token).await?;
        let total_repos = repos.len();
        println!("found {total_repos} repos in {org}");

        let client = &client;
        let crabwatch_dir = &crabwatch_dir;
        let zizmor_config = &zizmor_config;
        let org = &org;
        let mut failures = Vec::new();

        let mut stream = futures::stream::iter(repos)
            .map(|repo| {
                let parsed = ParsedRepo {
                    org: org.clone(),
                    repo,
                };
                async move {
                    let result =
                        analyze_repo(client, &parsed, crabwatch_dir, zizmor_config, token).await;
                    (parsed, result)
                }
            })
            .buffer_unordered(MAX_CONCURRENT_REPOS);

        while let Some((parsed, result)) = stream.next().await {
            println!("\n=== {}/{} ===", parsed.org, parsed.repo);
            match result {
                Ok(output) => print!("{output}"),
                Err(err) => {
                    let error = format!("{err:#}");
                    eprintln!("failed to scan {}/{}: {error}", parsed.org, parsed.repo);
                    failures.push((parsed, error));
                }
            }
        }

        if !failures.is_empty() {
            failures.sort_by(|(left, _), (right, _)| left.repo.cmp(&right.repo));
            return Err(incomplete_org_scan_error(org, total_repos, &failures));
        }
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
        let crabwatch_dir = crabwatch_dir(None).unwrap();
        let path = cache_path(&repo, &crabwatch_dir, sha);
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
        let crabwatch_dir = crabwatch_dir(Some(override_dir)).unwrap();
        let path = cache_path(&repo, &crabwatch_dir, sha);
        assert_eq!(
            path,
            PathBuf::from("/tmp/test-cache/repos/rust-lang/crabwatch").join(sha)
        );
    }
}
