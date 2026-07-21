# crabwatch

Analyze Rust project repositories CI and best practices.

> [!NOTE]
> This project is only intended to be used by the Rust project.

## Design

* Repositories should never experience CI failures due to new versions of crabwatch.
  Before introducing new mandatory lints, the infra team will raise PRs to fix them.

## How it works

A [ruleset](https://github.com/organizations/rust-lang/settings/rules) defined
in the [`rust-lang`](https://github.com/rust-lang) GitHub organization is
configured to run the file
[`.github/workflows/crabwatch.yml`](https://github.com/rust-lang/crabwatch/blob/main/.github/workflows/crabwatch.yml)
of this repository.

This workflow runs on all the repositories that set `crabwatch = true` in the
[`[custom-properties]`](https://github.com/rust-lang/team/blob/main/docs/toml-schema.md#repository-custom-properties)
of the [`team`](https://github.com/rust-lang/team/tree/main/repos) toml file.

## Docs

* [GitHub: required Workflows configured through org-wide rulesets](https://docs.github.com/en/enterprise-cloud@latest/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets#require-workflows-to-pass-before-merging)
