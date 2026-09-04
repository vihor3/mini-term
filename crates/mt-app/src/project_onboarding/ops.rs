use mt_github::CommandPlan;

use super::model::{
    GitRelationship, HostPathProbe, OnboardingError, OnboardingErrorKind,
    OnboardingOperationResult, OperationResultAuthority, ProjectLocationKey, TargetState,
    VerifiedProjectLocation,
};

const ERROR_DETAIL_LIMIT: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostCommandDispatch {
    Completed,
    SafeBeforeDispatchFailure,
    OutcomeUncertain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostCommandOutcome {
    pub dispatch: HostCommandDispatch,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub stderr: Vec<u8>,
    pub observed_connection_epoch: Option<u64>,
}

impl HostCommandOutcome {
    pub fn succeeded(&self) -> bool {
        self.dispatch == HostCommandDispatch::Completed
            && !self.timed_out
            && self.exit_code == Some(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedUncertainPostcondition {
    pub probe: HostPathProbe,
    pub authority: OperationResultAuthority,
}

pub trait ProjectHostOps {
    fn probe_existing_directory(
        &self,
        path: &str,
        include_empty: bool,
        inspect_git: bool,
    ) -> Result<HostPathProbe, OnboardingError>;

    fn probe_target(
        &self,
        canonical_parent: &str,
        name: &str,
    ) -> Result<TargetState, OnboardingError>;

    fn create_directory_exclusive(&self, canonical_target: &str) -> Result<(), OnboardingError>;

    fn remove_empty_directory(&self, canonical_target: &str) -> Result<(), OnboardingError>;

    fn run_git(&self, cwd: &str, plan: &CommandPlan)
    -> Result<HostCommandOutcome, OnboardingError>;

    fn probe_after_uncertain_dispatch(
        &self,
        _path: &str,
        _include_empty: bool,
        _inspect_git: bool,
    ) -> Result<VerifiedUncertainPostcondition, OnboardingError> {
        Err(OnboardingError::new(
            OnboardingErrorKind::RemoteOutcomeUncertain,
            "this host adapter does not support uncertain-dispatch recovery",
        ))
    }

    fn location_key(&self, canonical_path: &str) -> Result<ProjectLocationKey, OnboardingError>;

    fn join_path(&self, canonical_parent: &str, name: &str) -> String;

    fn validate_basename(&self, name: &str) -> Result<(), OnboardingError>;
}

pub fn add_existing_folder(
    host: &impl ProjectHostOps,
    selected_path: &str,
) -> Result<OnboardingOperationResult, OnboardingError> {
    let probe = host.probe_existing_directory(selected_path, false, false)?;
    ready_to_register(host, &probe.canonical_path, probe.observed_connection_epoch)
}

pub fn clone_from_url(
    host: &impl ProjectHostOps,
    git_url: &str,
    parent_path: &str,
    target_name: &str,
) -> Result<OnboardingOperationResult, OnboardingError> {
    validate_git_url(git_url)?;
    host.validate_basename(target_name)?;
    let parent = host.probe_existing_directory(parent_path, false, false)?;
    let target_state = host.probe_target(&parent.canonical_path, target_name)?;
    let target = target_state.canonical_target().to_string();
    match target_state {
        TargetState::Absent { .. } | TargetState::EmptyDirectory { .. } => {}
        TargetState::NonEmptyDirectory { .. } => {
            return Err(OnboardingError::new(
                OnboardingErrorKind::Collision,
                format!("clone target is not empty: {target}"),
            ));
        }
        TargetState::Other { .. } => {
            return Err(OnboardingError::new(
                OnboardingErrorKind::Collision,
                format!("clone target already exists and is not a directory: {target}"),
            ));
        }
    }

    let command = CommandPlan::new("git", ["clone", "--", git_url, target_name]);
    let outcome = match host.run_git(&parent.canonical_path, &command) {
        Ok(outcome) => outcome,
        Err(error) => {
            return Err(preserve_clone_target(
                host,
                &parent.canonical_path,
                target_name,
                &target,
                preserve_unclassified_command_target(error, &target),
            ));
        }
    };
    if !outcome.succeeded() {
        if outcome.dispatch == HostCommandDispatch::OutcomeUncertain {
            let verification = host.probe_after_uncertain_dispatch(&target, true, true);
            if let Ok(verification) = &verification
                && is_exact_repository_root(&verification.probe)
            {
                return ready_to_register_with_authority(
                    host,
                    &verification.probe.canonical_path,
                    verification.authority,
                );
            }
            let verification_authority = uncertain_probe_authority(&verification);
            return Err(uncertain_mutation_failure(
                "clone",
                git_url,
                &target,
                &outcome,
                verification.as_ref().err(),
                verification_authority,
            ));
        }
        let error = command_failure("clone", git_url, &target, &outcome);
        if outcome.dispatch == HostCommandDispatch::SafeBeforeDispatchFailure {
            return Err(error.with_cleanup(format!(
                "clone was not dispatched; target was preserved: {target}"
            )));
        }
        return Err(preserve_clone_target(
            host,
            &parent.canonical_path,
            target_name,
            &target,
            error,
        ));
    }

    let probe = host
        .probe_existing_directory(&target, true, true)
        .map_err(|error| {
            OnboardingError::new(
                OnboardingErrorKind::PostconditionFailed,
                format!(
                    "clone completed but target verification failed at {target}: {}",
                    error.message
                ),
            )
            .with_cleanup(format!("target was preserved for inspection: {target}"))
        })?;
    require_exact_repository_root(&probe, "clone").map_err(|error| {
        error.with_cleanup(format!("target was preserved for inspection: {target}"))
    })?;
    ready_to_register(host, &probe.canonical_path, probe.observed_connection_epoch)
}

pub fn create_new_project(
    host: &impl ProjectHostOps,
    parent_path: &str,
    project_name: &str,
) -> Result<OnboardingOperationResult, OnboardingError> {
    host.validate_basename(project_name)?;
    let parent = host.probe_existing_directory(parent_path, false, false)?;
    let target_state = host.probe_target(&parent.canonical_path, project_name)?;
    let target = target_state.canonical_target().to_string();
    if !matches!(target_state, TargetState::Absent { .. }) {
        return Err(OnboardingError::new(
            OnboardingErrorKind::Collision,
            format!("project target already exists: {target}"),
        ));
    }

    host.create_directory_exclusive(&target)?;
    let command = CommandPlan::new("git", ["init"]);
    let outcome = match host.run_git(&target, &command) {
        Ok(outcome) => outcome,
        Err(error) if error_proves_safe_before_dispatch(&error) => {
            return Err(cleanup_owned_empty_target(
                host,
                &parent.canonical_path,
                project_name,
                &target,
                error,
            ));
        }
        Err(error) => return Err(preserve_unclassified_command_target(error, &target)),
    };
    if !outcome.succeeded() {
        if outcome.dispatch == HostCommandDispatch::OutcomeUncertain {
            let verification = host.probe_after_uncertain_dispatch(&target, true, true);
            if let Ok(verification) = &verification
                && is_exact_repository_root(&verification.probe)
            {
                return ready_to_register_with_authority(
                    host,
                    &verification.probe.canonical_path,
                    verification.authority,
                );
            }
            let verification_authority = uncertain_probe_authority(&verification);
            return Err(uncertain_mutation_failure(
                "git init",
                "",
                &target,
                &outcome,
                verification.as_ref().err(),
                verification_authority,
            ));
        }
        let error = command_failure("git init", "", &target, &outcome);
        return Err(cleanup_owned_empty_target(
            host,
            &parent.canonical_path,
            project_name,
            &target,
            error,
        ));
    }

    let probe = host
        .probe_existing_directory(&target, true, true)
        .map_err(|error| {
            OnboardingError::new(
                OnboardingErrorKind::PostconditionFailed,
                format!(
                    "git init completed but target verification failed at {target}: {}",
                    error.message
                ),
            )
            .with_cleanup(format!("target was preserved for inspection: {target}"))
        })?;
    require_exact_repository_root(&probe, "git init").map_err(|error| {
        error.with_cleanup(format!("target was preserved for inspection: {target}"))
    })?;
    ready_to_register(host, &probe.canonical_path, probe.observed_connection_epoch)
}

pub fn initialize_existing_folder(
    host: &impl ProjectHostOps,
    selected_path: &str,
) -> Result<OnboardingOperationResult, OnboardingError> {
    let initial = host.probe_existing_directory(selected_path, false, true)?;
    match &initial.git {
        GitRelationship::RepositoryRoot { .. } => {
            return ready_to_register(
                host,
                &initial.canonical_path,
                initial.observed_connection_epoch,
            );
        }
        GitRelationship::NestedInRepository {
            top_level,
            common_dir,
        } => {
            return Ok(OnboardingOperationResult::NestedRepository {
                selected_path: initial.canonical_path,
                repository_root: top_level.clone(),
                common_dir: common_dir.clone(),
                authority: OperationResultAuthority::normal(initial.observed_connection_epoch),
            });
        }
        GitRelationship::NotGit => {}
    }

    let command = CommandPlan::new("git", ["init"]);
    let outcome = host.run_git(&initial.canonical_path, &command)?;
    if !outcome.succeeded() {
        if outcome.dispatch == HostCommandDispatch::OutcomeUncertain {
            let verification =
                host.probe_after_uncertain_dispatch(&initial.canonical_path, false, true);
            if let Ok(verification) = &verification
                && is_exact_repository_root(&verification.probe)
            {
                return ready_to_register_with_authority(
                    host,
                    &verification.probe.canonical_path,
                    verification.authority,
                );
            }
            let verification_authority = uncertain_probe_authority(&verification);
            return Err(uncertain_mutation_failure(
                "git init",
                "",
                &initial.canonical_path,
                &outcome,
                verification.as_ref().err(),
                verification_authority,
            ));
        }
        return Err(command_failure(
            "git init",
            "",
            &initial.canonical_path,
            &outcome,
        ));
    }

    let probe = host
        .probe_existing_directory(&initial.canonical_path, false, true)
        .map_err(|error| {
            OnboardingError::new(
                OnboardingErrorKind::PostconditionFailed,
                format!(
                    "git init completed but repository verification failed at {}: {}",
                    initial.canonical_path, error.message
                ),
            )
        })?;
    require_exact_repository_root(&probe, "git init")?;
    ready_to_register(host, &probe.canonical_path, probe.observed_connection_epoch)
}

pub fn infer_clone_folder_name(url: &str) -> Result<String, OnboardingError> {
    validate_git_url(url)?;
    let without_fragment = url.split_once('#').map_or(url, |(head, _)| head);
    let without_query = without_fragment
        .split_once('?')
        .map_or(without_fragment, |(head, _)| head);
    let trimmed = without_query.trim_end_matches('/');
    let segment = trimmed
        .rsplit(['/', ':'])
        .next()
        .unwrap_or_default()
        .strip_suffix(".git")
        .unwrap_or_else(|| trimmed.rsplit(['/', ':']).next().unwrap_or_default());
    validate_portable_basename(segment)?;
    Ok(segment.to_string())
}

pub fn validate_portable_basename(name: &str) -> Result<(), OnboardingError> {
    let invalid_windows = name.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
    });
    let windows_stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved_windows_name = matches!(
        windows_stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('\0')
        || invalid_windows
        || reserved_windows_name
        || name.ends_with([' ', '.'])
    {
        return Err(OnboardingError::new(
            OnboardingErrorKind::Validation,
            "project folder name must be one host-compatible path segment",
        ));
    }
    Ok(())
}

pub(crate) fn is_proven_not_git_repository(exit_code: Option<i32>, stderr: &[u8]) -> bool {
    exit_code == Some(128)
        && std::str::from_utf8(stderr).is_ok_and(|diagnostic| {
            diagnostic.lines().any(|line| {
                line.trim_start()
                    .to_ascii_lowercase()
                    .starts_with("fatal: not a git repository")
            })
        })
}

pub(crate) fn bounded_lossy_diagnostic(bytes: &[u8], limit: usize) -> String {
    let mut detail = String::from_utf8_lossy(bytes).trim().to_string();
    if detail.len() > limit {
        let mut end = limit;
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        detail.truncate(end);
    }
    detail
}

pub fn redact_git_url(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(scheme_at) = trimmed.find("://") {
        let authority_start = scheme_at + 3;
        let authority_end = trimmed[authority_start..]
            .find(['/', '?', '#'])
            .map(|offset| authority_start + offset)
            .unwrap_or(trimmed.len());
        let authority = &trimmed[authority_start..authority_end];
        if let Some(at) = authority.rfind('@') {
            let mut redacted = String::with_capacity(trimmed.len());
            redacted.push_str(&trimmed[..authority_start]);
            redacted.push_str("<redacted>@");
            redacted.push_str(&authority[at + 1..]);
            redacted.push_str(&trimmed[authority_end..]);
            return redact_query_and_fragment(redacted);
        }
        return redact_query_and_fragment(trimmed.to_string());
    }
    if let Some(at) = trimmed.find('@')
        && trimmed[at + 1..].contains(':')
    {
        return format!("<redacted>@{}", &trimmed[at + 1..]);
    }
    redact_query_and_fragment(trimmed.to_string())
}

fn redact_query_and_fragment(mut value: String) -> String {
    if let Some(query) = value.find('?') {
        let fragment = value[query..].find('#').map(|offset| query + offset);
        match fragment {
            Some(fragment) => value.replace_range(query..fragment, "?<redacted>"),
            None => value.replace_range(query.., "?<redacted>"),
        }
    }
    if let Some(fragment) = value.find('#') {
        value.replace_range(fragment.., "#<redacted>");
    }
    value
}

fn git_url_has_sensitive_components(value: &str) -> bool {
    let value = value.trim();
    let has_userinfo = if let Some(scheme_at) = value.find("://") {
        let authority_start = scheme_at + 3;
        let authority_end = value[authority_start..]
            .find(['/', '?', '#'])
            .map(|offset| authority_start + offset)
            .unwrap_or(value.len());
        value[authority_start..authority_end].contains('@')
    } else {
        value
            .find('@')
            .is_some_and(|at| value[at + 1..].contains(':'))
    };
    has_userinfo || value.contains(['?', '#'])
}

fn validate_git_url(url: &str) -> Result<(), OnboardingError> {
    let url = url.trim();
    if url.is_empty() || url.contains('\0') || url.chars().any(char::is_whitespace) {
        return Err(OnboardingError::new(
            OnboardingErrorKind::Validation,
            "Git URL is empty or contains unsupported whitespace",
        ));
    }
    if !url.contains('/') && !url.contains(':') {
        return Err(OnboardingError::new(
            OnboardingErrorKind::Validation,
            "Git URL does not contain a repository path",
        ));
    }
    Ok(())
}

fn ready_to_register(
    host: &impl ProjectHostOps,
    canonical_path: &str,
    observed_connection_epoch: Option<u64>,
) -> Result<OnboardingOperationResult, OnboardingError> {
    ready_to_register_with_authority(
        host,
        canonical_path,
        OperationResultAuthority::normal(observed_connection_epoch),
    )
}

fn ready_to_register_with_authority(
    host: &impl ProjectHostOps,
    canonical_path: &str,
    authority: OperationResultAuthority,
) -> Result<OnboardingOperationResult, OnboardingError> {
    let suggested_name = canonical_path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(canonical_path)
        .to_string();
    Ok(OnboardingOperationResult::ReadyToRegister(
        VerifiedProjectLocation {
            key: host.location_key(canonical_path)?,
            canonical_path: canonical_path.to_string(),
            suggested_name,
            authority,
        },
    ))
}

fn is_exact_repository_root(probe: &HostPathProbe) -> bool {
    matches!(probe.git, GitRelationship::RepositoryRoot { .. })
}

fn require_exact_repository_root(
    probe: &HostPathProbe,
    operation: &str,
) -> Result<(), OnboardingError> {
    if is_exact_repository_root(probe) {
        return Ok(());
    }
    Err(OnboardingError::new(
        OnboardingErrorKind::PostconditionFailed,
        format!(
            "{operation} did not produce an exact Git repository root at {}",
            probe.canonical_path
        ),
    ))
}

fn preserve_clone_target(
    host: &impl ProjectHostOps,
    canonical_parent: &str,
    target_name: &str,
    target: &str,
    error: OnboardingError,
) -> OnboardingError {
    match host.probe_target(canonical_parent, target_name) {
        Ok(TargetState::Absent { .. }) => error,
        Ok(TargetState::EmptyDirectory { canonical_target })
        | Ok(TargetState::NonEmptyDirectory { canonical_target })
        | Ok(TargetState::Other { canonical_target }) => error.with_cleanup(format!(
            "clone target was preserved for inspection: {canonical_target}"
        )),
        Err(probe_error) => error.with_cleanup(format!(
            "clone target state is uncertain and was preserved at {target}: {}",
            probe_error.message
        )),
    }
}

fn cleanup_owned_empty_target(
    host: &impl ProjectHostOps,
    canonical_parent: &str,
    target_name: &str,
    target: &str,
    error: OnboardingError,
) -> OnboardingError {
    match host.probe_target(canonical_parent, target_name) {
        Ok(TargetState::Absent { .. }) => error,
        Ok(TargetState::EmptyDirectory { canonical_target }) => {
            if let Err(cleanup) = host.remove_empty_directory(&canonical_target) {
                error.with_cleanup(cleanup.message)
            } else {
                error
            }
        }
        Ok(TargetState::NonEmptyDirectory { canonical_target }) => error.with_cleanup(format!(
            "operation-created target was not empty and was preserved: {}",
            canonical_target
        )),
        Ok(TargetState::Other { canonical_target }) => error.with_cleanup(format!(
            "operation-created target changed type and was preserved: {}",
            canonical_target
        )),
        Err(probe_error) => error.with_cleanup(format!(
            "target state is uncertain and was preserved at {target}: {}",
            probe_error.message
        )),
    }
}

fn error_proves_safe_before_dispatch(error: &OnboardingError) -> bool {
    matches!(
        error.kind,
        OnboardingErrorKind::Validation
            | OnboardingErrorKind::GitUnavailable
            | OnboardingErrorKind::Authentication
            | OnboardingErrorKind::DisconnectedBeforeDispatch
    )
}

fn preserve_unclassified_command_target(error: OnboardingError, target: &str) -> OnboardingError {
    error.with_cleanup(format!(
        "skipped because command completion was not proven; target was preserved for inspection: {target}"
    ))
}

fn uncertain_mutation_failure(
    operation: &str,
    secret_url: &str,
    target: &str,
    outcome: &HostCommandOutcome,
    verification_error: Option<&OnboardingError>,
    verification_authority: Option<OperationResultAuthority>,
) -> OnboardingError {
    let error = command_failure(operation, secret_url, target, outcome);
    let error = match verification_error {
        Some(error_detail) => error.with_cleanup(format!(
            "remote outcome could not be verified and the target was preserved at {target}: {}",
            error_detail.message
        )),
        None => error.with_cleanup(format!(
            "remote outcome did not prove the requested repository state; target was preserved at {target}"
        )),
    };
    match verification_authority {
        Some(authority) => error.with_authority(authority),
        None => error,
    }
}

fn uncertain_probe_authority(
    verification: &Result<VerifiedUncertainPostcondition, OnboardingError>,
) -> Option<OperationResultAuthority> {
    match verification {
        Ok(verified) => Some(verified.authority),
        Err(error) => error.authority,
    }
}

fn command_failure(
    operation: &str,
    secret_url: &str,
    target: &str,
    outcome: &HostCommandOutcome,
) -> OnboardingError {
    let kind = match outcome.dispatch {
        HostCommandDispatch::SafeBeforeDispatchFailure => {
            OnboardingErrorKind::DisconnectedBeforeDispatch
        }
        HostCommandDispatch::OutcomeUncertain => OnboardingErrorKind::RemoteOutcomeUncertain,
        HostCommandDispatch::Completed => OnboardingErrorKind::GitFailure,
    };
    let mut detail = bounded_lossy_diagnostic(&outcome.stderr, outcome.stderr.len());
    detail = if git_url_has_sensitive_components(secret_url) {
        String::new()
    } else {
        redact_git_diagnostic(detail, secret_url)
    };
    if detail.len() > ERROR_DETAIL_LIMIT {
        detail = bounded_lossy_diagnostic(detail.as_bytes(), ERROR_DETAIL_LIMIT);
    }
    let status = if outcome.timed_out {
        "timed out".to_string()
    } else if let Some(code) = outcome.exit_code {
        format!("exited with code {code}")
    } else {
        "did not return an exit code".to_string()
    };
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!("; {detail}")
    };
    OnboardingError::new(kind, format!("{operation} {status} for {target}{detail}"))
}

fn redact_git_diagnostic(mut detail: String, secret_url: &str) -> String {
    let secret_url = secret_url.trim();
    if secret_url.is_empty() {
        return detail;
    }
    detail = detail.replace(secret_url, &redact_git_url(secret_url));

    if let Some(scheme_at) = secret_url.find("://") {
        let authority_start = scheme_at + 3;
        let authority_end = secret_url[authority_start..]
            .find(['/', '?', '#'])
            .map(|offset| authority_start + offset)
            .unwrap_or(secret_url.len());
        let authority = &secret_url[authority_start..authority_end];
        if let Some(at) = authority.rfind('@') {
            detail = detail.replace(&authority[..=at], "<redacted>@");
        }
    } else if let Some(at) = secret_url.find('@')
        && secret_url[at + 1..].contains(':')
    {
        detail = detail.replace(&secret_url[..=at], "<redacted>@");
    }

    if let Some(query_start) = secret_url.find('?') {
        let query_end = secret_url[query_start..]
            .find('#')
            .map(|offset| query_start + offset)
            .unwrap_or(secret_url.len());
        let query = &secret_url[query_start..query_end];
        if query.len() > 1 {
            detail = detail.replace(query, "?<redacted>");
        }
    }
    detail
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::super::model::OperationResultProvenance;
    use super::*;

    struct FakeHost {
        calls: RefCell<Vec<String>>,
        git_inspection_requests: RefCell<Vec<bool>>,
        probes: RefCell<VecDeque<Result<HostPathProbe, OnboardingError>>>,
        targets: RefCell<VecDeque<Result<TargetState, OnboardingError>>>,
        commands: RefCell<VecDeque<Result<HostCommandOutcome, OnboardingError>>>,
        creates: RefCell<VecDeque<Result<(), OnboardingError>>>,
        removals: RefCell<VecDeque<Result<(), OnboardingError>>>,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                git_inspection_requests: RefCell::new(Vec::new()),
                probes: RefCell::new(VecDeque::new()),
                targets: RefCell::new(VecDeque::new()),
                commands: RefCell::new(VecDeque::new()),
                creates: RefCell::new(VecDeque::new()),
                removals: RefCell::new(VecDeque::new()),
            }
        }

        fn probe(path: &str, git: GitRelationship, empty: Option<bool>) -> HostPathProbe {
            HostPathProbe {
                canonical_path: path.into(),
                directory_empty: empty,
                git,
                observed_connection_epoch: None,
            }
        }
    }

    impl ProjectHostOps for FakeHost {
        fn probe_existing_directory(
            &self,
            path: &str,
            _include_empty: bool,
            inspect_git: bool,
        ) -> Result<HostPathProbe, OnboardingError> {
            self.calls.borrow_mut().push(format!("probe:{path}"));
            self.git_inspection_requests.borrow_mut().push(inspect_git);
            self.probes.borrow_mut().pop_front().unwrap()
        }

        fn probe_target(&self, parent: &str, name: &str) -> Result<TargetState, OnboardingError> {
            self.calls
                .borrow_mut()
                .push(format!("target:{parent}:{name}"));
            self.targets.borrow_mut().pop_front().unwrap()
        }

        fn create_directory_exclusive(&self, target: &str) -> Result<(), OnboardingError> {
            self.calls.borrow_mut().push(format!("create:{target}"));
            self.creates.borrow_mut().pop_front().unwrap_or(Ok(()))
        }

        fn remove_empty_directory(&self, target: &str) -> Result<(), OnboardingError> {
            self.calls.borrow_mut().push(format!("remove:{target}"));
            self.removals.borrow_mut().pop_front().unwrap_or(Ok(()))
        }

        fn run_git(
            &self,
            cwd: &str,
            plan: &CommandPlan,
        ) -> Result<HostCommandOutcome, OnboardingError> {
            self.calls
                .borrow_mut()
                .push(format!("git:{cwd}:{}", plan.display_argv().join("|")));
            self.commands.borrow_mut().pop_front().unwrap()
        }

        fn probe_after_uncertain_dispatch(
            &self,
            path: &str,
            include_empty: bool,
            inspect_git: bool,
        ) -> Result<VerifiedUncertainPostcondition, OnboardingError> {
            let probe = self.probe_existing_directory(path, include_empty, inspect_git)?;
            Ok(VerifiedUncertainPostcondition {
                authority: OperationResultAuthority::verified_after_uncertain_dispatch(
                    probe.observed_connection_epoch,
                ),
                probe,
            })
        }

        fn location_key(&self, path: &str) -> Result<ProjectLocationKey, OnboardingError> {
            Ok(ProjectLocationKey::Local {
                normalized_canonical_path: path.into(),
            })
        }

        fn join_path(&self, parent: &str, name: &str) -> String {
            format!("{}/{name}", parent.trim_end_matches('/'))
        }

        fn validate_basename(&self, name: &str) -> Result<(), OnboardingError> {
            validate_portable_basename(name)
        }
    }

    fn success() -> HostCommandOutcome {
        HostCommandOutcome {
            dispatch: HostCommandDispatch::Completed,
            exit_code: Some(0),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
            stderr: Vec::new(),
            observed_connection_epoch: None,
        }
    }

    fn failure(dispatch: HostCommandDispatch, detail: &str) -> HostCommandOutcome {
        HostCommandOutcome {
            dispatch,
            exit_code: None,
            timed_out: dispatch == HostCommandDispatch::OutcomeUncertain,
            stdout_truncated: false,
            stderr_truncated: false,
            stderr: detail.as_bytes().to_vec(),
            observed_connection_epoch: Some(12),
        }
    }

    #[test]
    fn add_existing_is_probe_only() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/repo",
            GitRelationship::NotGit,
            None,
        )));
        add_existing_folder(&host, "/repo").unwrap();
        assert_eq!(&*host.calls.borrow(), &["probe:/repo"]);
        assert_eq!(&*host.git_inspection_requests.borrow(), &[false]);
    }

    #[test]
    fn clone_uses_structured_argv_and_registers_only_after_exact_root_probe() {
        let host = FakeHost::new();
        host.probes.borrow_mut().extend([
            Ok(FakeHost::probe("/parent", GitRelationship::NotGit, None)),
            Ok(FakeHost::probe(
                "/parent/repo",
                GitRelationship::RepositoryRoot {
                    top_level: "/parent/repo".into(),
                    common_dir: "/parent/repo/.git".into(),
                },
                Some(false),
            )),
        ]);
        host.targets.borrow_mut().push_back(Ok(TargetState::Absent {
            canonical_target: "/parent/repo".into(),
        }));
        host.commands.borrow_mut().push_back(Ok(success()));
        clone_from_url(
            &host,
            "https://user:token@example.com/o/repo.git",
            "/parent",
            "repo",
        )
        .unwrap();
        assert_eq!(
            &*host.calls.borrow(),
            &[
                "probe:/parent",
                "target:/parent:repo",
                "git:/parent:git|clone|--|https://user:token@example.com/o/repo.git|repo",
                "probe:/parent/repo",
            ]
        );
        assert_eq!(&*host.git_inspection_requests.borrow(), &[false, true]);
    }

    #[test]
    fn clone_collision_stops_before_git_dispatch() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets
            .borrow_mut()
            .push_back(Ok(TargetState::NonEmptyDirectory {
                canonical_target: "/parent/repo".into(),
            }));

        let error =
            clone_from_url(&host, "https://example.com/o/repo.git", "/parent", "repo").unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::Collision);
        assert_eq!(
            &*host.calls.borrow(),
            &["probe:/parent", "target:/parent:repo"]
        );
    }

    #[test]
    fn new_folder_collision_stops_before_creation() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets
            .borrow_mut()
            .push_back(Ok(TargetState::EmptyDirectory {
                canonical_target: "/parent/repo".into(),
            }));

        let error = create_new_project(&host, "/parent", "repo").unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::Collision);
        assert_eq!(
            &*host.calls.borrow(),
            &["probe:/parent", "target:/parent:repo"]
        );
    }

    #[test]
    fn nested_existing_folder_returns_root_without_mutation() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/repo/child",
            GitRelationship::NestedInRepository {
                top_level: "/repo".into(),
                common_dir: "/repo/.git".into(),
            },
            None,
        )));

        let result = initialize_existing_folder(&host, "/repo/child").unwrap();

        assert!(matches!(
            result,
            OnboardingOperationResult::NestedRepository {
                repository_root,
                ..
            } if repository_root == "/repo"
        ));
        assert_eq!(&*host.calls.borrow(), &["probe:/repo/child"]);
    }

    #[test]
    fn existing_repository_root_skips_git_init() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/repo",
            GitRelationship::RepositoryRoot {
                top_level: "/repo".into(),
                common_dir: "/repo/.git".into(),
            },
            None,
        )));

        let result = initialize_existing_folder(&host, "/repo").unwrap();

        assert!(matches!(
            result,
            OnboardingOperationResult::ReadyToRegister(VerifiedProjectLocation {
                authority: OperationResultAuthority {
                    provenance: OperationResultProvenance::Normal,
                    ..
                },
                ..
            })
        ));
        assert_eq!(&*host.calls.borrow(), &["probe:/repo"]);
    }

    #[test]
    fn uncertain_clone_accepts_only_a_verified_exact_root() {
        let host = FakeHost::new();
        host.probes.borrow_mut().extend([
            Ok(FakeHost::probe("/parent", GitRelationship::NotGit, None)),
            Ok(FakeHost::probe(
                "/parent/repo",
                GitRelationship::RepositoryRoot {
                    top_level: "/parent/repo".into(),
                    common_dir: "/parent/repo/.git".into(),
                },
                Some(false),
            )),
        ]);
        host.targets.borrow_mut().push_back(Ok(TargetState::Absent {
            canonical_target: "/parent/repo".into(),
        }));
        host.commands.borrow_mut().push_back(Ok(failure(
            HostCommandDispatch::OutcomeUncertain,
            "SSH reply was lost",
        )));

        let result =
            clone_from_url(&host, "ssh://git@example.com/o/repo.git", "/parent", "repo").unwrap();

        assert!(matches!(
            result,
            OnboardingOperationResult::ReadyToRegister(VerifiedProjectLocation {
                authority: OperationResultAuthority {
                    provenance:
                        OperationResultProvenance::PostconditionVerifiedAfterUncertainDispatch,
                    ..
                },
                ..
            })
        ));
        assert!(
            !host
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("remove:"))
        );
    }

    #[test]
    fn uncertain_clone_preserves_operation_created_target() {
        let host = FakeHost::new();
        host.probes.borrow_mut().extend([
            Ok(FakeHost::probe("/parent", GitRelationship::NotGit, None)),
            Ok(FakeHost::probe(
                "/parent/repo",
                GitRelationship::NotGit,
                Some(true),
            )),
        ]);
        host.targets.borrow_mut().push_back(Ok(TargetState::Absent {
            canonical_target: "/parent/repo".into(),
        }));
        host.commands.borrow_mut().push_back(Ok(failure(
            HostCommandDispatch::OutcomeUncertain,
            "SSH exec reply was lost",
        )));

        let error = clone_from_url(&host, "ssh://git@example.com/o/repo.git", "/parent", "repo")
            .unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::RemoteOutcomeUncertain);
        assert!(error.message.contains("target was preserved"));
        assert_eq!(
            error.authority.map(|authority| authority.provenance),
            Some(OperationResultProvenance::PostconditionVerifiedAfterUncertainDispatch)
        );
        assert!(
            !host
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("remove:"))
        );
    }

    #[test]
    fn uncertain_clone_keeps_authority_from_a_failed_recovery_probe() {
        let host = FakeHost::new();
        host.probes.borrow_mut().extend([
            Ok(FakeHost::probe("/parent", GitRelationship::NotGit, None)),
            Err(OnboardingError::new(
                OnboardingErrorKind::GitFailure,
                "repository probe failed on the current recovery session",
            )
            .with_authority(
                OperationResultAuthority::verified_after_uncertain_dispatch(Some(13)),
            )),
        ]);
        host.targets.borrow_mut().push_back(Ok(TargetState::Absent {
            canonical_target: "/parent/repo".into(),
        }));
        host.commands.borrow_mut().push_back(Ok(failure(
            HostCommandDispatch::OutcomeUncertain,
            "SSH exec reply was lost",
        )));

        let error = clone_from_url(&host, "ssh://git@example.com/o/repo.git", "/parent", "repo")
            .unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::RemoteOutcomeUncertain);
        assert_eq!(
            error.authority,
            Some(OperationResultAuthority::verified_after_uncertain_dispatch(
                Some(13),
            ))
        );
        assert!(error.message.contains("repository probe failed"));
    }

    #[test]
    fn uncertain_new_folder_init_never_attempts_cleanup() {
        let host = FakeHost::new();
        host.probes.borrow_mut().extend([
            Ok(FakeHost::probe("/parent", GitRelationship::NotGit, None)),
            Ok(FakeHost::probe(
                "/parent/repo",
                GitRelationship::NotGit,
                Some(true),
            )),
        ]);
        host.targets.borrow_mut().push_back(Ok(TargetState::Absent {
            canonical_target: "/parent/repo".into(),
        }));
        host.commands.borrow_mut().push_back(Ok(failure(
            HostCommandDispatch::OutcomeUncertain,
            "channel state is uncertain",
        )));

        let error = create_new_project(&host, "/parent", "repo").unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::RemoteOutcomeUncertain);
        assert_eq!(
            error.authority.map(|authority| authority.provenance),
            Some(OperationResultProvenance::PostconditionVerifiedAfterUncertainDispatch)
        );
        assert!(
            !host
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("remove:"))
        );
    }

    #[test]
    fn completed_init_failure_removes_only_a_proven_empty_owned_directory() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets.borrow_mut().extend([
            Ok(TargetState::Absent {
                canonical_target: "/parent/repo".into(),
            }),
            Ok(TargetState::EmptyDirectory {
                canonical_target: "/parent/repo".into(),
            }),
        ]);
        host.commands.borrow_mut().push_back(Ok(HostCommandOutcome {
            dispatch: HostCommandDispatch::Completed,
            exit_code: Some(1),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
            stderr: b"init failed".to_vec(),
            observed_connection_epoch: None,
        }));

        let error = create_new_project(&host, "/parent", "repo").unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::GitFailure);
        assert!(
            host.calls
                .borrow()
                .contains(&"remove:/parent/repo".to_string())
        );
    }

    #[test]
    fn failed_clone_never_removes_a_pre_existing_empty_target() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets.borrow_mut().extend([
            Ok(TargetState::EmptyDirectory {
                canonical_target: "/parent/repo".into(),
            }),
            Ok(TargetState::EmptyDirectory {
                canonical_target: "/parent/repo".into(),
            }),
        ]);
        host.commands.borrow_mut().push_back(Ok(HostCommandOutcome {
            dispatch: HostCommandDispatch::Completed,
            exit_code: Some(1),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
            stderr: b"clone failed".to_vec(),
            observed_connection_epoch: None,
        }));

        let error =
            clone_from_url(&host, "https://example.com/o/repo.git", "/parent", "repo").unwrap_err();

        assert!(error.message.contains("clone target was preserved"));
        assert!(
            !host
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("remove:"))
        );
    }

    #[test]
    fn completed_clone_failure_preserves_a_concurrently_created_empty_target() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets.borrow_mut().extend([
            Ok(TargetState::Absent {
                canonical_target: "/parent/repo".into(),
            }),
            Ok(TargetState::EmptyDirectory {
                canonical_target: "/parent/repo".into(),
            }),
        ]);
        host.commands.borrow_mut().push_back(Ok(HostCommandOutcome {
            dispatch: HostCommandDispatch::Completed,
            exit_code: Some(1),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
            stderr: b"clone failed".to_vec(),
            observed_connection_epoch: None,
        }));

        let error =
            clone_from_url(&host, "https://example.com/o/repo.git", "/parent", "repo").unwrap_err();

        assert!(error.message.contains("clone target was preserved"));
        assert!(
            !host
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("remove:"))
        );
    }

    #[test]
    fn clone_runner_error_preserves_target_when_completion_is_not_proven() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets.borrow_mut().extend([
            Ok(TargetState::Absent {
                canonical_target: "/parent/repo".into(),
            }),
            Ok(TargetState::NonEmptyDirectory {
                canonical_target: "/parent/repo".into(),
            }),
        ]);
        host.commands
            .borrow_mut()
            .push_back(Err(OnboardingError::new(
                OnboardingErrorKind::GitFailure,
                "process status failed after dispatch",
            )));

        let error =
            clone_from_url(&host, "https://example.com/o/repo.git", "/parent", "repo").unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::GitFailure);
        assert!(error.message.contains("completion was not proven"));
        assert!(
            !host
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("remove:"))
        );
    }

    #[test]
    fn clone_safe_before_dispatch_never_removes_an_unowned_target() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets.borrow_mut().push_back(Ok(TargetState::Absent {
            canonical_target: "/parent/repo".into(),
        }));
        host.commands.borrow_mut().push_back(Ok(failure(
            HostCommandDispatch::SafeBeforeDispatchFailure,
            "command was rejected before dispatch",
        )));

        let error =
            clone_from_url(&host, "https://example.com/o/repo.git", "/parent", "repo").unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::DisconnectedBeforeDispatch);
        assert!(error.message.contains("target was preserved"));
        assert!(
            !host
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("remove:"))
        );
    }

    #[test]
    fn new_folder_runner_error_preserves_target_when_completion_is_not_proven() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets.borrow_mut().push_back(Ok(TargetState::Absent {
            canonical_target: "/parent/repo".into(),
        }));
        host.commands
            .borrow_mut()
            .push_back(Err(OnboardingError::new(
                OnboardingErrorKind::GitFailure,
                "process cleanup failed after dispatch",
            )));

        let error = create_new_project(&host, "/parent", "repo").unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::GitFailure);
        assert!(error.message.contains("completion was not proven"));
        assert!(
            !host
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("remove:"))
        );
    }

    #[test]
    fn cleanup_failure_augments_the_primary_git_error() {
        let host = FakeHost::new();
        host.probes.borrow_mut().push_back(Ok(FakeHost::probe(
            "/parent",
            GitRelationship::NotGit,
            None,
        )));
        host.targets.borrow_mut().extend([
            Ok(TargetState::Absent {
                canonical_target: "/parent/repo".into(),
            }),
            Ok(TargetState::EmptyDirectory {
                canonical_target: "/parent/repo".into(),
            }),
        ]);
        host.commands.borrow_mut().push_back(Ok(HostCommandOutcome {
            dispatch: HostCommandDispatch::Completed,
            exit_code: Some(1),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
            stderr: b"primary failure".to_vec(),
            observed_connection_epoch: None,
        }));
        host.removals
            .borrow_mut()
            .push_back(Err(OnboardingError::new(
                OnboardingErrorKind::PostconditionFailed,
                "directory became non-empty",
            )));

        let error = create_new_project(&host, "/parent", "repo").unwrap_err();

        assert!(error.message.contains("primary failure"));
        assert!(error.message.contains("directory became non-empty"));
    }

    #[test]
    fn clone_url_inference_and_redaction_cover_common_forms() {
        assert_eq!(
            infer_clone_folder_name("https://example.com/o/repo.git?ref=main").unwrap(),
            "repo"
        );
        assert_eq!(
            infer_clone_folder_name("git@example.com:o/repo.git").unwrap(),
            "repo"
        );
        assert_eq!(
            redact_git_url("https://user:token@example.com/o/repo.git?token=secret#readme"),
            "https://<redacted>@example.com/o/repo.git?<redacted>#<redacted>"
        );
        assert_eq!(
            redact_git_url("git@example.com:o/repo.git"),
            "<redacted>@example.com:o/repo.git"
        );
        for invalid in ["CON", "aux.txt", "COM1.log", "LPT9", "repo\nname"] {
            assert!(validate_portable_basename(invalid).is_err(), "{invalid}");
        }
        assert!(validate_portable_basename("console").is_ok());
        assert!(validate_portable_basename("com10").is_ok());
    }

    #[test]
    fn command_failure_does_not_expose_clone_credentials() {
        let url = "https://user:token@example.com/o/repo.git?access=secret";
        let outcome = HostCommandOutcome {
            dispatch: HostCommandDispatch::Completed,
            exit_code: Some(128),
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
            stderr: b"fatal: could not read from \
                https://user:token@example.com/o/repo.git/; \
                remote query was ?access=secret"
                .to_vec(),
            observed_connection_epoch: None,
        };

        let error = command_failure("clone", url, "/parent/repo", &outcome);

        assert!(!error.message.contains("user:token"));
        assert!(!error.message.contains("access=secret"));
        assert!(error.message.contains("/parent/repo"));
    }

    #[test]
    fn generic_git_failure_is_not_a_proven_non_repository_response() {
        assert!(is_proven_not_git_repository(
            Some(128),
            b"fatal: not a git repository (or any parent): .git"
        ));
        assert!(!is_proven_not_git_repository(
            Some(128),
            b"fatal: detected dubious ownership"
        ));
        assert!(!is_proven_not_git_repository(
            Some(128),
            b"fatal: detected dubious ownership in '/tmp/not a git repository'"
        ));
        assert!(!is_proven_not_git_repository(
            Some(1),
            b"fatal: not a git repository"
        ));
    }

    #[test]
    fn diagnostic_truncation_preserves_utf8_boundaries() {
        let detail = format!("{}é", "a".repeat(ERROR_DETAIL_LIMIT - 1));

        let bounded = bounded_lossy_diagnostic(detail.as_bytes(), ERROR_DETAIL_LIMIT);

        assert!(bounded.len() <= ERROR_DETAIL_LIMIT);
        assert!(std::str::from_utf8(bounded.as_bytes()).is_ok());
    }

    #[test]
    fn remote_transport_failure_after_session_acquire_is_uncertain() {
        let outcome = remote_mutation_outcome(crate::remote_ssh::RemoteMutationOutcome {
            output: None,
            transport_error: Some("channel exec failed".into()),
            authority_error: None,
            connection_epoch: 42,
            connection_fingerprint: 7,
        });

        assert_eq!(outcome.dispatch, HostCommandDispatch::OutcomeUncertain);
        assert_eq!(outcome.observed_connection_epoch, Some(42));
        assert_eq!(outcome.stderr, b"channel exec failed");
    }

    #[test]
    fn superseded_session_does_not_reclassify_a_normal_result_as_uncertain() {
        let outcome = remote_mutation_outcome(crate::remote_ssh::RemoteMutationOutcome {
            output: Some(mt_ssh::BoundedExecOutput {
                state: mt_ssh::BoundedExecState::Started,
                exit_code: Some(0),
                ..mt_ssh::BoundedExecOutput::default()
            }),
            transport_error: None,
            authority_error: Some("SSH operation result was superseded".into()),
            connection_epoch: 42,
            connection_fingerprint: 7,
        });

        assert_eq!(outcome.dispatch, HostCommandDispatch::Completed);
        assert_eq!(outcome.exit_code, Some(0));
        assert!(String::from_utf8_lossy(&outcome.stderr).contains("superseded"));
    }

    #[test]
    fn remote_target_collisions_keep_their_actionable_error_kind() {
        assert_eq!(
            remote_error("remote project target already exists: /repo".into()).kind,
            OnboardingErrorKind::Collision
        );
        assert_eq!(
            remote_error("remote target is not empty: /repo".into()).kind,
            OnboardingErrorKind::Collision
        );
    }

    #[test]
    fn recovery_probe_error_authority_requires_the_selected_fingerprint() {
        let owned = remote_recovery_error(
            crate::remote_ssh::RemoteRecoveryProbeError {
                message: "repository probe failed".into(),
                connection_epoch: Some(43),
                connection_fingerprint: 7,
            },
            7,
        );
        assert_eq!(
            owned.authority,
            Some(OperationResultAuthority::verified_after_uncertain_dispatch(
                Some(43),
            ))
        );

        let stale = remote_recovery_error(
            crate::remote_ssh::RemoteRecoveryProbeError {
                message: "repository probe failed".into(),
                connection_epoch: Some(44),
                connection_fingerprint: 8,
            },
            7,
        );
        assert_eq!(stale.authority, None);
    }
}

impl ProjectHostOps for crate::remote_ssh::RemoteProjectContext {
    fn probe_existing_directory(
        &self,
        path: &str,
        include_empty: bool,
        inspect_git: bool,
    ) -> Result<HostPathProbe, OnboardingError> {
        let probe =
            crate::remote_ssh::probe_existing_directory(self, path, include_empty, inspect_git)
                .map_err(remote_error)?;
        if probe.provenance != crate::remote_ssh::RemoteProbeProvenance::OperationEpoch {
            return Err(OnboardingError::new(
                OnboardingErrorKind::StaleOperation,
                "normal SSH probe returned recovery-only provenance",
            ));
        }
        Ok(host_path_probe_from_remote(probe))
    }

    fn probe_target(&self, parent: &str, name: &str) -> Result<TargetState, OnboardingError> {
        let probe = crate::remote_ssh::probe_target(self, parent, name).map_err(remote_error)?;
        Ok(match probe.state {
            crate::remote_ssh::RemoteTargetState::Absent(canonical_target) => {
                TargetState::Absent { canonical_target }
            }
            crate::remote_ssh::RemoteTargetState::EmptyDirectory(canonical_target) => {
                TargetState::EmptyDirectory { canonical_target }
            }
            crate::remote_ssh::RemoteTargetState::NonEmptyDirectory(canonical_target) => {
                TargetState::NonEmptyDirectory { canonical_target }
            }
            crate::remote_ssh::RemoteTargetState::Other(canonical_target) => {
                TargetState::Other { canonical_target }
            }
        })
    }

    fn create_directory_exclusive(&self, target: &str) -> Result<(), OnboardingError> {
        crate::remote_ssh::create_directory_exclusive(self, target)
            .map(|_| ())
            .map_err(remote_error)
    }

    fn remove_empty_directory(&self, target: &str) -> Result<(), OnboardingError> {
        crate::remote_ssh::remove_empty_directory(self, target)
            .map(|_| ())
            .map_err(remote_error)
    }

    fn run_git(
        &self,
        cwd: &str,
        plan: &CommandPlan,
    ) -> Result<HostCommandOutcome, OnboardingError> {
        let result = crate::remote_ssh::run_git(self, cwd, plan).map_err(remote_error)?;
        Ok(remote_mutation_outcome(result))
    }

    fn probe_after_uncertain_dispatch(
        &self,
        path: &str,
        include_empty: bool,
        inspect_git: bool,
    ) -> Result<VerifiedUncertainPostcondition, OnboardingError> {
        let probe = crate::remote_ssh::probe_existing_directory_after_uncertain_dispatch(
            self,
            path,
            include_empty,
            inspect_git,
        )
        .map_err(|failure| remote_recovery_error(failure, self.connection_fingerprint))?;
        if probe.provenance
            != crate::remote_ssh::RemoteProbeProvenance::PostconditionVerifiedAfterUncertainDispatch
        {
            return Err(OnboardingError::new(
                OnboardingErrorKind::StaleOperation,
                "SSH uncertainty recovery probe returned normal-operation provenance",
            ));
        }
        let observed_connection_epoch = Some(probe.connection_epoch);
        Ok(VerifiedUncertainPostcondition {
            probe: host_path_probe_from_remote(probe),
            authority: OperationResultAuthority::verified_after_uncertain_dispatch(
                observed_connection_epoch,
            ),
        })
    }

    fn location_key(&self, path: &str) -> Result<ProjectLocationKey, OnboardingError> {
        Ok(ProjectLocationKey::Ssh {
            connection_id: self.connection.id.clone(),
            normalized_posix_path: normalize_posix_location(path)?,
        })
    }

    fn join_path(&self, parent: &str, name: &str) -> String {
        crate::remote_ssh::join_posix(parent, name)
    }

    fn validate_basename(&self, name: &str) -> Result<(), OnboardingError> {
        validate_portable_basename(name)
    }
}

fn host_path_probe_from_remote(probe: crate::remote_ssh::RemotePathProbe) -> HostPathProbe {
    HostPathProbe {
        canonical_path: probe.canonical_path,
        directory_empty: probe.directory_empty,
        git: match probe.git {
            crate::remote_ssh::RemoteGitRelationship::NotGit => GitRelationship::NotGit,
            crate::remote_ssh::RemoteGitRelationship::RepositoryRoot {
                top_level,
                common_dir,
            } => GitRelationship::RepositoryRoot {
                top_level,
                common_dir,
            },
            crate::remote_ssh::RemoteGitRelationship::NestedInRepository {
                top_level,
                common_dir,
            } => GitRelationship::NestedInRepository {
                top_level,
                common_dir,
            },
        },
        observed_connection_epoch: Some(probe.connection_epoch),
    }
}

fn normalize_posix_location(path: &str) -> Result<String, OnboardingError> {
    if !path.starts_with('/') || path.contains('\0') {
        return Err(OnboardingError::new(
            OnboardingErrorKind::Validation,
            format!("SSH project path must be absolute POSIX: {path}"),
        ));
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                return Err(OnboardingError::new(
                    OnboardingErrorKind::Validation,
                    format!("SSH project path cannot contain `..`: {path}"),
                ));
            }
            value => segments.push(value),
        }
    }
    Ok(if segments.is_empty() {
        "/".into()
    } else {
        format!("/{}", segments.join("/"))
    })
}

fn remote_error(message: String) -> OnboardingError {
    let lower = message.to_ascii_lowercase();
    let kind = if lower.contains("auth") || lower.contains("permission denied") {
        OnboardingErrorKind::Authentication
    } else if lower.contains("already exists")
        || lower.contains("target is not empty")
        || lower.contains("target already exists")
    {
        OnboardingErrorKind::Collision
    } else if lower.contains("git is unavailable") {
        OnboardingErrorKind::GitUnavailable
    } else if lower.contains("git could not inspect")
        || lower.contains("repository probe")
        || lower.contains(".git marker")
    {
        OnboardingErrorKind::GitFailure
    } else if lower.contains("superseded") || lower.contains("connection") {
        OnboardingErrorKind::DisconnectedBeforeDispatch
    } else {
        OnboardingErrorKind::Validation
    };
    OnboardingError::new(kind, message)
}

fn remote_recovery_error(
    failure: crate::remote_ssh::RemoteRecoveryProbeError,
    expected_fingerprint: u64,
) -> OnboardingError {
    let mut error = remote_error(failure.message);
    if failure.connection_fingerprint == expected_fingerprint {
        if let Some(epoch) = failure.connection_epoch {
            error = error.with_authority(
                OperationResultAuthority::verified_after_uncertain_dispatch(Some(epoch)),
            );
        }
    }
    error
}

fn remote_mutation_outcome(result: crate::remote_ssh::RemoteMutationOutcome) -> HostCommandOutcome {
    let crate::remote_ssh::RemoteMutationOutcome {
        output,
        transport_error,
        authority_error,
        connection_epoch,
        ..
    } = result;
    let transport_uncertain = transport_error.is_some();
    let mut stderr = Vec::new();
    let (dispatch, exit_code, timed_out, stdout_truncated, stderr_truncated) =
        if let Some(output) = output {
            let dispatch = if transport_uncertain
                || output.requires_session_retirement()
                || output.state == mt_ssh::BoundedExecState::ExecReplyUnknown
                || (output.state == mt_ssh::BoundedExecState::Started
                    && (output.timed_out || output.exit_code.is_none()))
            {
                HostCommandDispatch::OutcomeUncertain
            } else if output.safe_to_fallback() {
                HostCommandDispatch::SafeBeforeDispatchFailure
            } else {
                HostCommandDispatch::Completed
            };
            let fields = (
                dispatch,
                output.exit_code.and_then(|code| i32::try_from(code).ok()),
                output.timed_out,
                output.stdout_truncated,
                output.stderr_truncated,
            );
            stderr = output.stderr;
            fields
        } else {
            (
                HostCommandDispatch::OutcomeUncertain,
                None,
                false,
                false,
                false,
            )
        };
    if let Some(error) = transport_error {
        if !stderr.is_empty() {
            stderr.extend_from_slice(b"; ");
        }
        stderr.extend_from_slice(error.as_bytes());
    }
    if let Some(error) = authority_error {
        if !stderr.is_empty() {
            stderr.extend_from_slice(b"; ");
        }
        stderr.extend_from_slice(error.as_bytes());
    }
    HostCommandOutcome {
        dispatch,
        exit_code,
        timed_out,
        stdout_truncated,
        stderr_truncated,
        stderr,
        observed_connection_epoch: Some(connection_epoch),
    }
}
