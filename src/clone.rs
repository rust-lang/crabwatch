use anyhow::{Context as _, anyhow, bail};
use std::fmt::{Display, Write as _};
use std::path::Path;
use tokio::process::Command;

pub async fn clone_repo(
    org: &str,
    repo: &str,
    github_token: &str,
    final_dest: &Path,
    expected_sha: &str,
) -> anyhow::Result<()> {
    let temp_dest = final_dest.with_extension("tmp");

    // Clean up any leftover temp dir from a previously interrupted run.
    if temp_dest.exists() {
        std::fs::remove_dir_all(&temp_dest)
            .context("failed to remove leftover temp clone directory")?;
    }

    let url = format!("https://{github_token}@github.com/{org}/{repo}.git");

    let output = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(url)
        .arg(&temp_dest)
        .output()
        .await
        .context("failed to run git clone")?;

    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&temp_dest);
        return Err(clone_failure(
            org,
            repo,
            output.status,
            &output.stdout,
            &output.stderr,
        ));
    }

    let actual_sha = head_sha(&temp_dest).await?;

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

async fn head_sha(repo_path: &Path) -> anyhow::Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--verify")
        .arg("HEAD^{commit}")
        .output()
        .await
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

fn clone_failure(
    org: &str,
    repo: &str,
    status: impl Display,
    stdout: &[u8],
    stderr: &[u8],
) -> anyhow::Error {
    let mut message =
        format!("git clone failed for https://github.com/{org}/{repo} (status = {status})");
    append_diagnostic(&mut message, "stdout", stdout);
    append_diagnostic(&mut message, "stderr", stderr);
    anyhow!(message)
}

fn append_diagnostic(message: &mut String, label: &str, output: &[u8]) {
    if output.is_empty() {
        return;
    }

    let output = String::from_utf8_lossy(output);
    write!(message, "\n{label}:\n{}", output.trim()).expect("writing to a String cannot fail");
}
