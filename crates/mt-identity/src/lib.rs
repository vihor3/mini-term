//! Stable, opaque identities shared across mini-term persistence and routing.
//!
//! Random identities use canonical UUID v4 payloads. Derived identities use
//! SHA-256 with a versioned domain and length-prefixed UTF-8 components. Each
//! length and the component count are unsigned 64-bit big-endian integers;
//! golden tests freeze that framing as a persistence contract. The serialized
//! string is the complete public representation, so callers do not duplicate
//! hashing or prefix validation.

use std::borrow::Borrow;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseIdentityError {
    identity: &'static str,
    expected: &'static str,
}

impl ParseIdentityError {
    fn new(identity: &'static str, expected: &'static str) -> Self {
        Self { identity, expected }
    }
}

impl fmt::Display for ParseIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid {}: expected canonical {}",
            self.identity, self.expected
        )
    }
}

impl std::error::Error for ParseIdentityError {}

fn is_canonical_uuid_v4(value: &str, prefix: &str) -> bool {
    let Some(payload) = value.strip_prefix(prefix) else {
        return false;
    };
    let Ok(uuid) = Uuid::parse_str(payload) else {
        return false;
    };
    uuid.hyphenated().to_string() == payload
        && payload.as_bytes().get(14) == Some(&b'4')
        && matches!(
            payload.as_bytes().get(19).copied(),
            Some(b'8' | b'9' | b'a' | b'b')
        )
}

fn is_canonical_digest(value: &str, prefix: &str) -> bool {
    let Some(payload) = value.strip_prefix(prefix) else {
        return false;
    };
    payload.len() == 64
        && payload
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn random_value(prefix: &str) -> String {
    format!("{prefix}{}", Uuid::new_v4().hyphenated())
}

fn derived_value(prefix: &str, domain: &str, components: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hash_component(&mut hasher, domain.as_bytes());
    hasher.update((components.len() as u64).to_be_bytes());
    for component in components {
        hash_component(&mut hasher, component.as_bytes());
    }

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = hasher.finalize();
    let mut value = String::with_capacity(prefix.len() + digest.len() * 2);
    value.push_str(prefix);
    for byte in digest.iter().copied() {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}

fn hash_component(hasher: &mut Sha256, component: &[u8]) {
    hasher.update((component.len() as u64).to_be_bytes());
    hasher.update(component);
}

macro_rules! define_identity {
    ($name:ident, $prefix:literal, $expected:literal, $validator:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl FromStr for $name {
            type Err = ParseIdentityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                if $validator(value, Self::PREFIX) {
                    Ok(Self(value.to_owned()))
                } else {
                    Err(ParseIdentityError::new(stringify!($name), $expected))
                }
            }
        }

        impl TryFrom<String> for $name {
            type Error = ParseIdentityError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                if $validator(&value, Self::PREFIX) {
                    Ok(Self(value))
                } else {
                    Err(ParseIdentityError::new(stringify!($name), $expected))
                }
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ParseIdentityError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                value.parse()
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::try_from(String::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

macro_rules! impl_random_identity {
    ($name:ident) => {
        impl $name {
            pub fn new() -> Self {
                Self(random_value(Self::PREFIX))
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

define_identity!(
    HostInstallId,
    "install-v1:",
    "install-v1:<uuid-v4>",
    is_canonical_uuid_v4
);
define_identity!(
    ExecutionHostId,
    "host-v1:",
    "host-v1:<sha256>",
    is_canonical_digest
);
define_identity!(RepoId, "repo-v1:", "repo-v1:<sha256>", is_canonical_digest);
define_identity!(
    WorktreeId,
    "worktree-v1:",
    "worktree-v1:<sha256>",
    is_canonical_digest
);
define_identity!(TabId, "tab-v1:", "tab-v1:<uuid-v4>", is_canonical_uuid_v4);
define_identity!(
    PaneKey,
    "pane-v1:",
    "pane-v1:<uuid-v4>",
    is_canonical_uuid_v4
);
define_identity!(
    TerminalSessionId,
    "terminal-v1:",
    "terminal-v1:<uuid-v4>",
    is_canonical_uuid_v4
);
define_identity!(
    TerminalIncarnationId,
    "incarnation-v1:",
    "incarnation-v1:<uuid-v4>",
    is_canonical_uuid_v4
);
define_identity!(
    AgentRunId,
    "agent-run-v1:",
    "agent-run-v1:<uuid-v4>",
    is_canonical_uuid_v4
);
define_identity!(
    AgentEventId,
    "agent-event-v1:",
    "agent-event-v1:<uuid-v4>",
    is_canonical_uuid_v4
);

impl_random_identity!(HostInstallId);
impl_random_identity!(TabId);
impl_random_identity!(PaneKey);
impl_random_identity!(TerminalSessionId);
impl_random_identity!(TerminalIncarnationId);
impl_random_identity!(AgentRunId);
impl_random_identity!(AgentEventId);

impl ExecutionHostId {
    pub fn derive(host_fingerprint: &str, install: &HostInstallId) -> Self {
        Self(derived_value(
            Self::PREFIX,
            "execution-host/v1",
            &[host_fingerprint, install.as_str()],
        ))
    }
}

impl RepoId {
    pub fn derive(host: &ExecutionHostId, canonical_repo: &str) -> Self {
        Self(derived_value(
            Self::PREFIX,
            "repo/v1",
            &[host.as_str(), canonical_repo],
        ))
    }
}

impl WorktreeId {
    pub fn derive(
        repo: &RepoId,
        canonical_worktree: &str,
        workspace_instance: Option<&str>,
    ) -> Self {
        let value = match workspace_instance {
            Some(instance) => derived_value(
                Self::PREFIX,
                "worktree/v1",
                &[repo.as_str(), canonical_worktree, instance],
            ),
            None => derived_value(
                Self::PREFIX,
                "worktree/v1",
                &[repo.as_str(), canonical_worktree],
            ),
        };
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn fixed_install() -> HostInstallId {
        "install-v1:123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap()
    }

    #[test]
    fn random_ids_are_canonical_uuid_v4_values() {
        let install = HostInstallId::new();
        let tab = TabId::new();
        let pane = PaneKey::new();
        let session = TerminalSessionId::new();
        let incarnation = TerminalIncarnationId::new();
        let agent_run = AgentRunId::new();
        let agent_event = AgentEventId::new();

        assert!(is_canonical_uuid_v4(
            install.as_str(),
            HostInstallId::PREFIX
        ));
        assert!(is_canonical_uuid_v4(tab.as_str(), TabId::PREFIX));
        assert!(is_canonical_uuid_v4(pane.as_str(), PaneKey::PREFIX));
        assert!(is_canonical_uuid_v4(
            session.as_str(),
            TerminalSessionId::PREFIX
        ));
        assert!(is_canonical_uuid_v4(
            incarnation.as_str(),
            TerminalIncarnationId::PREFIX
        ));
        assert!(is_canonical_uuid_v4(agent_run.as_str(), AgentRunId::PREFIX));
        assert!(is_canonical_uuid_v4(
            agent_event.as_str(),
            AgentEventId::PREFIX
        ));
        assert_ne!(PaneKey::new(), PaneKey::new());
    }

    #[test]
    fn deterministic_derivation_has_a_stable_golden_encoding() {
        let host = ExecutionHostId::derive("local", &fixed_install());
        assert_eq!(
            host.as_str(),
            "host-v1:eb7b1ae603a90d4daa5ab1dd0e661ad3c7448afad8f25ac41f9aee8480e772bc"
        );

        let repo = RepoId::derive(&host, "/srv/repo/.git");
        assert_eq!(
            repo.as_str(),
            "repo-v1:f72fe10a6e6368b839ed1a680aa8fd9d227bf2e3916b2d0f02707b93a89848d3"
        );

        let worktree = WorktreeId::derive(&repo, "/srv/repo", None);
        assert_eq!(
            worktree.as_str(),
            "worktree-v1:656e9e43c1f66d0ced3aac1c677496b32c7e370e6358a869c7bd71301d57d1a7"
        );
    }

    #[test]
    fn derivation_is_domain_separated_and_length_prefixed() {
        assert_ne!(
            derived_value("test-v1:", "test/v1", &["ab", "c"]),
            derived_value("test-v1:", "test/v1", &["a", "bc"])
        );

        let host = ExecutionHostId::derive("local", &fixed_install());
        let repo = RepoId::derive(&host, "/repo");
        assert_ne!(
            WorktreeId::derive(&repo, "/repo", None),
            WorktreeId::derive(&repo, "/repo", Some(""))
        );
        assert_ne!(
            WorktreeId::derive(&repo, "/repo", Some("one")),
            WorktreeId::derive(&repo, "/repo", Some("two"))
        );
    }

    #[test]
    fn parsing_and_serde_reject_wrong_or_noncanonical_values() {
        let pane: PaneKey = "pane-v1:123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap();
        assert_eq!(serde_json::to_string(&pane).unwrap(), format!("\"{pane}\""));
        assert_eq!(
            serde_json::from_str::<PaneKey>(&format!("\"{pane}\"")).unwrap(),
            pane
        );

        assert!(
            "tab-v1:123e4567-e89b-42d3-a456-426614174000"
                .parse::<PaneKey>()
                .is_err()
        );
        assert!(
            "pane-v1:123E4567-E89B-42D3-A456-426614174000"
                .parse::<PaneKey>()
                .is_err()
        );
        assert!(
            "pane-v1:123e4567-e89b-12d3-a456-426614174000"
                .parse::<PaneKey>()
                .is_err()
        );
        assert!(serde_json::from_str::<RepoId>("\"repo-v1:not-a-digest\"").is_err());
        assert!(
            "agent-run-v1:123e4567-e89b-12d3-a456-426614174000"
                .parse::<AgentRunId>()
                .is_err()
        );
        let event: AgentEventId = "agent-event-v1:123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<AgentEventId>(&serde_json::to_string(&event).unwrap()).unwrap(),
            event
        );
    }

    #[test]
    fn borrowed_string_lookup_uses_the_serialized_identity() {
        let pane: PaneKey = "pane-v1:123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap();
        let mut values = HashMap::new();
        values.insert(pane.clone(), 7);
        assert_eq!(values.get(pane.as_str()), Some(&7));
        assert_eq!(String::from(pane.clone()), pane.as_str());
        assert_eq!(pane.as_ref(), pane.as_str());
    }
}
