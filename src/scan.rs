use anyhow::{Context as _, bail};
use std::io::{self, ErrorKind, IsTerminal as _};
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Debug, PartialEq)]
pub enum ScanOutcome {
    Clean,
    Findings,
    NoWorkflows,
}

pub struct ScanReport {
    pub output: String,
    pub outcome: ScanOutcome,
}

fn zizmor_command(github_path: &Path, config_path: &Path, github_token: &str) -> Command {
    let mut command = Command::new("zizmor");
    command
        .env("ZIZMOR_GITHUB_TOKEN", github_token)
        .arg("--config")
        .arg(config_path)
        .arg("--persona")
        .arg("pedantic")
        // Fail on GitHub workflow syntax error.
        .arg("--strict-collection")
        .arg(github_path);
    if io::stdout().is_terminal() {
        command.arg("--color=always").arg("--render-links=always");
    }
    command
}

fn root_github_path(repo_path: &Path) -> anyhow::Result<Option<PathBuf>> {
    let github_path = repo_path.join(".github");
    let path = github_path
        .try_exists()
        .with_context(|| {
            format!(
                "failed to inspect GitHub directory at {}",
                github_path.display()
            )
        })?
        .then_some(github_path);
    Ok(path)
}

pub async fn scan_workflows(
    repo_path: &Path,
    config_path: &Path,
    github_token: &str,
) -> anyhow::Result<ScanReport> {
    let Some(github_path) = root_github_path(repo_path)? else {
        return Ok(ScanReport {
            output: "no workflows to scan".to_string(),
            outcome: ScanOutcome::NoWorkflows,
        });
    };

    let output = zizmor_command(&github_path, config_path, github_token)
        .output()
        .await;

    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            bail!("zizmor is not installed; see https://docs.zizmor.sh/installation/");
        }
        Err(err) => return Err(err).context("failed to run zizmor"),
    };
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    combined.push_str(&String::from_utf8_lossy(&output.stdout));

    match output.status.code() {
        Some(0) => Ok(ScanReport {
            output: combined,
            outcome: ScanOutcome::Clean,
        }),
        // Exit code 3 means no auditable inputs. With `--strict-collection`, that means
        // there were no workflows to scan (invalid workflows fail with a different code).
        Some(3) => Ok(ScanReport {
            output: "no workflows to scan".to_string(),
            outcome: ScanOutcome::NoWorkflows,
        }),
        // Exit codes 11-14 mean zizmor reported findings; the number is the top severity.
        Some(11..=14) => Ok(ScanReport {
            output: combined,
            outcome: ScanOutcome::Findings,
        }),
        _ => bail!(
            "zizmor failed ({})\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn nested_workflows_without_root_github_are_not_scanned() {
        let repo = tempfile::tempdir().unwrap();
        let nested_workflows = repo.path().join("vendor/project/.github/workflows");
        std::fs::create_dir_all(&nested_workflows).unwrap();
        std::fs::write(
            nested_workflows.join("publish.yml"),
            "on: push\njobs:\n  publish:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hello\n",
        )
        .unwrap();
        // Never read: without a root `.github`, zizmor is not run.
        let config_path = Path::new("unused-config.yml");

        let report = scan_workflows(repo.path(), config_path, "").await.unwrap();

        assert_eq!(report.outcome, ScanOutcome::NoWorkflows);
        assert_eq!(report.output, "no workflows to scan");
    }
}
