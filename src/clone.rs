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

    // The username keeps `insteadOf` rules for `https://github.com/` from
    // rewriting the URL, e.g. to clone with the user's SSH key.
    let url = format!("https://x-access-token@github.com/{org}/{repo}.git");

    let output = Command::new("git")
        // Fail instead of waiting for credentials if the clone isn't authorized.
        .env("GIT_TERMINAL_PROMPT", "0")
        // Crabwatch doesn't need Git LFS files, so don't download them.
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        // Read by the credential helper below. Git rejects URLs with a password
        // if `transfer.credentialsInUrl` is `die`, so the token can't be in the
        // URL.
        .env("CRABWATCH_GITHUB_TOKEN", github_token)
        // Reset the credential helpers, so that git doesn't use the user's
        // helpers to get credentials or to save the token.
        .arg("-c")
        .arg("credential.helper=")
        // GitHub reads the token from the password. Only give it to GitHub: the
        // programs that git runs, like the Git LFS filter, also use this helper
        // for hosts that the cloned repository can choose.
        .arg("-c")
        .arg(r#"credential.https://github.com.helper=!f() { test "$1" = get && echo "password=$CRABWATCH_GITHUB_TOKEN"; }; f"#)
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
