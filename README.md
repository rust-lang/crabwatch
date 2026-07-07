# crabwatch

Analyze Rust project repositories CI and best practices

## How it works

A [ruleset](https://github.com/organizations/rust-lang/settings/rules) defined
in the [`rust-lang`](https://github.com/rust-lang) GitHub organization is
configured to run the file
[`.github/workflows/crabwatch.yml`](https://github.com/rust-lang/crabwatch/blob/main/.github/workflows/crabwatch.yml)
of this repository.

This workflow runs on all the repositories that set `crabwatch = true` in the
[`[custom-properties]`](https://github.com/rust-lang/team/blob/main/docs/toml-schema.md#repository-custom-properties)
of the [`team`](https://github.com/rust-lang/team/tree/main/repos) toml file.
