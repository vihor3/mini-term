use serde::Deserialize;

use crate::{CommandOutput, GitHubError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkItemKind {
    Issue,
    PullRequest,
}

impl WorkItemKind {
    pub const ALL: [Self; 2] = [Self::Issue, Self::PullRequest];

    pub fn label(self) -> &'static str {
        match self {
            Self::Issue => "Issues",
            Self::PullRequest => "Pull requests",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Issue => "Issues",
            Self::PullRequest => "PRs",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkItemStateFilter {
    #[default]
    Open,
    Closed,
    All,
}

impl WorkItemStateFilter {
    pub const ALL: [Self; 3] = [Self::Open, Self::Closed, Self::All];

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Closed => "Closed",
            Self::All => "All",
        }
    }

    pub fn matches(self, item: &GitHubWorkItemSummary) -> bool {
        match self {
            Self::Open => item.state == WorkItemState::Open,
            Self::Closed => item.state != WorkItemState::Open,
            Self::All => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkItemState {
    Open,
    Closed,
    Merged,
}

impl WorkItemState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Closed => "Closed",
            Self::Merged => "Merged",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubWorkItemSummary {
    pub kind: WorkItemKind,
    pub number: u64,
    pub title: String,
    pub state: WorkItemState,
    pub is_draft: bool,
    pub author: Option<String>,
    pub labels: Vec<String>,
    pub updated_at: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubWorkItemDetail {
    pub summary: GitHubWorkItemSummary,
    pub body: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWorkItem {
    number: u64,
    title: String,
    state: String,
    #[serde(default)]
    is_draft: bool,
    author: Option<RawAuthor>,
    #[serde(default)]
    labels: Vec<RawLabel>,
    updated_at: String,
    url: String,
    body: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAuthor {
    login: String,
}

#[derive(Debug, Deserialize)]
struct RawLabel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawAccount {
    login: String,
}

pub fn parse_work_item_list(
    kind: WorkItemKind,
    output: &CommandOutput,
) -> Result<Vec<GitHubWorkItemSummary>, GitHubError> {
    let text = bounded_utf8(output, "GitHub work-item list")?;
    let raw: Vec<RawWorkItem> =
        serde_json::from_str(text).map_err(|_| GitHubError::malformed("GitHub work-item list"))?;
    if raw.len() > 1_000 {
        return Err(GitHubError::malformed("GitHub work-item list"));
    }
    raw.into_iter()
        .map(|item| normalize_item(kind, item))
        .collect()
}

pub fn parse_work_item_detail(
    kind: WorkItemKind,
    output: &CommandOutput,
) -> Result<GitHubWorkItemDetail, GitHubError> {
    let text = bounded_utf8(output, "GitHub work-item detail")?;
    let raw: RawWorkItem = serde_json::from_str(text)
        .map_err(|_| GitHubError::malformed("GitHub work-item detail"))?;
    let body = raw.body.clone().unwrap_or_default();
    Ok(GitHubWorkItemDetail {
        summary: normalize_item(kind, raw)?,
        body,
    })
}

pub fn parse_account(output: &CommandOutput) -> Result<String, GitHubError> {
    let text = bounded_utf8(output, "GitHub account probe")?;
    let raw: RawAccount =
        serde_json::from_str(text).map_err(|_| GitHubError::malformed("GitHub account probe"))?;
    validate_text(&raw.login, "GitHub account probe")?;
    Ok(raw.login)
}

fn normalize_item(
    kind: WorkItemKind,
    raw: RawWorkItem,
) -> Result<GitHubWorkItemSummary, GitHubError> {
    if raw.number == 0 {
        return Err(GitHubError::malformed("GitHub work item"));
    }
    validate_text(&raw.title, "GitHub work item")?;
    validate_text(&raw.updated_at, "GitHub work item")?;
    validate_text(&raw.url, "GitHub work item")?;
    let state = match raw.state.to_ascii_uppercase().as_str() {
        "OPEN" => WorkItemState::Open,
        "CLOSED" => WorkItemState::Closed,
        "MERGED" if kind == WorkItemKind::PullRequest => WorkItemState::Merged,
        _ => return Err(GitHubError::malformed("GitHub work item")),
    };
    let author = raw
        .author
        .map(|author| author.login)
        .filter(|login| !login.is_empty());
    let labels = raw
        .labels
        .into_iter()
        .map(|label| label.name)
        .filter(|name| !name.is_empty())
        .collect();
    Ok(GitHubWorkItemSummary {
        kind,
        number: raw.number,
        title: raw.title,
        state,
        is_draft: raw.is_draft,
        author,
        labels,
        updated_at: raw.updated_at,
        url: raw.url,
    })
}

fn validate_text(value: &str, label: &str) -> Result<(), GitHubError> {
    if value.contains('\0') {
        Err(GitHubError::malformed(label))
    } else {
        Ok(())
    }
}

fn bounded_utf8<'a>(output: &'a CommandOutput, label: &str) -> Result<&'a str, GitHubError> {
    if output.stdout_truncated || output.stderr_truncated {
        return Err(GitHubError::malformed(label));
    }
    std::str::from_utf8(&output.stdout).map_err(|_| GitHubError::malformed(label))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(json: &str) -> CommandOutput {
        CommandOutput {
            stdout: json.as_bytes().to_vec(),
            exit_code: Some(0),
            ..CommandOutput::default()
        }
    }

    #[test]
    fn parses_issue_and_pull_request_json_without_trusting_text() {
        let issues = parse_work_item_list(
            WorkItemKind::Issue,
            &output(
                r#"[{"number":7,"title":"<img src=x onerror=alert(1)>","state":"OPEN","author":{"login":"octo"},"labels":[{"name":"bug"}],"updatedAt":"2026-09-03T01:02:03Z","url":"javascript:alert(1)"}]"#,
            ),
        )
        .unwrap();
        assert_eq!(issues[0].title, "<img src=x onerror=alert(1)>");
        assert_eq!(issues[0].url, "javascript:alert(1)");

        let pr = parse_work_item_detail(
            WorkItemKind::PullRequest,
            &output(
                r#"{"number":9,"title":"PR","state":"MERGED","isDraft":false,"author":null,"labels":[],"updatedAt":"2026-09-03T01:02:03Z","url":"https://github.example/pr/9","body":"<script>alert(1)</script>"}"#,
            ),
        )
        .unwrap();
        assert_eq!(pr.summary.state, WorkItemState::Merged);
        assert_eq!(pr.body, "<script>alert(1)</script>");
    }

    #[test]
    fn malformed_json_unknown_states_and_truncation_fail_closed() {
        assert!(parse_work_item_list(WorkItemKind::Issue, &output("not-json")).is_err());
        assert!(
            parse_work_item_list(
                WorkItemKind::Issue,
                &output(
                    r#"[{"number":1,"title":"x","state":"MERGED","author":null,"labels":[],"updatedAt":"x","url":"x"}]"#,
                ),
            )
            .is_err()
        );
        let mut truncated = output("[]");
        truncated.stdout_truncated = true;
        assert!(parse_work_item_list(WorkItemKind::Issue, &truncated).is_err());
    }

    #[test]
    fn account_probe_requires_structured_json() {
        assert_eq!(
            parse_account(&output(r#"{"login":"octocat"}"#)).unwrap(),
            "octocat"
        );
        assert!(parse_account(&output("octocat")).is_err());
    }
}
