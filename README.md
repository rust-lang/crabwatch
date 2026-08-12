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
[`zizmor-default.yml`](./zizmor-default.yml) configuration file, maintained by
the Rust Infrastructure team.

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

Analyze every eligible repository in a GitHub organization:

```console
GITHUB_TOKEN=$(gh auth token) cargo run -- analyze --org rust-lang
```

Set `CRABWATCH_LOG=debug` to debug issues.

### Cache

By default, the CLI stores its data under the platform's user cache directory
in a `crabwatch` subdirectory, so that you don't have to clone repositories
twice if you already have the latest commit.

## Docs

* [GitHub: required workflows configured through org-wide rulesets](https://docs.github.com/en/enterprise-cloud@latest/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets#require-workflows-to-pass-before-merging)
* [zizmor documentation](https://docs.zizmor.sh/)
