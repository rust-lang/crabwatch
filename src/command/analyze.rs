use crate::{clone, config, github, scan};
use anyhow::{Context as _, bail};
use futures::stream::StreamExt;
use std::path::{Path, PathBuf};

const MAX_CONCURRENT_REPOS: usize = 8;

fn level_for(outcome: &scan::ScanOutcome) -> log::Level {
    match outcome {
        scan::ScanOutcome::Findings => log::Level::Info,
        scan::ScanOutcome::Clean | scan::ScanOutcome::NoWorkflows => log::Level::Debug,
    }
}

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
    private_failures: usize,
) -> String {
    let mut details = failures
        .iter()
        .map(|(repo, error)| format!("  {}/{}: {error}", repo.org, repo.repo))
        .collect::<Vec<_>>();
    if private_failures > 0 {
        details.push(format!("  private repositories: {private_failures}"));
    }

    format!(
        "organization scan for {org} incomplete: {} of {total_repos} repositories failed:\n{}",
        failures.len() + private_failures,
        details.join("\n")
    )
}

fn org_findings_error(
    org: &str,
    total_repos: usize,
    repos: &[String],
    private_repos: usize,
) -> String {
    let mut details = repos
        .iter()
        .map(|repo| format!("  {org}/{repo}"))
        .collect::<Vec<_>>();
    if private_repos > 0 {
        details.push(format!("  private repositories: {private_repos}"));
    }

    format!(
        "zizmor reported findings in {} of {total_repos} repositories of {org}:\n{}",
        repos.len() + private_repos,
        details.join("\n")
    )
}

pub fn cache_path(repo: &ParsedRepo, crabwatch_dir: &Path, sha: &str) -> PathBuf {
    crabwatch_dir
        .join("repos")
        .join(&repo.org)
        .join(&repo.repo)
        .join(sha)
}

/// Removes everything except the cached clone for the current commit.
fn prune_repo_cache(repo_cache_dir: &Path, keep_sha: &str) -> anyhow::Result<()> {
    let directory = std::fs::read_dir(repo_cache_dir)
        .with_context(|| format!("failed to read repository cache at {repo_cache_dir:?}",))?;

    for entry in directory {
        let entry = entry.with_context(|| {
            format!("failed to read an entry in repository cache at {repo_cache_dir:?}",)
        })?;
        let file_type = entry.file_type().with_context(|| {
            format!(
                "failed to inspect repository cache entry at {}",
                entry.path().display()
            )
        })?;

        if file_type.is_dir() && entry.file_name() == keep_sha {
            continue;
        }

        let path = entry.path();
        let result = if file_type.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        result.with_context(|| {
            format!(
                "failed to remove repository cache entry at {}",
                path.display()
            )
        })?;
    }

    Ok(())
}

async fn analyze_repo(
    client: &reqwest::Client,
    parsed: &ParsedRepo,
    crabwatch_dir: &Path,
    policy: &config::ZizmorPolicy,
    github_token: &str,
) -> anyhow::Result<(String, scan::ScanOutcome)> {
    let head = github::fetch_head_commit(client, &parsed.org, &parsed.repo, github_token).await?;
    // GitHub's canonical name selects the policy overrides. Kept alive until
    // the scan finishes; the file is deleted on drop.
    let config_file = policy.write_config(&head.name_with_owner).await?;
    let sha = head.sha;
    log::debug!("HEAD commit: {sha}");
    let path = cache_path(parsed, crabwatch_dir, &sha);
    let repo_cache_dir = path
        .parent()
        .context("repository cache path has no parent directory")?;
    std::fs::create_dir_all(repo_cache_dir)
        .context("failed to create repository cache directory")?;

    // Old versions cannot satisfy this scan, so remove them before cloning to
    // save disk space.
    prune_repo_cache(repo_cache_dir, &sha)?;

    if path.exists() {
        log::debug!("cache hit: {}", path.display());
    } else {
        log::debug!("cloning into: {}", path.display());
        clone::clone_repo(&parsed.org, &parsed.repo, github_token, &path, &sha).await?;
    }

    let report = scan::scan_workflows(&path, config_file.path(), github_token).await?;

    Ok((report.output.trim_end().to_string(), report.outcome))
}

pub async fn run(
    github_token: &str,
    repo_arg: Option<String>,
    org_arg: Option<String>,
    redact_private: bool,
    cache_dir_override: Option<&Path>,
) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let crabwatch_dir = crabwatch_dir(cache_dir_override)?;
    let policy = config::ZizmorPolicy::bundled().await?;

    if let Some(repo_arg) = repo_arg {
        let parsed = parse_repo(&repo_arg)?;
        let (output, outcome) =
            analyze_repo(&client, &parsed, &crabwatch_dir, &policy, github_token).await?;
        match outcome {
            scan::ScanOutcome::Findings => {
                log::info!("{output}");
                bail!("zizmor reported findings in {repo_arg}");
            }
            scan::ScanOutcome::Clean => log::info!("No findings to report."),
            scan::ScanOutcome::NoWorkflows => log::info!("No workflows found to scan."),
        }
    } else if let Some(org) = org_arg {
        let repos = github::list_org_repos(&client, &org, github_token).await?;
        let total_repos = repos.len();
        log::debug!("found {total_repos} repos with workflows in {org}");

        let client = &client;
        let crabwatch_dir = &crabwatch_dir;
        let policy = &policy;
        let org = &org;
        let mut failures = Vec::new();
        let mut private_failures = 0;
        let mut repos_with_findings = Vec::new();
        let mut private_with_findings = 0;

        let mut stream = futures::stream::iter(repos)
            .map(|repo| {
                let redacted = redact_private && repo.is_private;
                let parsed = ParsedRepo {
                    org: org.clone(),
                    repo: repo.name,
                };
                async move {
                    let result =
                        analyze_repo(client, &parsed, crabwatch_dir, policy, github_token).await;
                    (parsed, redacted, result)
                }
            })
            .buffer_unordered(MAX_CONCURRENT_REPOS);

        while let Some((parsed, redacted, result)) = stream.next().await {
            // The output and errors of private repositories can contain their
            // names and weaknesses, so only count them.
            if redacted {
                match result {
                    Ok((_, scan::ScanOutcome::Findings)) => private_with_findings += 1,
                    Ok(_) => {}
                    Err(_) => private_failures += 1,
                }
                continue;
            }

            match result {
                Ok((output, outcome)) => {
                    log::log!(
                        level_for(&outcome),
                        "=== {}/{} ===\n{output}",
                        parsed.org,
                        parsed.repo
                    );
                    if matches!(outcome, scan::ScanOutcome::Findings) {
                        repos_with_findings.push(parsed.repo);
                    }
                }

                Err(err) => {
                    let error = format!("{err:#}");
                    log::error!("failed to scan {}/{}: {error}", parsed.org, parsed.repo);
                    failures.push((parsed, error));
                }
            }
        }

        // Report both lists, so that a failed repository doesn't hide the
        // findings of the others.
        let mut errors = Vec::new();
        if !failures.is_empty() || private_failures > 0 {
            failures.sort_by(|(left, _), (right, _)| left.repo.cmp(&right.repo));
            errors.push(incomplete_org_scan_error(
                org,
                total_repos,
                &failures,
                private_failures,
            ));
        }
        if !repos_with_findings.is_empty() || private_with_findings > 0 {
            repos_with_findings.sort();
            errors.push(org_findings_error(
                org,
                total_repos,
                &repos_with_findings,
                private_with_findings,
            ));
        }
        if !errors.is_empty() {
            bail!("{}", errors.join("\n"));
        }
        log::info!("Scanned {total_repos} repositories, no findings to report.");
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
    fn findings_log_at_info() {
        assert_eq!(level_for(&scan::ScanOutcome::Findings), log::Level::Info);
    }

    #[test]
    fn clean_and_no_workflows_log_at_debug() {
        assert_eq!(level_for(&scan::ScanOutcome::Clean), log::Level::Debug);
        assert_eq!(
            level_for(&scan::ScanOutcome::NoWorkflows),
            log::Level::Debug
        );
    }

    #[test]
    fn error_lists_public_failures_and_counts_private_ones() {
        let failures = [(test_repo(), "clone failed".to_string())];

        let error = incomplete_org_scan_error("rust-lang", 10, &failures, 2);

        // Only the public failure is listed, but the total also includes the
        // 2 private failures, so it still matches how many repositories failed.
        insta::assert_snapshot!(error, @r"
        organization scan for rust-lang incomplete: 3 of 10 repositories failed:
          rust-lang/crabwatch: clone failed
          private repositories: 2
        ");
    }

    #[test]
    fn findings_error_lists_public_repositories_and_counts_private_ones() {
        let repos = ["crabwatch".to_string()];

        let error = org_findings_error("rust-lang", 10, &repos, 2);

        insta::assert_snapshot!(error, @r"
        zizmor reported findings in 3 of 10 repositories of rust-lang:
          rust-lang/crabwatch
          private repositories: 2
        ");
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

    #[test]
    fn keeps_only_the_current_version() {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo_cache_dir = temp_dir.path().join("rust-lang").join("crabwatch");
        std::fs::create_dir_all(&repo_cache_dir).unwrap();

        let current_sha = "1111111111111111111111111111111111111111";
        let old_sha = "2222222222222222222222222222222222222222";
        let stale_temp = format!("{}.tmp", "3333333333333333333333333333333333333333");
        let unrelated_dir = "saved-by-user";
        let unrelated_file = "notes.txt";

        for entry in [current_sha, old_sha, &stale_temp, unrelated_dir] {
            std::fs::create_dir(repo_cache_dir.join(entry)).unwrap();
        }
        std::fs::write(repo_cache_dir.join(unrelated_file), "not cache data").unwrap();

        prune_repo_cache(&repo_cache_dir, current_sha).unwrap();

        assert!(repo_cache_dir.join(current_sha).is_dir());
        assert!(!repo_cache_dir.join(old_sha).exists());
        assert!(!repo_cache_dir.join(stale_temp).exists());
        assert!(!repo_cache_dir.join(unrelated_dir).exists());
        assert!(!repo_cache_dir.join(unrelated_file).exists());
    }
}
