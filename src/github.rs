use anyhow::{Context, anyhow};
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

    response
        .data
        .and_then(|d| d.repository)
        .and_then(|r| r.default_branch_ref)
        .map(|b| b.target.oid)
        .ok_or_else(|| anyhow!("repository {org}/{repo} not found or has no default branch"))
}

#[derive(Deserialize)]
struct GraphQlResponse {
    data: Option<GraphQlData>,
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
    fn head_commit_query_includes_org_and_repo() {
        let body = head_commit_query("rust-lang", "crabwatch");
        assert!(body.contains("\"query\""));
        assert!(body.contains("rust-lang"));
        assert!(body.contains("crabwatch"));
        assert!(body.contains("defaultBranchRef"));
        assert!(body.contains("oid"));
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
        let sha = parsed
            .data
            .and_then(|d| d.repository)
            .and_then(|r| r.default_branch_ref)
            .map(|b| b.target.oid);
        assert_eq!(sha, Some("abc123".to_string()));
    }

    #[test]
    fn parses_missing_repository_as_none() {
        let json = r#"{ "data": { "repository": null } }"#;
        let parsed: GraphQlResponse = serde_json::from_str(json).unwrap();
        let sha = parsed
            .data
            .and_then(|d| d.repository)
            .and_then(|r| r.default_branch_ref)
            .map(|b| b.target.oid);
        assert_eq!(sha, None);
    }
}
