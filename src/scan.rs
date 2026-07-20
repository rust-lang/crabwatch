use anyhow::{Context as _, bail};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

const ZIZMOR_CONFIG: &str = include_str!("../zizmor-default.yml");
const ZIZMOR_CONFIG_FILE: &str = "zizmor-default.yml";

fn zizmor_command(repo_path: &Path, config_path: &Path, github_token: &str) -> Command {
    let mut command = Command::new("zizmor");
    command
        .env("ZIZMOR_GITHUB_TOKEN", github_token)
        .arg("--config")
        .arg(config_path)
        .arg("--no-exit-codes")
        .arg(repo_path);
    command
}

fn sync_zizmor_config(crabwatch_dir: &Path) -> anyhow::Result<PathBuf> {
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

pub fn scan_workflows(
    repo_path: &Path,
    crabwatch_dir: &Path,
    github_token: &str,
) -> anyhow::Result<()> {
    let config_path = sync_zizmor_config(crabwatch_dir)?;

    let status = zizmor_command(repo_path, &config_path, github_token).status();

    let status = match status {
        Ok(status) => status,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            bail!("zizmor is not installed; see https://docs.zizmor.sh/installation/");
        }
        Err(err) => return Err(err).context("failed to run zizmor"),
    };

    if !status.success() {
        bail!("zizmor failed ({status})");
    }

    Ok(())
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
