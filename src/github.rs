use crate::security_fork;
use anyhow::{Context, anyhow, bail};
use serde::Deserialize;

pub fn head_commit_query(org: &str, repo: &str) -> String {
    let inner = format!(
        "query {{ repository(owner: \"{org}\", name: \"{repo}\") {{ defaultBranchRef {{ target {{ oid }} }} }} }}"
    );
    serde_json::json!({ "query": inner }).to_string()
}

pub async fn fetch_head_commit(
    client: &reqwest::Client,
    org: &str,
    repo: &str,
    token: &str,
) -> anyhow::Result<String> {
    let body = head_commit_query(org, repo);
    let response: GraphQlResponse = post_graphql(client, token, body).await?;
    check_graphql_errors(&response.errors)?;

    response
        .head_commit_sha()
        .ok_or_else(|| anyhow!("repository {org}/{repo} not found or has no default branch"))
}

async fn post_graphql<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    token: &str,
    body: String,
) -> anyhow::Result<T> {
    let response = client
        .post("https://api.github.com/graphql")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "crabwatch")
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .context("failed to send GraphQL request to GitHub")?
        .error_for_status()
        .context("GitHub returned an error status")?
        .json()
        .await
        .context("failed to parse GraphQL response from GitHub")?;
    Ok(response)
}

fn check_graphql_errors(errors: &Option<Vec<serde_json::Value>>) -> anyhow::Result<()> {
    if let Some(errors) = errors {
        bail!(
            "GitHub GraphQL API returned errors: {}",
            serde_json::to_string(errors).unwrap_or_default()
        );
    }
    Ok(())
}

#[derive(Deserialize)]
struct GraphQlResponse {
    data: Option<GraphQlData>,
    errors: Option<Vec<serde_json::Value>>,
}

impl GraphQlResponse {
    fn head_commit_sha(self) -> Option<String> {
        self.data
            .and_then(|d| d.repository)
            .and_then(|r| r.default_branch_ref)
            .map(|b| b.target.oid)
    }
}

#[derive(Deserialize)]
struct GraphQlData {
    repository: Option<Repository>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Repository {
    default_branch_ref: Option<BranchRef>,
}

#[derive(Deserialize)]
struct BranchRef {
    target: Target,
}

#[derive(Deserialize)]
struct Target {
    oid: String,
}

pub fn list_repos_query(org: &str, cursor: Option<&str>) -> String {
    let query = "query($org: String!, $cursor: String) { \
        organization(login: $org) { \
            repositories(first: 100, after: $cursor, isFork: false, isArchived: false) { \
                nodes { \
                    name \
                    isPrivate \
                    workflows: object(expression: \"HEAD:.github/workflows\") { __typename } \
                } \
                pageInfo { hasNextPage endCursor } \
            } \
        } \
    }";
    serde_json::json!({
        "query": query,
        "variables": { "org": org, "cursor": cursor }
    })
    .to_string()
}

pub async fn list_org_repos(
    client: &reqwest::Client,
    org: &str,
    token: &str,
) -> anyhow::Result<Vec<String>> {
    let mut repos = Vec::new();
    let mut cursor: Option<String> = None;

    loop {
        let body = list_repos_query(org, cursor.as_deref());
        let response: OrgReposResponse = post_graphql(client, token, body).await?;
        check_graphql_errors(&response.errors)?;

        let connection = response
            .data
            .and_then(|d| d.organization)
            .map(|o| o.repositories)
            .ok_or_else(|| anyhow!("organization {org} not found"))?;

        let repositories = connection.nodes.into_iter().filter_map(|node| {
            (node.has_workflows_directory()
                && !security_fork::is_security_advisory_fork(&node.name, node.is_private))
            .then_some(node.name)
        });

        repos.extend(repositories);

        if connection.page_info.has_next_page {
            cursor = connection.page_info.end_cursor;
        } else {
            break;
        }
    }

    Ok(repos)
}

#[derive(Deserialize)]
struct OrgReposResponse {
    data: Option<OrgReposData>,
    errors: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct OrgReposData {
    organization: Option<OrgRepositories>,
}

#[derive(Deserialize)]
struct OrgRepositories {
    repositories: RepositoryConnection,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryConnection {
    nodes: Vec<RepoNode>,
    page_info: PageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepoNode {
    name: String,
    is_private: bool,
    workflows: Option<GitObject>,
}

impl RepoNode {
    fn has_workflows_directory(&self) -> bool {
        self.workflows
            .as_ref()
            .is_some_and(|object| object.type_name == "Tree")
    }
}

#[derive(Deserialize)]
struct GitObject {
    #[serde(rename = "__typename")]
    type_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_commit_query_snapshot() {
        insta::assert_snapshot!(head_commit_query("rust-lang", "crabwatch"), @r#"{"query":"query { repository(owner: \"rust-lang\", name: \"crabwatch\") { defaultBranchRef { target { oid } } } }"}"#);
    }

    #[test]
    fn list_repos_query_snapshot() {
        insta::assert_snapshot!(list_repos_query("rust-lang", Some("cursor")), @r#"{"query":"query($org: String!, $cursor: String) { organization(login: $org) { repositories(first: 100, after: $cursor, isFork: false, isArchived: false) { nodes { name isPrivate workflows: object(expression: \"HEAD:.github/workflows\") { __typename } } pageInfo { hasNextPage endCursor } } } }","variables":{"cursor":"cursor","org":"rust-lang"}}"#);
    }

    #[test]
    fn identifies_repository_with_workflows_directory() {
        let json = r#"{
            "name": "crabwatch",
            "isPrivate": false,
            "workflows": {
                "__typename": "Tree"
            }
        }"#;
        let node: RepoNode = serde_json::from_str(json).unwrap();
        assert!(node.has_workflows_directory());
    }

    #[test]
    fn rejects_repository_without_workflows_directory() {
        let json = r#"{
            "name": "no-workflows",
            "isPrivate": false,
            "workflows": null
        }"#;
        let node: RepoNode = serde_json::from_str(json).unwrap();
        assert!(!node.has_workflows_directory());
    }

    #[test]
    fn rejects_non_directory_at_workflows_path() {
        let json = r#"{
            "name": "workflows-is-a-file",
            "isPrivate": false,
            "workflows": {
                "__typename": "Blob"
            }
        }"#;
        let node: RepoNode = serde_json::from_str(json).unwrap();
        assert!(!node.has_workflows_directory());
    }

    #[test]
    fn parses_successful_response() {
        let json = r#"{
            "data": {
                "repository": {
                    "defaultBranchRef": {
                        "target": {
                            "oid": "abc123"
                        }
                    }
                }
            }
        }"#;
        let parsed: GraphQlResponse = serde_json::from_str(json).unwrap();
        let sha = parsed.head_commit_sha();
        assert_eq!(sha, Some("abc123".to_string()));
    }

    #[test]
    fn parses_missing_repository_as_none() {
        let json = r#"{ "data": { "repository": null } }"#;
        let parsed: GraphQlResponse = serde_json::from_str(json).unwrap();
        let sha = parsed.head_commit_sha();
        assert_eq!(sha, None);
    }

    #[test]
    fn parses_errors_field() {
        let json = r#"{
            "data": null,
            "errors": [
                {
                    "message": "Could not resolve to a Repository with the name 'rust-lang/does-not-exist'.",
                    "type": "NOT_FOUND"
                }
            ]
        }"#;
        let parsed: GraphQlResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.errors.is_some());
    }
}
