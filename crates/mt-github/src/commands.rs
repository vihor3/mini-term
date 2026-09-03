use crate::{GitHubRepoIdentity, WorkItemKind};

pub const COMMAND_OUTPUT_LIMIT: usize = 64 * 1024;
pub const LIST_OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
pub const DETAIL_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
pub const LIST_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPlan {
    pub program: String,
    pub args: Vec<String>,
}

impl CommandPlan {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    pub fn display_argv(&self) -> Vec<&str> {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect()
    }
}

pub fn discover_remote_plan() -> CommandPlan {
    CommandPlan::new("git", ["remote", "get-url", "origin"])
}

pub fn version_plan() -> CommandPlan {
    CommandPlan::new("gh", ["--version"])
}

pub fn auth_status_plan(host: &str) -> CommandPlan {
    CommandPlan::new("gh", ["auth", "status", "--active", "--hostname", host])
}

pub fn account_plan(host: &str) -> CommandPlan {
    CommandPlan::new(
        "gh",
        ["api", "--hostname", host, "user", "--jq", "{login: .login}"],
    )
}

pub fn list_plan(repo: &GitHubRepoIdentity, kind: WorkItemKind) -> CommandPlan {
    let command = match kind {
        WorkItemKind::Issue => "issue",
        WorkItemKind::PullRequest => "pr",
    };
    let fields = match kind {
        WorkItemKind::Issue => "number,title,state,author,labels,updatedAt,url",
        WorkItemKind::PullRequest => "number,title,state,author,labels,updatedAt,url,isDraft",
    };
    CommandPlan::new(
        "gh",
        [
            command,
            "list",
            "--repo",
            repo.cli_spec().as_str(),
            "--state",
            "all",
            "--limit",
            LIST_LIMIT.to_string().as_str(),
            "--json",
            fields,
        ],
    )
}

pub fn detail_plan(repo: &GitHubRepoIdentity, kind: WorkItemKind, number: u64) -> CommandPlan {
    let command = match kind {
        WorkItemKind::Issue => "issue",
        WorkItemKind::PullRequest => "pr",
    };
    let fields = match kind {
        WorkItemKind::Issue => "number,title,state,author,labels,updatedAt,url,body",
        WorkItemKind::PullRequest => "number,title,state,author,labels,updatedAt,url,body,isDraft",
    };
    CommandPlan::new(
        "gh",
        [
            command,
            "view",
            number.to_string().as_str(),
            "--repo",
            repo.cli_spec().as_str(),
            "--json",
            fields,
        ],
    )
}

pub fn auth_login_command(host: &str) -> String {
    format!("gh auth login --hostname {host}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> GitHubRepoIdentity {
        GitHubRepoIdentity::new("github.example.com", "Owner", "Repo").unwrap()
    }

    #[test]
    fn plans_use_structured_argv_and_explicit_repository() {
        let list = list_plan(&repo(), WorkItemKind::PullRequest);
        assert_eq!(list.program, "gh");
        assert_eq!(list.args[0..2], ["pr", "list"]);
        assert!(
            list.args
                .windows(2)
                .any(|pair| { pair == ["--repo", "github.example.com/owner/repo"] })
        );
        assert!(list.args.windows(2).any(|pair| pair == ["--state", "all"]));
        assert!(!list.args.iter().any(|arg| arg == "--web"));

        let detail = detail_plan(&repo(), WorkItemKind::Issue, 42);
        assert_eq!(detail.args[0..3], ["issue", "view", "42"]);
        assert!(!detail.args.iter().any(|arg| arg == "--web"));
    }

    #[test]
    fn auth_probe_never_requests_or_prints_a_token() {
        let status = auth_status_plan("github.example.com");
        assert!(status.args.iter().any(|arg| arg == "--active"));
        assert!(!status.args.iter().any(|arg| arg == "--show-token"));
        assert_eq!(
            auth_login_command("github.example.com"),
            "gh auth login --hostname github.example.com"
        );
    }
}
