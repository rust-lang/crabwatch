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

/// Whether to hide the name, zizmor output and errors of the repository.
fn is_redacted(repo: &github::OrgRepo, redact_private: bool) -> bool {
    redact_private && repo.is_private
}

/// Lists the repositories of an error, one per line and sorted by name. The
/// redacted repositories are only counted.
fn repo_list<'a>(
    lines: impl IntoIterator<Item = (&'a github::OrgRepo, String)>,
    redact_private: bool,
) -> String {
    let (redacted, mut listed): (Vec<_>, Vec<_>) = lines
        .into_iter()
        .partition(|(repo, _)| is_redacted(repo, redact_private));
    listed.sort_by(|(left, _), (right, _)| left.name.cmp(&right.name));

    let count = redacted.len();
    let noun = if count == 1 {
        "repository"
    } else {
        "repositories"
    };
    listed
        .into_iter()
        .map(|(_, line)| line)
        .chain((count > 0).then(|| format!("  {count} private {noun}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lists both the repositories that failed and the ones with findings, so
/// that a failed repository doesn't hide the findings of the others.
fn org_scan_error(
    org: &str,
    total_repos: usize,
    failures: &[(github::OrgRepo, String)],
    repos_with_findings: &[github::OrgRepo],
    redact_private: bool,
) -> Option<String> {
    let mut sections = Vec::new();
    if !failures.is_empty() {
        let lines = failures
            .iter()
            .map(|(repo, error)| (repo, format!("  {org}/{}: {error}", repo.name)));
        sections.push(format!(
            "organization scan for {org} incomplete: {} of {total_repos} repositories failed:\n{}",
            failures.len(),
            repo_list(lines, redact_private)
        ));
    }
    if !repos_with_findings.is_empty() {
        let lines = repos_with_findings
            .iter()
            .map(|repo| (repo, format!("  {org}/{}", repo.name)));
        sections.push(format!(
            "zizmor reported findings in {} of {total_repos} repositories of {org}:\n{}",
            repos_with_findings.len(),
            repo_list(lines, redact_private)
        ));
    }
    (!sections.is_empty()).then(|| sections.join("\n"))
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
        let mut repos_with_findings = Vec::new();

        let mut stream = futures::stream::iter(repos)
            .map(|repo| {
                let parsed = ParsedRepo {
                    org: org.clone(),
                    repo: repo.name.clone(),
                };
                async move {
                    let result =
                        analyze_repo(client, &parsed, crabwatch_dir, policy, github_token).await;
                    (repo, result)
                }
            })
            .buffer_unordered(MAX_CONCURRENT_REPOS);

        while let Some((repo, result)) = stream.next().await {
            let redacted = is_redacted(&repo, redact_private);
            match result {
                Ok((output, outcome)) => {
                    if !redacted {
                        log::log!(level_for(&outcome), "=== {org}/{} ===\n{output}", repo.name);
                    }
                    if matches!(outcome, scan::ScanOutcome::Findings) {
                        repos_with_findings.push(repo);
                    }
                }

                Err(err) => {
                    let error = format!("{err:#}");
                    if !redacted {
                        log::error!("failed to scan {org}/{}: {error}", repo.name);
                    }
                    failures.push((repo, error));
                }
            }
        }

        if let Some(error) = org_scan_error(
            org,
            total_repos,
            &failures,
            &repos_with_findings,
            redact_private,
        ) {
            bail!("{error}");
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

    fn org_repo(name: &str, is_private: bool) -> github::OrgRepo {
        github::OrgRepo {
            name: name.to_string(),
            is_private,
        }
    }

    #[test]
    fn redacted_errors_only_count_private_repositories() {
        let failures = [
            (org_repo("public-fail", false), "clone failed".to_string()),
            (org_repo("secret-fail", true), "secret stderr".to_string()),
        ];
        let findings = [
            org_repo("public-findings", false),
            org_repo("secret-findings-1", true),
            org_repo("secret-findings-2", true),
        ];

        let error = org_scan_error("rust-lang", 10, &failures, &findings, true).unwrap();

        // The error ends up in public CI logs, so neither the names nor the
        // errors of private repositories may appear in it.
        assert!(!error.contains("secret"));
        // Public repositories are still listed, and the totals still count
        // the private ones.
        insta::assert_snapshot!(error, @r"
        organization scan for rust-lang incomplete: 2 of 10 repositories failed:
          rust-lang/public-fail: clone failed
          1 private repository
        zizmor reported findings in 3 of 10 repositories of rust-lang:
          rust-lang/public-findings
          2 private repositories
        ");

        // Without the flag, maintainers see every repository.
        let error = org_scan_error("rust-lang", 10, &failures, &findings, false).unwrap();
        assert!(error.contains("rust-lang/secret-findings-1"));
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
