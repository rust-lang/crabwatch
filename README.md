# crabwatch

Crabwatch audits GitHub Actions workflows across Rust project repositories.

It provides:

* the [`.github/workflows/crabwatch.yml`](./.github/workflows/crabwatch.yml)
  workflow used by the `rust-lang` organization.
* the `crabwatch` CLI for scanning GitHub repositories or organizations.

> [!NOTE]
> This project is intended to be used only by the Rust project.

## Checks

Crabwatch runs [zizmor](https://docs.zizmor.sh/) with the
[`zizmor-policy.yml`](./zizmor-policy.yml) policy, maintained by the Rust
Infrastructure team. Its `default` section contains the shared zizmor
configuration, and `repositories` contains repository-specific overrides.

### Repository overrides

Add an `owner/repository` entry under `repositories` to change a rule's settings
for that repository, for example to disable a rule or to opt in to a rule that is
disabled by default.
Overrides are deep-merged into the default, so unspecified settings are
inherited, but a list such as `ignore` replaces the default's list rather than
extending it.
CI validates the policy's structure and that every repository key is GitHub's
canonical `owner/repository` name, since the lookup is case-sensitive.

Example (hypothetical):

```yaml
repositories:
  rust-lang/rust-clippy:
    rules:
      bot-conditions:
        # Accepted exception for this repository's automation pattern.
        disable: true
  rust-lang/rust:
    rules:
      bot-conditions:
        disable: true
      overprovisioned-secrets:
        # Temporary exception while its workflows are migrated.
        disable: true
```

## Design principles

* Repositories should never experience CI failures due to new versions of crabwatch or zizmor.
  Before introducing new mandatory lints, the Infrastructure team will raise PRs to fix them.
* Checks are managed centrally here, so other repositories only need their own zizmor setup
  if they want stricter checks.

## How the GitHub Action works

A [ruleset](https://github.com/organizations/rust-lang/settings/rules) in the
[`rust-lang`](https://github.com/rust-lang) GitHub organization is
configured to run the file
[`.github/workflows/crabwatch.yml`](./.github/workflows/crabwatch.yml).

The workflow runs for pull requests and merge queue checks in repositories that
set `crabwatch = true` in the
[`[custom-properties]`](https://github.com/rust-lang/team/blob/main/docs/toml-schema.md#repository-custom-properties)
section of their [`team`](https://github.com/rust-lang/team/tree/main/repos)
repository definition.

The workflow does not run the Crabwatch CLI. The CLI is a separate tool for
manually auditing repositories with the same configuration.

## CLI usage

The CLI determines the repository's configuration in the same way as the workflow,
so you need to install [mikefarah's `yq` v4](https://github.com/mikefarah/yq#install).

Analyze every eligible repository in a GitHub organization:

```console
GITHUB_TOKEN=$(gh auth token) cargo run -- analyze --org rust-lang
```

The command fails if zizmor reports findings in any repository.
The merge queue of this repository runs it to make sure that changes don't make
the Crabwatch workflow fail in any `rust-lang` repository.

If the output ends up in public logs, such as CI, pass `--redact-private`.
It hides the names, zizmor output and errors of private repositories and only
counts them. Debug logs can still show their names.

Set `CRABWATCH_LOG=debug` to debug issues.

### Cache

By default, the CLI stores its data under the platform's user cache directory
in a `crabwatch` subdirectory, so that you don't have to clone repositories
twice if you already have the latest commit.

## Docs

* [GitHub: required workflows configured through org-wide rulesets](https://docs.github.com/en/enterprise-cloud@latest/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets#require-workflows-to-pass-before-merging)
* [zizmor documentation](https://docs.zizmor.sh/)

## How to help

Over time, we'll turn on more lints by default for everyone.
Until then, it would help us a lot if you fix zizmor findings in your repository and opt in to more checks 🙏

To check what zizmor lints fail for your repository:

1. [Install zizmor](https://docs.zizmor.sh/installation/).
2. Run `zizmor .` in the root directory of your repository.
   Or `zizmor . --persona=pedantic` if you already pass the basic checks.
3. Open PRs in your repository to fix those.
4. Once fixed, open a PR in this repository to enable the lints you solved, so that
   if they are re-introduced in the future, crabwatch will catch them. Example
   [here](https://github.com/rust-lang/crabwatch/blob/4630d36d74262fbcbed9c42861737343db167eed/zizmor-policy.yml#L178).

Pull requests to fix zizmor findings are tracked in the
[t-crabwatch policy expansion GitHub Project](https://github.com/orgs/rust-lang/projects/81).

## About the team

The crabwatch team is available in the
[t-infra/crabwatch](https://rust-lang.zulipchat.com/#narrow/channel/650096-t-infra.2Fcrabwatch)
Zulip channel.
