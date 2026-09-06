use std::collections::HashSet;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, MapAccess, Visitor};
use serde::de::value::MapAccessDeserializer;
use serde::{Deserialize, Deserializer};

use crate::account_error::classify_account_diagnostic;
use crate::{AccountCommandStage, AccountError, CommandOutput, require_account_success};

pub const KNOWN_ACCOUNT_LIMIT: usize = 64;
pub const ACCOUNT_LOGIN_LIMIT: usize = 39;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GitHubAccountIdentity {
    host: String,
    login: String,
}

impl GitHubAccountIdentity {
    pub fn new(host: &str, login: &str) -> Result<Self, AccountError> {
        Ok(Self {
            host: normalize_account_host(host)?,
            login: normalize_account_login(login)?,
        })
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn login(&self) -> &str {
        &self.login
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KnownAccountState {
    Success,
    Timeout,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownGitHubAccount {
    identity: GitHubAccountIdentity,
    login: String,
    active: bool,
    state: KnownAccountState,
    problem: Option<AccountError>,
}

impl KnownGitHubAccount {
    pub fn identity(&self) -> &GitHubAccountIdentity {
        &self.identity
    }

    /// Exact spelling from gh, for case-sensitive config/keyring lookup only.
    /// Use identity().login() for selection persistence and account comparisons.
    pub fn login(&self) -> &str {
        &self.login
    }

    /// Informational only; never a selection default or request ownership fence.
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn state(&self) -> KnownAccountState {
        self.state
    }

    pub fn problem(&self) -> Option<AccountError> {
        self.problem
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownGitHubAccounts {
    host: String,
    accounts: Vec<KnownGitHubAccount>,
}

impl KnownGitHubAccounts {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn accounts(&self) -> &[KnownGitHubAccount] {
        &self.accounts
    }

    /// An unavailable selected row is still returned so its problem stays visible.
    pub fn find(&self, identity: &GitHubAccountIdentity) -> Result<&KnownGitHubAccount, AccountError> {
        if self.host != identity.host() {
            return Err(AccountError::WrongHostOrAccount);
        }
        self.accounts
            .iter()
            .find(|account| account.identity() == identity)
            .ok_or(AccountError::SelectedAccountUnavailable)
    }

    /// Only for a project with no saved selection. Never call as a fallback.
    pub fn initial_selection(&self) -> Option<&KnownGitHubAccount> {
        match self.accounts.as_slice() {
            [account] if account.state == KnownAccountState::Success => Some(account),
            _ => None,
        }
    }
}

// gh auth/status/status.go: hosts is a map of arrays, not an account list.
// No raw DTO implements Debug and unknown fields (including token) are skipped.
#[derive(Deserialize)]
struct RawAuthStatus {
    hosts: RawHosts,
}

// Serde's derived structs also accept positional arrays. This boundary only
// accepts JSON objects, while leaving duplicate-field checks in the derives.
struct JsonObject<T>(T);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for JsonObject<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ObjectVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
            type Value = JsonObject<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                T::deserialize(MapAccessDeserializer::new(map)).map(JsonObject)
            }
        }

        deserializer.deserialize_map(ObjectVisitor(PhantomData))
    }
}

struct RawHosts(Vec<(String, Vec<JsonObject<RawKnownAccount>>)>);

impl<'de> Deserialize<'de> for RawHosts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HostVisitor;

        impl<'de> Visitor<'de> for HostVisitor {
            type Value = RawHosts;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an object containing at most the requested host")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut hosts = Vec::new();
                while let Some(host) = map.next_key::<String>()? {
                    // A normal map decoder would silently overwrite duplicate keys.
                    if !hosts.is_empty() {
                        return Err(de::Error::custom("duplicate or unexpected host"));
                    }
                    let accounts = map.next_value::<Vec<JsonObject<RawKnownAccount>>>()?;
                    if accounts.len() > KNOWN_ACCOUNT_LIMIT {
                        return Err(de::Error::custom("too many accounts"));
                    }
                    hosts.push((host, accounts));
                }
                Ok(RawHosts(hosts))
            }
        }

        deserializer.deserialize_map(HostVisitor)
    }
}

#[derive(Deserialize)]
struct RawKnownAccount {
    state: KnownAccountState,
    #[serde(default, deserialize_with = "deserialize_problem")]
    error: Option<AccountError>,
    active: bool,
    host: String,
    login: String,
    #[serde(
        default,
        rename = "tokenSource",
        deserialize_with = "deserialize_inherited_auth"
    )]
    inherited_auth: bool,
}

fn deserialize_problem<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<AccountError>, D::Error> {
    let error = String::deserialize(deserializer)?;
    if error.is_empty() {
        return Err(de::Error::custom("empty account error"));
    }
    Ok(Some(
        classify_account_diagnostic(&error).unwrap_or(AccountError::CommandFailed),
    ))
}

fn deserialize_inherited_auth<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<bool, D::Error> {
    let source = String::deserialize(deserializer)?;
    Ok(source.ends_with("_TOKEN"))
}

/// Parse only a complete nonsecret `known_accounts_plan` result. The executor
/// must clear inherited auth/debug overrides before enumeration on that host.
pub fn parse_known_accounts(
    host: &str,
    output: &CommandOutput,
) -> Result<KnownGitHubAccounts, AccountError> {
    let host = normalize_account_host(host)?;
    let bytes = require_account_success(AccountCommandStage::Enumeration, output)?;
    let JsonObject(raw): JsonObject<RawAuthStatus> =
        serde_json::from_slice(bytes).map_err(|_| AccountError::MalformedResponse)?;
    let mut accounts = Vec::new();
    let mut identities = HashSet::new();
    let mut active_count = 0;
    for (raw_host, entries) in raw.hosts.0 {
        if normalize_account_host(&raw_host)? != host {
            return Err(AccountError::WrongHostOrAccount);
        }
        for JsonObject(entry) in entries {
            if entry.inherited_auth {
                return Err(AccountError::InheritedAuthentication);
            }
            let identity = GitHubAccountIdentity::new(&entry.host, &entry.login)?;
            if identity.host() != host {
                return Err(AccountError::WrongHostOrAccount);
            }
            if !identities.insert(identity.clone()) {
                return Err(AccountError::DuplicateAccount);
            }
            if entry.active {
                active_count += 1;
                if active_count > 1 {
                    return Err(AccountError::MalformedResponse);
                }
            }
            let problem = match (entry.state, entry.error) {
                (KnownAccountState::Success, None) => None,
                (KnownAccountState::Timeout, Some(_)) => Some(AccountError::Offline),
                (KnownAccountState::Error, Some(error)) => Some(error),
                _ => return Err(AccountError::MalformedResponse),
            };
            accounts.push(KnownGitHubAccount {
                identity,
                login: entry.login,
                active: entry.active,
                state: entry.state,
                problem,
            });
        }
    }
    accounts.sort_by(|left, right| left.identity.login.cmp(&right.identity.login));
    Ok(KnownGitHubAccounts { host, accounts })
}

#[derive(Deserialize)]
struct AccountProof {
    login: String,
}

/// The dedicated executor must bind this response to the selected request host
/// and credential. JSON login alone cannot prove transport/source ownership.
pub fn verify_selected_account(
    expected: &GitHubAccountIdentity,
    output: &CommandOutput,
) -> Result<(), AccountError> {
    let bytes = require_account_success(AccountCommandStage::Identity, output)?;
    let JsonObject(raw): JsonObject<AccountProof> =
        serde_json::from_slice(bytes).map_err(|_| AccountError::MalformedResponse)?;
    if normalize_account_login(&raw.login)? != expected.login() {
        return Err(AccountError::WrongHostOrAccount);
    }
    Ok(())
}

pub(crate) fn normalize_account_host(value: &str) -> Result<String, AccountError> {
    let without_root_dot = value.strip_suffix('.').unwrap_or(value);
    if without_root_dot.len() > 253
        || without_root_dot.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label.starts_with(|ch: char| ch.is_ascii_alphanumeric())
                || !label.ends_with(|ch: char| ch.is_ascii_alphanumeric())
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(AccountError::InvalidHost);
    }
    crate::remote::normalize_host(value).map_err(|_| AccountError::InvalidHost)
}

fn normalize_account_login(value: &str) -> Result<String, AccountError> {
    // Managed-user and setup-admin names may include one underscore suffix.
    let mut parts = value.split('_');
    let name = parts.next().unwrap_or_default();
    let suffix = parts.next();
    if value.len() > ACCOUNT_LOGIN_LIMIT
        || name.is_empty()
        || !name.starts_with(|ch: char| ch.is_ascii_alphanumeric())
        || !name.ends_with(|ch: char| ch.is_ascii_alphanumeric())
        || name.contains("--")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || suffix.is_some_and(|suffix| {
            suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
        || parts.next().is_some()
    {
        return Err(AccountError::InvalidLogin);
    }
    Ok(value.to_ascii_lowercase())
}
