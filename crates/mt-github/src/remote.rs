use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GitHubRepoIdentity {
    host: String,
    owner: String,
    repo: String,
}

impl GitHubRepoIdentity {
    pub fn new(host: &str, owner: &str, repo: &str) -> Result<Self, RemoteParseError> {
        let host = normalize_host(host)?;
        let owner = normalize_component(owner, "owner")?;
        let repo = normalize_component(repo.trim_end_matches(".git"), "repository")?;
        Ok(Self { host, owner, repo })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn repo(&self) -> &str {
        &self.repo
    }

    pub fn cli_spec(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repo)
    }

    pub fn display_name(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteParseError {
    message: &'static str,
}

impl RemoteParseError {
    fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for RemoteParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for RemoteParseError {}

pub fn parse_remote_url(input: &str) -> Result<GitHubRepoIdentity, RemoteParseError> {
    let input = input.trim();
    if input.is_empty()
        || input
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
        || input.contains(['?', '#'])
    {
        return Err(RemoteParseError::new("remote URL is malformed"));
    }

    let (host, path) = if let Some(scheme_end) = input.find("://") {
        let scheme = input[..scheme_end].to_ascii_lowercase();
        if !matches!(scheme.as_str(), "https" | "http" | "ssh" | "git") {
            return Err(RemoteParseError::new("remote URL scheme is unsupported"));
        }
        let rest = &input[scheme_end + 3..];
        let slash = rest
            .find('/')
            .ok_or_else(|| RemoteParseError::new("remote URL has no repository path"))?;
        let authority = &rest[..slash];
        let host = authority.rsplit('@').next().unwrap_or(authority);
        if host.contains(':') {
            return Err(RemoteParseError::new(
                "remote URL ports cannot identify a GitHub host",
            ));
        }
        (host, &rest[slash + 1..])
    } else {
        let colon = input
            .find(':')
            .ok_or_else(|| RemoteParseError::new("remote is not a network repository"))?;
        let authority = &input[..colon];
        if authority.contains('/') || authority.contains('\\') {
            return Err(RemoteParseError::new("remote is not a network repository"));
        }
        let host = authority.rsplit('@').next().unwrap_or(authority);
        (host, &input[colon + 1..])
    };

    let path = path.trim_matches('/');
    let mut components = path.split('/');
    let owner = components
        .next()
        .ok_or_else(|| RemoteParseError::new("remote URL has no owner"))?;
    let repo = components
        .next()
        .ok_or_else(|| RemoteParseError::new("remote URL has no repository"))?;
    if components.next().is_some() {
        return Err(RemoteParseError::new(
            "remote URL must identify one owner and repository",
        ));
    }
    GitHubRepoIdentity::new(host, owner, repo)
}

fn normalize_host(value: &str) -> Result<String, RemoteParseError> {
    let value = value.trim().trim_end_matches('.').to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 253
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(RemoteParseError::new("GitHub host is malformed"));
    }
    Ok(value)
}

fn normalize_component(value: &str, label: &'static str) -> Result<String, RemoteParseError> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 255
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(RemoteParseError::new(match label {
            "owner" => "GitHub owner is malformed",
            _ => "GitHub repository is malformed",
        }));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_ssh_scp_and_enterprise_remotes() {
        for remote in [
            "https://github.com/Owner/Repo.git",
            "ssh://git@github.com/Owner/Repo.git",
            "git@github.com:Owner/Repo.git",
            "git://github.example.com/Owner/Repo.git",
        ] {
            let parsed = parse_remote_url(remote).unwrap();
            assert_eq!(parsed.owner(), "owner");
            assert_eq!(parsed.repo(), "repo");
        }
        assert_eq!(
            parse_remote_url("git@github.example.com:Acme/Widget.git")
                .unwrap()
                .cli_spec(),
            "github.example.com/acme/widget"
        );
    }

    #[test]
    fn rejects_local_malformed_and_hostile_remotes() {
        for remote in [
            "../repo",
            "/srv/repo",
            "file:///srv/repo",
            "https://github.com/owner",
            "https://github.com/owner/repo/extra",
            "https://github.com/owner/repo.git?token=secret",
            "git@github.com:owner/repo.git\n--web",
            "git@github.com:owner/repo;touch-pwned",
            "ssh://git@github.com:2222/owner/repo.git",
        ] {
            assert!(parse_remote_url(remote).is_err(), "{remote}");
        }
    }

    #[test]
    fn identity_normalizes_case_and_git_suffix() {
        assert_eq!(
            GitHubRepoIdentity::new("GitHub.COM.", "Owner", "Repo.git")
                .unwrap()
                .cli_spec(),
            "github.com/owner/repo"
        );
    }
}
