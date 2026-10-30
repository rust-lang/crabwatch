use anyhow::{Context as _, anyhow, bail};
use base64::prelude::{BASE64_STANDARD, Engine as _};
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

    let output = git_clone_command(org, repo, github_token, &temp_dest)
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

/// Builds the `git clone` command.
///
/// The token is passed as an HTTP header via an environment variable.
/// This is more secure than embedding the token in the URL, as:
/// * Git prints the URL in error messages and stores it in the clone's `.git/config`.
/// * Process arguments are visible to other users on the same machine.
fn git_clone_command(org: &str, repo: &str, github_token: &str, dest: &Path) -> Command {
    // GitHub expects Basic auth with the token as password for Git over HTTPS.
    let credentials = BASE64_STANDARD.encode(format!("x-access-token:{github_token}"));
    let mut command = Command::new("git");
    command
        .env(
            "CRABWATCH_GIT_AUTH_HEADER",
            format!("Authorization: Basic {credentials}"),
        )
        // Fails instead of waiting for a password if GitHub rejects the token.
        // Git runs askpass programs even with terminal prompts disabled, e.g.
        // the password dialog that VS Code sets in `GIT_ASKPASS`. An empty
        // `GIT_ASKPASS` turns off `core.askPass` and `SSH_ASKPASS` too.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        // Crabwatch doesn't need Git LFS files, so don't download them.
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        // Turns off credential helpers for github.com. Otherwise, if GitHub
        // rejects the token, git asks the helpers for the user's saved GitHub
        // login, and when the retry fails too, git tells every helper to erase
        // that credential.
        // An empty value empties the list of helpers.
        .arg("-c")
        .arg("credential.https://github.com.helper=")
        // Clears github.com headers from the user's git config: GitHub rejects
        // requests with two `Authorization` headers. Must come before
        // `--config-env` because git applies them in order, or it would also
        // clear our header.
        .arg("-c")
        .arg("http.https://github.com/.extraheader=")
        // Reads the header from the variable above. Scoped to
        // github.com so the header isn't sent to any other host.
        .arg("--config-env=http.https://github.com/.extraheader=CRABWATCH_GIT_AUTH_HEADER")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        // Has a username so that users' `insteadOf` rules for
        // `https://github.com/` (e.g. to SSH) don't match.
        .arg(format!(
            "https://x-access-token@github.com/{org}/{repo}.git"
        ))
        .arg(dest);
    command
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
