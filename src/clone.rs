use anyhow::{Context as _, bail};
use std::path::Path;
use std::process::Command;

pub fn clone_repo(
    org: &str,
    repo: &str,
    final_dest: &Path,
    expected_sha: &str,
) -> anyhow::Result<()> {
    let temp_dest = final_dest.with_extension("tmp");

    // Clean up any leftover temp dir from a previously interrupted run.
    if temp_dest.exists() {
        std::fs::remove_dir_all(&temp_dest)
            .context("failed to remove leftover temp clone directory")?;
    }

    let url = format!("https://github.com/{org}/{repo}.git");

    let status = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(&url)
        .arg(&temp_dest)
        .status()
        .context("failed to run git clone")?;

    if !status.success() {
        let _ = std::fs::remove_dir_all(&temp_dest);
        bail!("git clone failed for {org}/{repo} ({status})");
    }

    let actual_sha = head_sha(&temp_dest)?;
    if actual_sha != expected_sha {
        let _ = std::fs::remove_dir_all(&temp_dest);
        bail!(
            "cloned SHA {actual_sha} does not match expected SHA {expected_sha} \
             (the repository may have been updated during the clone)"
        );
    }

    std::fs::rename(&temp_dest, final_dest)
        .context("failed to move cloned repository into cache")?;

    Ok(())
}

fn head_sha(repo_path: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--verify")
        .arg("HEAD^{commit}")
        .output()
        .context("failed to run git rev-parse")?;

    if !output.status.success() {
        bail!("git rev-parse failed ({})", output.status);
    }

    let sha = String::from_utf8(output.stdout)
        .context("git rev-parse output was not valid UTF-8")?
        .trim()
        .to_string();

    Ok(sha)
}
