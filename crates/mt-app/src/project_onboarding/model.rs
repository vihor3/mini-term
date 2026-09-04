use mt_config::SshConnection;

pub use crate::store::ProjectLocationKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OnboardingPage {
    Home,
    Clone,
    Create,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CreateMode {
    NewFolder,
    InitializeExisting,
}

#[derive(Clone, Debug)]
pub enum ProjectHostSelection {
    Local,
    Ssh {
        connection: SshConnection,
        connection_fingerprint: u64,
    },
}

impl ProjectHostSelection {
    pub fn signature(&self) -> HostSignature {
        match self {
            Self::Local => HostSignature::Local,
            Self::Ssh {
                connection,
                connection_fingerprint,
            } => HostSignature::Ssh {
                connection_id: connection.id.clone(),
                connection_fingerprint: *connection_fingerprint,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HostSignature {
    Local,
    Ssh {
        connection_id: String,
        connection_fingerprint: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostStatus {
    Ready { observed_epoch: Option<u64> },
    Connecting,
    NotConnected,
    Error(String),
}

impl HostStatus {
    pub fn observed_epoch(&self) -> Option<u64> {
        match self {
            Self::Ready { observed_epoch } => *observed_epoch,
            Self::Connecting | Self::NotConnected | Self::Error(_) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingErrorKind {
    Validation,
    Collision,
    GitUnavailable,
    GitFailure,
    Authentication,
    DisconnectedBeforeDispatch,
    RemoteOutcomeUncertain,
    PostconditionFailed,
    Registration,
    StaleOperation,
    GenerationOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnboardingError {
    pub kind: OnboardingErrorKind,
    pub message: String,
    pub authority: Option<OperationResultAuthority>,
}

impl OnboardingError {
    pub fn new(kind: OnboardingErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            authority: None,
        }
    }

    pub fn with_authority(mut self, authority: OperationResultAuthority) -> Self {
        self.authority = Some(authority);
        self
    }

    pub fn with_cleanup(mut self, cleanup: impl AsRef<str>) -> Self {
        let cleanup = cleanup.as_ref().trim();
        if !cleanup.is_empty() {
            self.message.push_str("; cleanup: ");
            self.message.push_str(cleanup);
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationPhase {
    Idle,
    Validating,
    Running,
    Success,
    Failure(OnboardingError),
}

impl OperationPhase {
    pub fn is_busy(&self) -> bool {
        matches!(self, Self::Validating | Self::Running)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationOwner {
    pub form_instance_id: u64,
    pub host_generation: u64,
    pub operation_id: u64,
    pub page: OnboardingPage,
    pub create_mode: Option<CreateMode>,
    pub host_signature: HostSignature,
    pub expected_connection_epoch: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationResultProvenance {
    Normal,
    PostconditionVerifiedAfterUncertainDispatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationResultAuthority {
    pub observed_connection_epoch: Option<u64>,
    pub provenance: OperationResultProvenance,
}

impl OperationResultAuthority {
    pub const fn normal(observed_connection_epoch: Option<u64>) -> Self {
        Self {
            observed_connection_epoch,
            provenance: OperationResultProvenance::Normal,
        }
    }

    pub const fn verified_after_uncertain_dispatch(observed_connection_epoch: Option<u64>) -> Self {
        Self {
            observed_connection_epoch,
            provenance: OperationResultProvenance::PostconditionVerifiedAfterUncertainDispatch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitRelationship {
    NotGit,
    RepositoryRoot {
        top_level: String,
        common_dir: String,
    },
    NestedInRepository {
        top_level: String,
        common_dir: String,
    },
}

impl GitRelationship {
    pub fn exact_root(&self) -> Option<(&str, &str)> {
        match self {
            Self::RepositoryRoot {
                top_level,
                common_dir,
            } => Some((top_level, common_dir)),
            Self::NotGit | Self::NestedInRepository { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostPathProbe {
    pub canonical_path: String,
    pub directory_empty: Option<bool>,
    pub git: GitRelationship,
    pub observed_connection_epoch: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetState {
    Absent { canonical_target: String },
    EmptyDirectory { canonical_target: String },
    NonEmptyDirectory { canonical_target: String },
    Other { canonical_target: String },
}

impl TargetState {
    pub fn canonical_target(&self) -> &str {
        match self {
            Self::Absent { canonical_target }
            | Self::EmptyDirectory { canonical_target }
            | Self::NonEmptyDirectory { canonical_target }
            | Self::Other { canonical_target } => canonical_target,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedProjectLocation {
    pub key: ProjectLocationKey,
    pub canonical_path: String,
    pub suggested_name: String,
    pub authority: OperationResultAuthority,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnboardingOperationResult {
    ReadyToRegister(VerifiedProjectLocation),
    NestedRepository {
        selected_path: String,
        repository_root: String,
        common_dir: String,
        authority: OperationResultAuthority,
    },
}

impl OnboardingOperationResult {
    pub fn authority(&self) -> OperationResultAuthority {
        match self {
            Self::ReadyToRegister(location) => location.authority,
            Self::NestedRepository { authority, .. } => *authority,
        }
    }
}

#[derive(Clone, Debug)]
pub struct OnboardingState {
    pub form_instance_id: u64,
    pub host_generation: u64,
    next_operation_id: u64,
    pub page: OnboardingPage,
    pub create_mode: CreateMode,
    pub host: ProjectHostSelection,
    pub host_status: HostStatus,
    pub phase: OperationPhase,
    pub active_owner: Option<OperationOwner>,
    terminal_failure: Option<OnboardingError>,
    pub closed: bool,
}

impl OnboardingState {
    pub fn new(form_instance_id: u64, host: ProjectHostSelection) -> Self {
        let host_status = match &host {
            ProjectHostSelection::Local => HostStatus::Ready {
                observed_epoch: None,
            },
            ProjectHostSelection::Ssh { .. } => HostStatus::NotConnected,
        };
        let terminal_failure = (form_instance_id == 0).then_some(OnboardingError::new(
            OnboardingErrorKind::GenerationOverflow,
            "onboarding form identity overflowed; restart the application",
        ));
        let phase = terminal_failure
            .clone()
            .map_or(OperationPhase::Idle, OperationPhase::Failure);
        Self {
            form_instance_id,
            host_generation: 0,
            next_operation_id: 0,
            page: OnboardingPage::Home,
            create_mode: CreateMode::NewFolder,
            host,
            host_status,
            phase,
            active_owner: None,
            terminal_failure,
            closed: false,
        }
    }

    pub fn switch_host(&mut self, host: ProjectHostSelection) -> Result<(), OnboardingError> {
        self.invalidate()?;
        self.host_status = match &host {
            ProjectHostSelection::Local => HostStatus::Ready {
                observed_epoch: None,
            },
            ProjectHostSelection::Ssh { .. } => HostStatus::NotConnected,
        };
        self.host = host;
        Ok(())
    }

    pub fn navigate(&mut self, page: OnboardingPage) -> Result<(), OnboardingError> {
        if self.page != page {
            self.invalidate()?;
            self.page = page;
        }
        Ok(())
    }

    pub fn switch_create_mode(&mut self, mode: CreateMode) -> Result<(), OnboardingError> {
        if self.create_mode != mode {
            self.invalidate()?;
            self.create_mode = mode;
        }
        Ok(())
    }

    pub fn set_host_status(&mut self, status: HostStatus) {
        self.host_status = status;
    }

    pub fn is_terminally_failed(&self) -> bool {
        self.terminal_failure.is_some()
    }

    pub fn begin_validation(&mut self) -> Result<Option<OperationOwner>, OnboardingError> {
        if let Some(error) = &self.terminal_failure {
            return Err(error.clone());
        }
        if self.phase.is_busy() || self.closed {
            return Ok(None);
        }
        if matches!(self.host, ProjectHostSelection::Ssh { .. })
            && !matches!(
                self.host_status,
                HostStatus::Ready {
                    observed_epoch: Some(_)
                }
            )
        {
            let error = OnboardingError::new(
                OnboardingErrorKind::DisconnectedBeforeDispatch,
                "connect the selected SSH host before starting this operation",
            );
            self.phase = OperationPhase::Failure(error.clone());
            self.active_owner = None;
            return Err(error);
        }
        self.next_operation_id = match checked_next(self.next_operation_id) {
            Ok(next) => next,
            Err(error) => {
                self.enter_terminal_failure(error.clone());
                return Err(error);
            }
        };
        let owner = OperationOwner {
            form_instance_id: self.form_instance_id,
            host_generation: self.host_generation,
            operation_id: self.next_operation_id,
            page: self.page,
            create_mode: (self.page == OnboardingPage::Create).then_some(self.create_mode),
            host_signature: self.host.signature(),
            expected_connection_epoch: self.host_status.observed_epoch(),
        };
        self.phase = OperationPhase::Validating;
        self.active_owner = Some(owner.clone());
        Ok(Some(owner))
    }

    pub fn mark_running(&mut self, owner: &OperationOwner) -> bool {
        if !self.owns(owner, owner.expected_connection_epoch)
            || self.phase != OperationPhase::Validating
        {
            return false;
        }
        self.phase = OperationPhase::Running;
        true
    }

    pub fn apply_success(
        &mut self,
        owner: &OperationOwner,
        observed_connection_epoch: Option<u64>,
    ) -> bool {
        if !self.owns_completion(owner, observed_connection_epoch) {
            return false;
        }
        self.phase = OperationPhase::Success;
        self.active_owner = None;
        true
    }

    pub fn apply_failure(
        &mut self,
        owner: &OperationOwner,
        observed_connection_epoch: Option<u64>,
        error: OnboardingError,
    ) -> bool {
        if !self.owns_completion(owner, observed_connection_epoch) {
            return false;
        }
        self.phase = OperationPhase::Failure(error);
        self.active_owner = None;
        true
    }

    pub fn apply_neutral_result(
        &mut self,
        owner: &OperationOwner,
        observed_connection_epoch: Option<u64>,
    ) -> bool {
        if !self.owns_completion(owner, observed_connection_epoch) {
            return false;
        }
        self.phase = OperationPhase::Idle;
        self.active_owner = None;
        true
    }

    pub fn close(&mut self) -> Result<(), OnboardingError> {
        let result = self.invalidate();
        self.closed = true;
        self.active_owner = None;
        result
    }

    pub fn owns_completion(
        &self,
        owner: &OperationOwner,
        observed_connection_epoch: Option<u64>,
    ) -> bool {
        self.owns(owner, observed_connection_epoch)
    }

    pub fn owns(&self, owner: &OperationOwner, observed_connection_epoch: Option<u64>) -> bool {
        self.owns_context(owner)
            && match &owner.host_signature {
                HostSignature::Local => observed_connection_epoch.is_none(),
                HostSignature::Ssh { .. } => {
                    owner.expected_connection_epoch == observed_connection_epoch
                        && self.host_status.observed_epoch() == observed_connection_epoch
                }
            }
    }

    pub fn reconcile_completion_owner(
        &mut self,
        owner: &OperationOwner,
        authority: OperationResultAuthority,
        current_connection_fingerprint: Option<u64>,
        current_connection_epoch: Option<u64>,
    ) -> Option<OperationOwner> {
        if !self.owns_context(owner) {
            return None;
        }

        match &owner.host_signature {
            HostSignature::Local => {
                if authority != OperationResultAuthority::normal(None)
                    || current_connection_fingerprint.is_some()
                    || current_connection_epoch.is_some()
                {
                    return None;
                }
                self.owns(owner, None).then_some(owner.clone())
            }
            HostSignature::Ssh {
                connection_fingerprint,
                ..
            } => {
                if current_connection_fingerprint != Some(*connection_fingerprint) {
                    return None;
                }
                let expected_epoch = owner.expected_connection_epoch?;
                let observed_epoch = authority.observed_connection_epoch?;
                if current_connection_epoch != Some(observed_epoch)
                    || self.host_status.observed_epoch() != Some(expected_epoch)
                {
                    return None;
                }
                if observed_epoch == expected_epoch {
                    return self
                        .owns(owner, Some(observed_epoch))
                        .then_some(owner.clone());
                }
                if authority.provenance
                    != OperationResultProvenance::PostconditionVerifiedAfterUncertainDispatch
                    || observed_epoch <= expected_epoch
                {
                    return None;
                }

                let mut reconciled = owner.clone();
                reconciled.expected_connection_epoch = Some(observed_epoch);
                self.host_status = HostStatus::Ready {
                    observed_epoch: Some(observed_epoch),
                };
                self.active_owner = Some(reconciled.clone());
                Some(reconciled)
            }
        }
    }

    fn owns_context(&self, owner: &OperationOwner) -> bool {
        !self.closed
            && self.active_owner.as_ref() == Some(owner)
            && owner.form_instance_id == self.form_instance_id
            && owner.host_generation == self.host_generation
            && owner.page == self.page
            && owner.create_mode
                == (self.page == OnboardingPage::Create).then_some(self.create_mode)
            && owner.host_signature == self.host.signature()
    }

    pub fn enter_terminal_failure(&mut self, error: OnboardingError) {
        debug_assert_eq!(error.kind, OnboardingErrorKind::GenerationOverflow);
        self.terminal_failure = Some(error.clone());
        self.phase = OperationPhase::Failure(error);
        self.active_owner = None;
    }

    fn invalidate(&mut self) -> Result<(), OnboardingError> {
        if let Some(error) = self.terminal_failure.clone() {
            self.phase = OperationPhase::Failure(error.clone());
            self.active_owner = None;
            return Err(error);
        }
        match checked_next(self.host_generation) {
            Ok(next) => {
                self.host_generation = next;
                self.phase = OperationPhase::Idle;
                self.active_owner = None;
                Ok(())
            }
            Err(error) => {
                self.enter_terminal_failure(error.clone());
                Err(error)
            }
        }
    }
}

pub fn checked_next(value: u64) -> Result<u64, OnboardingError> {
    value.checked_add(1).ok_or_else(|| {
        OnboardingError::new(
            OnboardingErrorKind::GenerationOverflow,
            "onboarding operation identity overflowed; reopen the form",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh(id: &str, fingerprint: u64) -> ProjectHostSelection {
        ProjectHostSelection::Ssh {
            connection: SshConnection {
                id: id.into(),
                name: id.into(),
                host: "example.com".into(),
                port: 22,
                user: "deploy".into(),
                password: None,
                identity_file: None,
                group: None,
            },
            connection_fingerprint: fingerprint,
        }
    }

    #[test]
    fn host_page_and_mode_changes_reject_stale_owner() {
        let mut state = OnboardingState::new(7, ssh("a", 1));
        state.set_host_status(HostStatus::Ready {
            observed_epoch: Some(10),
        });
        state.navigate(OnboardingPage::Create).unwrap();
        let owner = state.begin_validation().unwrap().unwrap();
        assert!(state.mark_running(&owner));

        state.switch_host(ssh("b", 2)).unwrap();
        state.switch_host(ssh("a", 1)).unwrap();
        state.set_host_status(HostStatus::Ready {
            observed_epoch: Some(10),
        });
        state.navigate(OnboardingPage::Create).unwrap();
        assert!(!state.apply_success(&owner, Some(10)));

        let next = state.begin_validation().unwrap().unwrap();
        state
            .switch_create_mode(CreateMode::InitializeExisting)
            .unwrap();
        assert!(!state.apply_failure(
            &next,
            Some(10),
            OnboardingError::new(OnboardingErrorKind::Validation, "old")
        ));
    }

    #[test]
    fn only_current_owner_can_change_or_clear_busy_state() {
        let mut state = OnboardingState::new(9, ProjectHostSelection::Local);
        state.navigate(OnboardingPage::Clone).unwrap();
        let owner = state.begin_validation().unwrap().unwrap();
        assert!(state.begin_validation().unwrap().is_none());

        let mut stale = owner.clone();
        stale.operation_id += 1;
        assert!(!state.mark_running(&stale));
        assert_eq!(state.phase, OperationPhase::Validating);
        assert!(state.mark_running(&owner));
        assert!(!state.apply_failure(
            &stale,
            None,
            OnboardingError::new(OnboardingErrorKind::Validation, "stale")
        ));
        assert_eq!(state.phase, OperationPhase::Running);
        assert!(state.apply_success(&owner, None));
    }

    #[test]
    fn invalid_form_identity_is_a_persistent_terminal_failure() {
        let mut state = OnboardingState::new(0, ProjectHostSelection::Local);

        let initial = match &state.phase {
            OperationPhase::Failure(error) => error.clone(),
            phase => panic!("expected terminal failure, got {phase:?}"),
        };
        assert_eq!(initial.kind, OnboardingErrorKind::GenerationOverflow);
        assert!(state.navigate(OnboardingPage::Clone).is_err());
        assert_eq!(state.page, OnboardingPage::Home);
        assert_eq!(state.begin_validation().unwrap_err(), initial);
    }

    #[test]
    fn neutral_result_requires_the_exact_owner_and_an_authenticated_epoch() {
        let mut state = OnboardingState::new(10, ssh("host", 7));
        state.set_host_status(HostStatus::Ready {
            observed_epoch: Some(20),
        });
        state.navigate(OnboardingPage::Create).unwrap();
        state
            .switch_create_mode(CreateMode::InitializeExisting)
            .unwrap();
        let owner = state.begin_validation().unwrap().unwrap();
        assert!(state.mark_running(&owner));

        let mut stale = owner.clone();
        stale.operation_id += 1;
        assert!(!state.apply_neutral_result(&stale, Some(20)));
        assert!(!state.apply_neutral_result(&owner, None));
        assert_eq!(state.phase, OperationPhase::Running);
        assert_eq!(state.active_owner.as_ref(), Some(&owner));

        assert!(!state.apply_neutral_result(&owner, Some(21)));
        assert_eq!(state.phase, OperationPhase::Running);
        assert_eq!(state.active_owner.as_ref(), Some(&owner));

        assert!(state.apply_neutral_result(&owner, Some(20)));
        assert_eq!(state.phase, OperationPhase::Idle);
        assert!(state.active_owner.is_none());
    }

    #[test]
    fn ssh_validation_requires_a_ready_authenticated_epoch() {
        let mut state = OnboardingState::new(12, ssh("host", 7));

        let error = state.begin_validation().unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::DisconnectedBeforeDispatch);
        assert_eq!(state.phase, OperationPhase::Failure(error));
        assert!(state.active_owner.is_none());

        state.set_host_status(HostStatus::Ready {
            observed_epoch: Some(20),
        });
        assert!(state.begin_validation().unwrap().is_some());
    }

    #[test]
    fn fingerprint_epoch_and_form_identity_are_all_required() {
        let mut state = OnboardingState::new(11, ssh("host", 7));
        state.set_host_status(HostStatus::Ready {
            observed_epoch: Some(20),
        });
        state.navigate(OnboardingPage::Clone).unwrap();
        let owner = state.begin_validation().unwrap().unwrap();

        assert!(!state.owns(&owner, Some(19)));
        state.host = ssh("host", 8);
        assert!(!state.owns(&owner, Some(20)));
        state.host = ssh("host", 7);
        state.form_instance_id = 12;
        assert!(!state.owns(&owner, Some(20)));
        assert_eq!(state.phase, OperationPhase::Validating);
    }

    #[test]
    fn normal_ssh_completion_rejects_a_new_current_epoch() {
        let mut state = OnboardingState::new(12, ssh("host", 7));
        state.set_host_status(HostStatus::Ready {
            observed_epoch: Some(20),
        });
        state.navigate(OnboardingPage::Clone).unwrap();
        let owner = state.begin_validation().unwrap().unwrap();
        assert!(state.mark_running(&owner));

        let reconciled = state.reconcile_completion_owner(
            &owner,
            OperationResultAuthority::normal(Some(21)),
            Some(7),
            Some(21),
        );

        assert!(reconciled.is_none());
        assert_eq!(state.active_owner.as_ref(), Some(&owner));
        assert_eq!(
            state.host_status,
            HostStatus::Ready {
                observed_epoch: Some(20)
            }
        );
    }

    #[test]
    fn verified_uncertain_postcondition_accepts_a_new_current_epoch() {
        let mut state = OnboardingState::new(13, ssh("host", 7));
        state.set_host_status(HostStatus::Ready {
            observed_epoch: Some(20),
        });
        state.navigate(OnboardingPage::Clone).unwrap();
        let owner = state.begin_validation().unwrap().unwrap();
        assert!(state.mark_running(&owner));

        let reconciled = state
            .reconcile_completion_owner(
                &owner,
                OperationResultAuthority::verified_after_uncertain_dispatch(Some(21)),
                Some(7),
                Some(21),
            )
            .expect("verified recovery should reconcile to the fresh current epoch");

        assert_eq!(reconciled.expected_connection_epoch, Some(21));
        assert_eq!(state.active_owner.as_ref(), Some(&reconciled));
        assert_eq!(
            state.host_status,
            HostStatus::Ready {
                observed_epoch: Some(21)
            }
        );
        assert!(state.apply_success(&reconciled, Some(21)));
    }

    #[test]
    fn operation_id_overflow_enters_visible_failure_state() {
        let mut state = OnboardingState::new(14, ProjectHostSelection::Local);
        state.next_operation_id = u64::MAX;

        let error = state.begin_validation().unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::GenerationOverflow);
        assert_eq!(state.phase, OperationPhase::Failure(error));
        assert!(state.active_owner.is_none());
        let retry = state.begin_validation().unwrap_err();
        assert_eq!(retry.kind, OnboardingErrorKind::GenerationOverflow);
        assert!(state.navigate(OnboardingPage::Clone).is_err());
        assert_eq!(state.page, OnboardingPage::Home);
    }

    #[test]
    fn generation_overflow_does_not_apply_the_requested_transition() {
        let mut state = OnboardingState::new(16, ProjectHostSelection::Local);
        state.host_generation = u64::MAX;

        let error = state.switch_host(ssh("remote", 1)).unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::GenerationOverflow);
        assert!(matches!(state.host, ProjectHostSelection::Local));
        assert_eq!(state.phase, OperationPhase::Failure(error));
        assert!(state.active_owner.is_none());
        let retry = state.begin_validation().unwrap_err();
        assert_eq!(retry.kind, OnboardingErrorKind::GenerationOverflow);
    }

    #[test]
    fn close_remains_terminal_when_generation_overflows() {
        let mut state = OnboardingState::new(18, ProjectHostSelection::Local);
        state.navigate(OnboardingPage::Clone).unwrap();
        let owner = state.begin_validation().unwrap().unwrap();
        state.host_generation = u64::MAX;

        let error = state.close().unwrap_err();

        assert_eq!(error.kind, OnboardingErrorKind::GenerationOverflow);
        assert!(state.closed);
        assert!(state.active_owner.is_none());
        assert_eq!(state.phase, OperationPhase::Failure(error));
        assert!(!state.apply_success(&owner, None));
    }
}
