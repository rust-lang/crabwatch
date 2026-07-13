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

    let response: GraphQlResponse = client
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

    if let Some(errors) = &response.errors {
        bail!("GitHub GraphQL API returned errors: {errors}");
    }

    response
        .head_commit_sha()
        .ok_or_else(|| anyhow!("repository {org}/{repo} not found or has no default branch"))
}

#[derive(Deserialize)]
struct GraphQlResponse {
    data: Option<GraphQlData>,
    errors: Option<serde_json::Value>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_commit_query_snapshot() {
        insta::assert_snapshot!(head_commit_query("rust-lang", "crabwatch"), @r#"{"query":"query { repository(owner: \"rust-lang\", name: \"crabwatch\") { defaultBranchRef { target { oid } } } }"}"#);
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
