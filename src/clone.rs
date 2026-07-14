use anyhow::{Context as _, bail};
use std::path::Path;
use std::process::Command;

pub fn clone_repo(org: &str, repo: &str, dest: &Path) -> anyhow::Result<()> {
    let url = format!("https://github.com/{org}/{repo}.git");

    let status = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(&url)
        .arg(dest)
        .status()
        .context("failed to run git clone")?;

    if !status.success() {
        bail!("git clone failed for {org}/{repo} (exit status: {status})");
    }

    Ok(())
}
