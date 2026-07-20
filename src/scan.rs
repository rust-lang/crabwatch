use anyhow::{Context as _, bail};
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

const ZIZMOR_CONFIG: &str = include_str!("../zizmor-default.yml");

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

pub fn scan_workflows(repo_path: &Path, github_token: &str) -> anyhow::Result<()> {
    let config_path = std::env::temp_dir().join("crabwatch-zizmor-default.yml");
    std::fs::write(&config_path, ZIZMOR_CONFIG)
        .context("failed to write zizmor config to a temporary file")?;

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
