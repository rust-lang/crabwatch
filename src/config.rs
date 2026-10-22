use anyhow::{Context as _, bail};
use std::io::Write as _;
use std::path::Path;
use tempfile::NamedTempFile;
use tokio::process::Command;

// Embed/bundle the zizmor policy inside the compiled Crabwatch executable.
const ZIZMOR_POLICY: &str = include_str!("../zizmor-policy.yml");

/// Builds a repository's zizmor configuration from the policy: the `default`
/// section with the repository's overrides deep-merged on top.
///
/// Must stay identical to the expression in `.github/workflows/crabwatch.yml`
/// so the CLI and the workflow resolve the same configuration (checked by a test).
const BUILD_CONFIG_EXPR: &str = ".default * (.repositories[strenv(GITHUB_REPOSITORY)] // {})";

const YQ_INSTALL_HINT: &str =
    "If you haven't, install mikefarah's yq v4: https://github.com/mikefarah/yq#install";

/// The bundled zizmor policy, written once to a temporary file that every
/// scan passes to `yq` as a path.
pub(crate) struct ZizmorPolicy {
    file: NamedTempFile,
}

fn temp_file(contents: &str) -> anyhow::Result<NamedTempFile> {
    let mut file = tempfile::Builder::new()
        .prefix("crabwatch-")
        .tempfile()
        .context("failed to create temporary file")?;
    file.write_all(contents.as_bytes())
        .context("failed to write temporary file")?;
    Ok(file)
}

/// Run `yq` on `policy`, exposing `repository` to the expression as
/// `GITHUB_REPOSITORY` like GitHub Actions does.
async fn yq(args: &[&str], policy: &Path, repository: &str) -> anyhow::Result<String> {
    let output = Command::new("yq")
        .args(["--exit-status", "--no-colors"])
        .args(args)
        .arg(policy)
        .env("GITHUB_REPOSITORY", repository)
        .output()
        .await
        .with_context(|| format!("failed to run yq. {YQ_INSTALL_HINT}"))?;
    if !output.status.success() {
        // The unrelated Python `yq` also installs a `yq` binary, so mention the right one.
        bail!(
            "yq failed ({}): {}. {YQ_INSTALL_HINT}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("yq returned invalid UTF-8")
}

impl ZizmorPolicy {
    /// Fail if `yq` is missing or the bundled policy is not valid YAML.
    /// The policy's structure is validated by tests.
    pub(crate) async fn bundled() -> anyhow::Result<Self> {
        Self::from_source(ZIZMOR_POLICY).await
    }

    async fn from_source(source: &str) -> anyhow::Result<Self> {
        let policy = Self {
            file: temp_file(source)?,
        };
        policy.effective_config("").await?;
        Ok(policy)
    }

    async fn effective_config(&self, repository: &str) -> anyhow::Result<String> {
        yq(&[BUILD_CONFIG_EXPR], self.file.path(), repository).await
    }

    /// Write `repository`'s zizmor configuration to a temporary file that is
    /// deleted on drop.
    ///
    /// `repository` is GitHub's canonical `owner/name`; override lookup is
    /// case-sensitive.
    pub(crate) async fn write_config(&self, repository: &str) -> anyhow::Result<NamedTempFile> {
        temp_file(&self.effective_config(repository).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::Value;
    use std::collections::BTreeMap;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Policy {
        default: Config,
        repositories: BTreeMap<String, Config>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Config {
        rules: BTreeMap<String, RuleSettings>,
    }

    /// Mirrors the shape of zizmor's rule settings so a malformed field is caught early.
    /// Unknown rule names are not caught.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct RuleSettings {
        // Non-Option like zizmor, so an explicit `null` is rejected.
        #[serde(default)]
        disable: bool,
        #[serde(default)]
        ignore: Vec<String>,
        config: Option<serde_json::Map<String, Value>>,
        remap: Option<Value>,
    }

    /// Check the policy's structure. This runs only in tests: the bundled policy
    /// is validated in CI by `embedded_policy_is_valid`.
    async fn validate(source: &str) -> anyhow::Result<()> {
        // yq preserves duplicate mapping keys when parsing, so check them explicitly.
        let file = temp_file(source)?;
        let json = yq(
            &[
                // Convert to JSON so that we can use `serde_json` for validation.
                "--output-format=json",
                r#"
                select( # Keep the original document only if the check passes.
                  [
                    # Gather the separate mapping checks into an array so `all`
                    # can decide whether the entire document passes.
                    .. # Visit the document root and its nested values.
                    | select(tag == "!!map") # Inspect mappings only.
                    | keys # Get this mapping's keys, including duplicates.
                    | (length == (unique | length)) # Equal counts mean no duplicates.
                  ]
                  | all # Require every mapping to pass.
                ) // null # Emit null instead of silently dropping a rejected document.
                "#,
            ],
            file.path(),
            "",
        )
        .await?;
        // Policy rejects null, and from_str rejects trailing JSON values. Keeping
        // rejected documents as null prevents multi-document input from hiding them.
        let policy: Policy = serde_json::from_str(&json).context("invalid zizmor policy")?;
        // Check that each override can be merged using the production expression.
        // JSON validation alone misses failures such as an aliased override.
        let resolved = ZizmorPolicy::from_source(source).await?;
        for (repository, overrides) in &policy.repositories {
            crate::command::analyze::parse_repo(repository)
                .with_context(|| format!("repository override {repository}"))?;
            for rule in overrides.rules.keys() {
                if !policy.default.rules.contains_key(rule) {
                    bail!("{repository}: unknown rule {rule}; add it to default.rules first");
                }
            }
            resolved
                .effective_config(repository)
                .await
                .with_context(|| format!("{repository}: failed to resolve configuration"))?;
        }
        Ok(())
    }

    async fn yaml_as_json(source: &str) -> Value {
        let file = temp_file(source).unwrap();
        serde_json::from_str(
            &yq(&["--output-format=json", "."], file.path(), "")
                .await
                .unwrap(),
        )
        .unwrap()
    }

    const TEST_POLICY: &str = "default:
  rules:
    bot-conditions:
      disable: false
      ignore: [example.yml]
    insecure-commands:
      disable: false
      ignore: [default.yml]
    unpinned-uses:
      disable: true
repositories:
  Example/Overridden:
    rules:
      bot-conditions:
        disable: true
      insecure-commands:
        ignore: [strict.yml]
  Example/Stricter:
    rules:
      bot-conditions:
        ignore: []
      unpinned-uses:
        disable: false
";

    #[tokio::test]
    async fn embedded_policy_is_valid() {
        validate(ZIZMOR_POLICY).await.unwrap();
    }

    #[tokio::test]
    async fn selects_override_with_exact_case_and_falls_back() {
        validate(TEST_POLICY).await.unwrap();
        let policy = ZizmorPolicy::from_source(TEST_POLICY).await.unwrap();

        let overridden =
            yaml_as_json(&policy.effective_config("Example/Overridden").await.unwrap()).await;
        assert_eq!(overridden["rules"]["bot-conditions"]["disable"], true);
        assert_eq!(
            overridden["rules"]["bot-conditions"]["ignore"],
            serde_json::json!(["example.yml"])
        );
        assert_eq!(overridden["rules"]["insecure-commands"]["disable"], false);
        assert_eq!(
            overridden["rules"]["insecure-commands"]["ignore"],
            serde_json::json!(["strict.yml"])
        );
        assert_eq!(overridden["rules"]["unpinned-uses"]["disable"], true);

        let stricter =
            yaml_as_json(&policy.effective_config("Example/Stricter").await.unwrap()).await;
        assert_eq!(stricter["rules"]["unpinned-uses"]["disable"], false);
        assert_eq!(stricter["rules"]["bot-conditions"]["disable"], false);
        assert_eq!(
            stricter["rules"]["bot-conditions"]["ignore"],
            serde_json::json!([])
        );

        for repository in ["example/Overridden", "Example/overridden", "Example/other"] {
            let default = yaml_as_json(&policy.effective_config(repository).await.unwrap()).await;
            assert_eq!(default["rules"]["bot-conditions"]["disable"], false);
            assert_eq!(default["rules"]["insecure-commands"]["disable"], false);
            assert_eq!(default["rules"]["unpinned-uses"]["disable"], true);
        }
    }

    #[tokio::test]
    async fn write_config_uses_the_bundled_policy() {
        let policy = ZizmorPolicy::bundled().await.unwrap();
        // Write the config for a repo that doesn't exist.
        let config_file = policy.write_config("example/repo").await.unwrap();
        let effective_config = policy.effective_config("example/repo").await.unwrap();
        // Ensure the file was created, and its contents match the effective config.
        assert_eq!(
            std::fs::read_to_string(config_file.path()).unwrap(),
            effective_config
        );
        // Since the repo doesn't exist, the effective configuration should be the default configuration.
        assert_eq!(
            yaml_as_json(&effective_config).await,
            yaml_as_json(ZIZMOR_POLICY).await["default"]
        );
    }

    #[test]
    fn workflow_uses_the_same_yq_expression() {
        assert!(include_str!("../.github/workflows/crabwatch.yml").contains(BUILD_CONFIG_EXPR));
    }

    #[test]
    fn ci_runs_the_same_zizmor_action_as_the_workflow() {
        let action = |workflow: &'static str| {
            workflow
                .lines()
                .find_map(|line| line.trim().strip_prefix("uses: zizmorcore/zizmor-action@"))
                .expect("no step uses zizmor-action")
        };

        // The zizmor job of ci.yml runs on pull requests with the zizmor-action
        // release and the zizmor version of the Crabwatch workflow, so that it
        // fails before merge if the release doesn't support the version.
        assert_eq!(
            action(include_str!("../.github/workflows/ci.yml")),
            action(include_str!("../.github/workflows/crabwatch.yml"))
        );
    }

    #[tokio::test]
    async fn rejects_invalid_policies() {
        for (reason, source) in [
            ("malformed YAML", "default: ["),
            ("empty", ""),
            ("missing default", "repositories: {}"),
            ("missing repositories", "default: {rules: {}}"),
            ("missing rules", "default: {}\nrepositories: {}"),
            (
                "multiple documents",
                "default: {rules: {}}\nrepositories: {}\n---\ndefault: {rules: {}}\nrepositories: {}",
            ),
            (
                "unknown top-level key",
                "default: {rules: {}}\nrepositories: {}\nextra: {}",
            ),
            (
                "duplicate top-level key",
                "default: {rules: {}}\ndefault: {rules: {}}\nrepositories: {}",
            ),
            (
                "duplicate rule key",
                "default: {rules: {bot-conditions: {}, bot-conditions: {}}}\nrepositories: {}",
            ),
            (
                "duplicate key before valid document",
                "default: {rules: {}}\ndefault: {rules: {}}\nrepositories: {}\n---\ndefault: {rules: {}}\nrepositories: {}",
            ),
            (
                "duplicate key after valid document",
                "default: {rules: {}}\nrepositories: {}\n---\ndefault: {rules: {}}\ndefault: {rules: {}}\nrepositories: {}",
            ),
            (
                "non-boolean default disable",
                "default: {rules: {a: {disable: 'false'}}}\nrepositories: {}",
            ),
            (
                "unknown rule field",
                "default: {rules: {a: {disabled: true}}}\nrepositories: {}",
            ),
            (
                "non-list ignore",
                "default: {rules: {a: {ignore: strict.yml}}}\nrepositories: {}",
            ),
            (
                "repository key without slash",
                "default: {rules: {}}\nrepositories: {example: {rules: {}}}",
            ),
            (
                "unknown rule",
                "default: {rules: {}}\nrepositories: {example/repo: {rules: {unknown: {disable: true}}}}",
            ),
            (
                "override rules not a mapping",
                "default: {rules: {a: {}}}\nrepositories: {example/repo: {rules: []}}",
            ),
            (
                "non-mapping override",
                "default: {rules: {a: {}}}\nrepositories: {example/repo: {rules: {a: true}}}",
            ),
            (
                "quoted override disable",
                "default: {rules: {a: {}}}\nrepositories: {example/repo: {rules: {a: {disable: 'false'}}}}",
            ),
            (
                "numeric override disable",
                "default: {rules: {a: {}}}\nrepositories: {example/repo: {rules: {a: {disable: 0}}}}",
            ),
            (
                "null override disable",
                "default: {rules: {a: {}}}\nrepositories: {example/repo: {rules: {a: {disable: null}}}}",
            ),
            (
                "unknown override key",
                "default: {rules: {a: {}}}\nrepositories: {example/repo: {rules: {}, extra: {}}}",
            ),
            (
                "aliased override",
                "default: {rules: {a: {}}}\nrepositories: {example/one: &shared {rules: {a: {disable: true}}}, example/two: *shared}",
            ),
        ] {
            assert!(
                validate(source).await.is_err(),
                "accepted {reason}: {source:?}"
            );
        }
    }
}
