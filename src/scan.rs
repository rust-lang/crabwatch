use anyhow::{Context as _, bail};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::process::Command;

const ZIZMOR_CONFIG: &str = include_str!("../zizmor-default.yml");
const ZIZMOR_CONFIG_FILE: &str = "zizmor-default.yml";
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

fn zizmor_command(repo_path: &Path, config_path: &Path, github_token: &str) -> Command {
    let mut command = Command::new("zizmor");
    command
        .env("ZIZMOR_GITHUB_TOKEN", github_token)
        .arg("--config")
        .arg(config_path)
        // Fail on GitHub workflow syntax error.
        .arg("--strict-collection")
        .arg(repo_path);
    command
}

pub(crate) fn sync_zizmor_config(crabwatch_dir: &Path) -> anyhow::Result<PathBuf> {
    let config_path = crabwatch_dir.join(ZIZMOR_CONFIG_FILE);

    match std::fs::read(&config_path) {
        // Config is already present and identical, return early
        Ok(contents) if contents == ZIZMOR_CONFIG.as_bytes() => return Ok(config_path),
        // Config is already present but different, overwrite it
        Ok(_) => {}
        // Config is not present, create it
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read zizmor config at {config_path:?}"));
        }
    }

    std::fs::create_dir_all(crabwatch_dir)
        .with_context(|| format!("failed to create Crabwatch directory at {crabwatch_dir:?}"))?;
    std::fs::write(&config_path, ZIZMOR_CONFIG)
        .with_context(|| format!("failed to write zizmor config at {config_path:?}"))?;

    Ok(config_path)
}

pub async fn scan_workflows(
    repo_path: &Path,
    config_path: &Path,
    github_token: &str,
) -> anyhow::Result<ScanReport> {
    let output = zizmor_command(repo_path, config_path, github_token)
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

    #[test]
    fn creates_config_and_keeps_identical_file() {
        // The first sync should create the directory and bundled config from scratch.
        let temp_dir = tempfile::tempdir().unwrap();
        let crabwatch_dir = temp_dir.path().join("crabwatch");
        let config_path = sync_zizmor_config(&crabwatch_dir).unwrap();

        // Make the generated file read-only so a second sync can succeed only by
        // recognizing the identical contents and returning without rewriting it.
        let original_metadata = std::fs::metadata(&config_path).unwrap();
        let original_permissions = original_metadata.permissions();
        let mut read_only_permissions = original_permissions.clone();
        read_only_permissions.set_readonly(true);
        std::fs::set_permissions(&config_path, read_only_permissions).unwrap();

        let second_path = sync_zizmor_config(&crabwatch_dir).expect("failed to sync config a second time. Maybe the read-only permission prevented it from being overwritten?");
        let second_metadata = std::fs::metadata(&second_path).unwrap();

        std::fs::set_permissions(&config_path, original_permissions).unwrap();

        assert_eq!(config_path, second_path);
        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            ZIZMOR_CONFIG
        );
        // The modification time should be preserved because the second sync
        // should not have rewritten the file.
        assert_eq!(
            original_metadata.modified().unwrap(),
            second_metadata.modified().unwrap()
        );
    }

    #[test]
    fn overwrites_different_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let crabwatch_dir = temp_dir.path().join("crabwatch");
        std::fs::create_dir_all(&crabwatch_dir).unwrap();
        let config_path = crabwatch_dir.join(ZIZMOR_CONFIG_FILE);
        std::fs::write(&config_path, "different config").unwrap();

        sync_zizmor_config(&crabwatch_dir).unwrap();

        assert_eq!(
            std::fs::read_to_string(&config_path).unwrap(),
            ZIZMOR_CONFIG
        );
    }
}
