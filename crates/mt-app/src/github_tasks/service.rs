use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{App, Context, Entity};
use mt_config::{TasksAccountScope, TasksAccountSelection};
use mt_github::{AccountError, GitHubAccountIdentity, WorkItemKind};

use super::github_project_tasks_enabled;
use super::model::*;
use super::pipeline::{
    HostExecutor, PipelineCompletion, PreparedAccounts, ReadData, ReadSuccess, ReadTarget,
    prepare_with, read_with,
};
use crate::execution_host::{ExecutionSourceSignature, ProjectExecutionSnapshot};
use crate::store::AppStore;
use crate::tasks_account_executor::{AccountCancellation, AccountExecutionError};

/// Only identities, sanitized account states, and data results enter this owner.
pub struct GitHubTaskService {
    pub(super) store: Entity<AppStore>,
    sources: HashMap<ExecutionSourceSignature, SourceRecord>,
    auth_scopes: HashMap<TasksAccountScope, u64>,
    repositories: HashMap<RepositoryCacheKey, RepositoryCache>,
    requests: HashMap<u64, RequestOwner>,
    next_id: u64,
}

impl Drop for GitHubTaskService {
    fn drop(&mut self) {
        for request in self.requests.values() {
            request.cancellation.cancel();
        }
    }
}

pub(super) fn preparation_scope_matches(
    scope: &TasksAccountScope,
    captured_generations: &HashMap<TasksAccountScope, u64>,
    current_generations: &HashMap<TasksAccountScope, u64>,
    captured_choices: &[TasksAccountSelection],
    current_choices: &[TasksAccountSelection],
) -> bool {
    captured_generations.get(scope) == current_generations.get(scope)
        && captured_choices.iter().filter(|entry| entry.scope == *scope)
            .eq(current_choices.iter().filter(|entry| entry.scope == *scope))
}

fn preparation_source_matches(snapshot: &ProjectExecutionSnapshot,
    generations: &HashMap<TasksAccountScope, u64>, current_generations: &HashMap<TasksAccountScope, u64>,
    choices: &[TasksAccountSelection], current_choices: &[TasksAccountSelection]) -> bool {
    let execution = account_scope(snapshot, "");
    let same_execution = |scope: &TasksAccountScope| scope.root_project_id == execution.root_project_id
        && scope.execution_host_id == execution.execution_host_id && scope.backend == execution.backend;
    generations.iter().filter(|(scope, _)| same_execution(scope))
        .all(|(scope, generation)| current_generations.get(scope) == Some(generation))
        && current_generations.iter().filter(|(scope, _)| same_execution(scope))
            .all(|(scope, generation)| generations.get(scope) == Some(generation))
        && choices.iter().filter(|entry| same_execution(&entry.scope))
            .eq(current_choices.iter().filter(|entry| same_execution(&entry.scope)))
}

fn current_source_matches(store: &AppStore, snapshot: &ProjectExecutionSnapshot,
    source: &ExecutionSourceSignature) -> bool {
    store.project_execution_snapshot(&snapshot.project_id)
        .is_ok_and(|current| current.source_signature() == *source)
}

fn now_unix_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok()).unwrap_or_default()
}

pub(super) fn invalidates_account(error: &TasksError) -> bool {
    matches!(error, TasksError::Account(AccountExecutionError::Account(
        AccountError::AuthRequired | AccountError::SelectedAccountUnavailable
        | AccountError::CredentialStoreUnavailable | AccountError::CredentialLookupFailed
        | AccountError::AuthenticationFailed | AccountError::WrongHostOrAccount
        | AccountError::InheritedAuthentication
    )))
}

pub(super) fn apply_list_completion(slot: &mut ListSlot, owner: &RequestOwner,
    result: Result<ReadSuccess, TasksError>) -> bool {
    if !owns_slot(slot.request_id, slot.loading, owner) { return false; }
    slot.loading = false;
    match result {
        Ok(ReadSuccess { data: ReadData::List(rows), .. }) => {
            slot.rows = rows; slot.error = None; slot.updated_at_unix_ms = Some(now_unix_ms());
        }
        Err(error) => {
            if !error.retains_last_known() { slot.rows.clear(); slot.updated_at_unix_ms = None; }
            slot.error = Some(error);
        }
        _ => {
            slot.rows.clear(); slot.updated_at_unix_ms = None;
            slot.error = Some(TasksError::changed());
        }
    }
    true
}

pub(super) fn apply_detail_completion(slot: &mut DetailSlot, owner: &RequestOwner,
    result: Result<ReadSuccess, TasksError>) -> bool {
    if !owns_slot(slot.request_id, slot.loading, owner) { return false; }
    slot.loading = false;
    match result {
        Ok(ReadSuccess { data: ReadData::Detail(detail), .. }) => {
            slot.detail = Some(detail); slot.error = None;
        }
        Err(error) => {
            if !error.retains_last_known() { slot.detail = None; }
            slot.error = Some(error);
        }
        _ => { slot.detail = None; slot.error = Some(TasksError::changed()); }
    }
    true
}

pub(super) fn apply_source_completion(source: &mut SourceRecord, target: ReadTarget,
    accounts: Option<mt_github::KnownGitHubAccounts>, error: Option<TasksError>) -> Option<TasksError> {
    if let Some(accounts) = accounts { source.accounts = Some(accounts); }
    let invalidated = error.as_ref().filter(|error| !error.retains_last_known()
        && !(matches!(target, ReadTarget::Detail(_, _))
            && matches!(error, TasksError::Account(AccountExecutionError::Account(AccountError::NotFound)))))
        .cloned();
    if matches!(target, ReadTarget::List(_)) {
        source.phase = if error.is_none() { SourcePhase::Ready } else { SourcePhase::Error };
        source.error = error;
    } else if invalidated.is_some() {
        source.phase = SourcePhase::Error;
        source.error = error;
    }
    // A prior list/source error is not evidence against a newly proved detail.
    invalidated
}

pub(super) fn detail_slot_view(slot: Option<&DetailSlot>, access: &RequestOwner) -> GitHubDetailView {
    let slot = slot.filter(|slot| slot.request_id == access.request_id && !access.cancellation.is_cancelled());
    GitHubDetailView { detail: slot.and_then(|slot| slot.detail.clone()),
        loading: slot.is_some_and(|slot| slot.loading),
        error: slot.map_or_else(|| Some(TasksError::changed()), |slot| slot.error.clone()) }
}

pub(super) fn list_request_ids(source: &ExecutionSourceSignature,
    requests: &HashMap<u64, RequestOwner>, repositories: &HashMap<RepositoryCacheKey, RepositoryCache>) -> Vec<u64> {
    requests.values().filter(|owner| owner.source == *source && owner.cache_key.as_ref().is_none_or(|key| {
        repositories.get(key).is_some_and(|cache| cache.lists.values()
            .any(|slot| slot.request_id == owner.request_id && slot.loading))
    })).map(|owner| owner.request_id).collect()
}

pub(super) fn rotate_scope_state(
    scope: &TasksAccountScope, generation: u64,
    auth_scopes: &mut HashMap<TasksAccountScope, u64>,
    requests: &mut HashMap<u64, RequestOwner>,
    sources: &mut HashMap<ExecutionSourceSignature, SourceRecord>,
    repositories: &mut HashMap<RepositoryCacheKey, RepositoryCache>,
) {
    let old_generation = auth_scopes.insert(scope.clone(), generation);
    requests.retain(|_, owner| {
        if owner.scope.as_ref() == Some(scope) { owner.cancellation.cancel(); false } else { true }
    });
    if let Some(old_generation) = old_generation {
        repositories.retain(|key, _| key.auth_generation != old_generation);
    }
    for source in sources.values_mut().filter(|source| source.scope.as_ref() == Some(scope)) {
        source.invalidate(generation, true);
        source.auth_generation = generation;
    }
}

impl GitHubTaskService {
    pub fn new(store: Entity<AppStore>) -> Self {
        Self { store, sources: HashMap::new(), auth_scopes: HashMap::new(),
            repositories: HashMap::new(), requests: HashMap::new(), next_id: 0 }
    }

    fn allocate_id(&mut self) -> u64 {
        // Never wrap an authority token back onto an old request.
        self.next_id = self.next_id.checked_add(1).expect("Tasks request identity exhausted");
        self.next_id
    }

    fn generation(&mut self, scope: &TasksAccountScope) -> u64 {
        if let Some(generation) = self.auth_scopes.get(scope) { return *generation; }
        let generation = self.allocate_id();
        self.auth_scopes.insert(scope.clone(), generation);
        generation
    }

    fn cancel_request(&mut self, request_id: u64) {
        let Some(owner) = self.requests.remove(&request_id) else { return; };
        owner.cancellation.cancel();
        if let Some(key) = owner.cache_key.as_ref()
            && let Some(cache) = self.repositories.get_mut(key) {
            for slot in cache.lists.values_mut() {
                if slot.request_id == request_id {
                    slot.loading = false;
                    slot.error = Some(TasksError::changed());
                }
            }
            for slot in cache.details.values_mut() {
                if slot.request_id == request_id {
                    slot.loading = false;
                    slot.error = Some(TasksError::changed());
                }
            }
        }
        if let Some(source) = self.sources.get_mut(&owner.source)
            && source.request_id == owner.source_request && source.phase == SourcePhase::Loading {
            source.phase = SourcePhase::Idle;
        }
    }

    pub(super) fn suspend_source(&mut self, source: &ExecutionSourceSignature) {
        let requests = self.requests.values().filter(|owner| owner.source == *source)
            .map(|owner| owner.request_id).collect::<Vec<_>>();
        for request in requests { self.cancel_request(request); }
        let id = self.allocate_id();
        if let Some(record) = self.sources.get_mut(source) { record.invalidate(id, false); }
    }

    fn rotate_scope(&mut self, scope: &TasksAccountScope) -> u64 {
        let generation = self.allocate_id();
        rotate_scope_state(scope, generation, &mut self.auth_scopes, &mut self.requests,
            &mut self.sources, &mut self.repositories);
        generation
    }

    /// Store notifications invalidate changed configuration/epochs even if a
    /// detail tab, rather than Tasks itself, is currently visible.
    pub(super) fn reconcile_sources(&mut self, cx: &mut Context<Self>) {
        let mut stale = Vec::new();
        let mut changed_choices = Vec::new();
        for (key, source) in &self.sources {
            let current = self.store.read(cx).project_execution_snapshot(&source.snapshot.project_id);
            if !current.as_ref().is_ok_and(|snapshot| snapshot.source_signature() == *key) {
                let initial_connection = source.phase == SourcePhase::Loading
                    && self.requests.values().any(|owner| owner.source == *key && owner.cache_key.is_none())
                    && current.as_ref().is_ok_and(|snapshot| {
                        snapshot.source_signature().with_connection_epoch(None)
                            == key.with_connection_epoch(None)
                    });
                if !initial_connection { stale.push(key.clone()); }
                continue;
            }
            if source.phase != SourcePhase::Idle
                && let Some(scope) = source.scope.as_ref()
                && saved_selection(self.store.read(cx).config(), scope).ok().flatten() != source.selected {
                changed_choices.push(scope.clone());
            }
        }
        let changed = !stale.is_empty() || !changed_choices.is_empty();
        for source in stale {
            self.suspend_source(&source);
            self.sources.remove(&source);
        }
        for scope in changed_choices { self.rotate_scope(&scope); }
        if changed { cx.notify(); }
    }

    pub(super) fn ensure_list(&mut self, snapshot: ProjectExecutionSnapshot, kind: WorkItemKind,
        force: bool, cx: &mut Context<Self>) {
        if !github_project_tasks_enabled() { return; }
        let key = snapshot.source_signature();
        if !current_source_matches(self.store.read(cx), &snapshot, &key) { return; }
        if force {
            self.access_list(snapshot, kind, true, cx);
            return;
        }
        let Some((access_id, kind)) = self.sources.get_mut(&key).and_then(SourceRecord::take_list_access) else { return; };
        if self.sources[&key].cache_key.is_some() {
            if self.start_read(key.clone(), ReadTarget::List(kind), cx).is_none() {
                self.suspend_source(&key);
                let source = self.sources.get_mut(&key).expect("access source");
                source.cache_key = None;
                source.accounts = None;
                let request_id = source.request_id;
                self.start_preparation(snapshot, kind, request_id, cx);
            }
        } else {
            self.start_preparation(snapshot, kind, access_id, cx);
        }
    }

    pub(super) fn access_list(&mut self, snapshot: ProjectExecutionSnapshot, kind: WorkItemKind,
        force: bool, cx: &mut Context<Self>) {
        if !github_project_tasks_enabled() { return; }
        let key = snapshot.source_signature();
        if !current_source_matches(self.store.read(cx), &snapshot, &key) { return; }
        if force && let Some(scope) = self.sources.get(&key).and_then(|record| record.scope.clone()) {
            self.rotate_scope(&scope);
        }
        for request in list_request_ids(&key, &self.requests, &self.repositories) {
            self.cancel_request(request);
        }
        let access_id = self.allocate_id();
        if !self.sources.contains_key(&key) {
            self.sources.insert(key.clone(), SourceRecord::new(snapshot.clone(), access_id));
        }
        self.sources.get_mut(&key).expect("access source").begin_list_access(access_id, kind);
        self.ensure_list(snapshot, kind, false, cx);
    }

    fn start_preparation(&mut self, snapshot: ProjectExecutionSnapshot, kind: WorkItemKind,
        request_id: u64, cx: &mut Context<Self>) {
        let key = snapshot.source_signature();
        let selected = self.sources[&key].scope.as_ref()
            .and_then(|scope| saved_selection(self.store.read(cx).config(), scope).ok().flatten());
        let record = self.sources.get_mut(&key).expect("preparation source");
        record.phase = SourcePhase::Loading;
        record.error = None;
        record.request_id = request_id;
        record.snapshot = snapshot.clone();
        record.selected = selected;
        let owner = RequestOwner { source: key, source_request: request_id, request_id,
            scope: record.scope.clone(), cache_key: None, cancellation: AccountCancellation::default() };
        self.requests.insert(request_id, owner.clone());
        let generations = self.auth_scopes.clone();
        let choices = self.store.read(cx).config().tasks_account_selections.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let cancellation = owner.cancellation.clone();
            let result = cx.background_executor().spawn(async move {
                prepare_with(&snapshot, &cancellation, &mut HostExecutor)
            }).await;
            let _ = this.update(cx, |service, cx| {
                service.finish_preparation(owner, kind, generations, choices, result, cx);
            });
        }).detach();
    }

    fn finish_preparation(&mut self, owner: RequestOwner, kind: WorkItemKind,
        generations: HashMap<TasksAccountScope, u64>, choices: Vec<TasksAccountSelection>,
        completion: PipelineCompletion<PreparedAccounts>, cx: &mut Context<Self>) {
        if !self.requests.contains_key(&owner.request_id) { return; }
        if !owns_request(self.sources.get(&owner.source), &owner) {
            self.cancel_request(owner.request_id);
            cx.notify();
            return;
        }
        let snapshot = self.sources[&owner.source].snapshot.clone();
        let scope = completion.repository.as_ref().map(|repo| account_scope(&snapshot, repo.host()));
        let current_choices = &self.store.read(cx).config().tasks_account_selections;
        let scope_current = match scope.as_ref() {
            Some(scope) => preparation_scope_matches(scope, &generations, &self.auth_scopes, &choices, current_choices),
            None => preparation_source_matches(&snapshot, &generations, &self.auth_scopes, &choices, current_choices),
        };
        if !current_source_matches(self.store.read(cx), &snapshot, &completion.observed_source)
            || !scope_current
            || matches!(&completion.result, Err(TasksError::Account(AccountExecutionError::ContextChanged))) {
            self.suspend_source(&owner.source);
            cx.notify();
            return;
        }
        self.requests.remove(&owner.request_id);
        let Some(mut record) = self.sources.remove(&owner.source) else { return; };
        // A newer request on the adopted epoch must never be overwritten.
        if self.sources.get(&completion.observed_source)
            .is_some_and(|other| other.request_id > owner.source_request) { return; }
        record.snapshot = self.store.read(cx).project_execution_snapshot(&snapshot.project_id)
            .expect("validated Tasks source");
        record.repository = completion.repository;
        record.scope = scope.clone();
        let mut start_read = false;
        if let Some(scope) = scope.as_ref() {
            record.auth_generation = self.generation(scope);
            let saved = saved_selection(self.store.read(cx).config(), scope);
            record.selected = saved.as_ref().ok().cloned().flatten();
            match completion.result {
                Ok(prepared) => {
                    record.repository = Some(prepared.repository.clone());
                    let resolved = saved.and_then(|saved| resolve_selection(&prepared.accounts, saved.as_ref()));
                    record.accounts = Some(prepared.accounts);
                    match resolved {
                        Ok(account) => {
                            if record.selected.is_none() {
                                let scope = scope.clone();
                                let identity = account.identity().clone();
                                self.store.update(cx, |store, cx| {
                                    store.patch_config(|config| write_selection(config, scope, &identity), cx);
                                });
                            }
                            record.selected = Some(account.identity().clone());
                            record.cache_key = Some(RepositoryCacheKey::new(&completion.observed_source,
                                prepared.repository, account.identity().login().to_string(), record.auth_generation));
                            record.phase = SourcePhase::Loading;
                            record.error = None;
                            start_read = true;
                        }
                        Err(error) => {
                            record.cache_key = None;
                            record.phase = SourcePhase::Choosing;
                            record.error = Some(error);
                        }
                    }
                }
                Err(error) => { record.phase = SourcePhase::Error; record.error = Some(error); }
            }
        } else {
            record.phase = SourcePhase::Error;
            record.error = completion.result.err();
            record.cache_key = None;
        }
        let source = completion.observed_source;
        self.sources.insert(source.clone(), record);
        if start_read { let _ = self.start_read(source, ReadTarget::List(kind), cx); }
        cx.notify();
    }

    fn start_read(&mut self, source_key: ExecutionSourceSignature, target: ReadTarget,
        cx: &mut Context<Self>) -> Option<RequestOwner> {
        let Some(source) = self.sources.get(&source_key) else { return None; };
        let (Some(cache_key), Some(scope), Some(identity)) =
            (source.cache_key.clone(), source.scope.clone(), source.selected.clone()) else { return None; };
        if !current_source_matches(self.store.read(cx), &source.snapshot, &source_key)
            || saved_selection(self.store.read(cx).config(), &scope).ok().flatten() != Some(identity.clone())
            || self.auth_scopes.get(&scope) != Some(&cache_key.auth_generation) { return None; }
        let snapshot = source.snapshot.clone();
        let source_request = source.request_id;
        let previous = self.repositories.get(&cache_key).and_then(|cache| match target {
            ReadTarget::List(kind) => cache.lists.get(&kind).filter(|slot| slot.loading).map(|slot| slot.request_id),
            ReadTarget::Detail(kind, number) => cache.details.get(&(kind, number))
                .filter(|slot| slot.loading).map(|slot| slot.request_id),
        });
        if let Some(previous) = previous { self.cancel_request(previous); }
        let request_id = self.allocate_id();
        let cache = self.repositories.entry(cache_key.clone()).or_default();
        match target {
            ReadTarget::List(kind) => {
                let slot = cache.lists.entry(kind).or_default();
                slot.loading = true; slot.error = None; slot.request_id = request_id;
            }
            ReadTarget::Detail(kind, number) => {
                let slot = cache.details.entry((kind, number)).or_default();
                slot.begin_access(request_id);
            }
        }
        let owner = RequestOwner { source: source_key, source_request, request_id, scope: Some(scope),
            cache_key: Some(cache_key.clone()), cancellation: AccountCancellation::default() };
        self.requests.insert(request_id, owner.clone());
        let access = owner.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let cancellation = owner.cancellation.clone();
            let result = cx.background_executor().spawn(async move {
                read_with(&snapshot, &cache_key.repository, &identity, target, &cancellation, &mut HostExecutor)
            }).await;
            let _ = this.update(cx, |service, cx| service.finish_read(owner, target, result, cx));
        }).detach();
        Some(access)
    }

    fn finish_read(&mut self, owner: RequestOwner, target: ReadTarget,
        completion: PipelineCompletion<ReadSuccess>, cx: &mut Context<Self>) {
        if !self.requests.contains_key(&owner.request_id) { return; }
        if !owns_request(self.sources.get(&owner.source), &owner) {
            self.cancel_request(owner.request_id);
            cx.notify();
            return;
        }
        let source = &self.sources[&owner.source];
        let scope = owner.scope.as_ref().expect("account request scope");
        let cache_key = owner.cache_key.as_ref().expect("account request cache");
        if completion.observed_source != owner.source
            || !current_source_matches(self.store.read(cx), &source.snapshot, &owner.source)
            || self.auth_scopes.get(scope) != Some(&cache_key.auth_generation)
            || saved_selection(self.store.read(cx).config(), scope).ok().flatten() != source.selected {
            self.cancel_request(owner.request_id);
            cx.notify();
            return;
        }
        self.requests.remove(&owner.request_id);
        if let Err(error) = &completion.result
            && invalidates_account(error) {
            let error = error.clone();
            let accounts = source.accounts.clone();
            self.rotate_scope(scope);
            if let Some(source) = self.sources.get_mut(&owner.source) {
                source.phase = SourcePhase::Error;
                source.error = Some(error);
                source.accounts = accounts;
            }
            cx.notify();
            return;
        }
        let Some(cache) = self.repositories.get_mut(cache_key) else { return; };
        let error = completion.result.as_ref().err().cloned();
        let accounts = completion.result.as_ref().ok().map(|success| success.accounts.clone());
        match target {
            ReadTarget::List(kind) => {
                let Some(slot) = cache.lists.get_mut(&kind) else { return; };
                if !apply_list_completion(slot, &owner, completion.result) { return; }
            }
            ReadTarget::Detail(kind, number) => {
                let Some(slot) = cache.details.get_mut(&(kind, number)) else { return; };
                if !apply_detail_completion(slot, &owner, completion.result) { return; }
            }
        }
        let invalidated = self.sources.get_mut(&owner.source)
            .and_then(|source| apply_source_completion(source, target, accounts, error));
        if let Some(error) = invalidated {
            self.suspend_source(&owner.source);
            if let Some(source) = self.sources.get_mut(&owner.source) {
                source.phase = SourcePhase::Error;
                source.error = Some(error);
            }
        }
        cx.notify();
    }

    pub(super) fn select_account(&mut self, snapshot: ProjectExecutionSnapshot, view: &GitHubListView,
        identity: GitHubAccountIdentity, kind: WorkItemKind, cx: &mut Context<Self>) {
        if !github_project_tasks_enabled() || snapshot.source_signature() != view.source
            || !current_source_matches(self.store.read(cx), &snapshot, &view.source) { return; }
        let Some(source) = self.sources.get(&view.source) else { return; };
        if source.request_id != view.source_request || source.repository != view.repository
            || source.list_access_id != view.list_access_id
            || source.auth_generation != view.auth_generation
            || source.accounts.as_ref().is_none_or(|accounts| accounts.find(&identity).is_err()) { return; }
        let Some(scope) = source.scope.clone() else { return; };
        if identity.host() != scope.github_host { return; }
        // An explicitly chosen broken row remains chosen and reports its own error.
        self.store.update(cx, |store, cx| {
            store.patch_config(|config| write_selection(config, scope.clone(), &identity), cx);
        });
        self.rotate_scope(&scope);
        self.access_list(snapshot, kind, false, cx);
        cx.notify();
    }

    pub(super) fn list_view(&self, snapshot: &ProjectExecutionSnapshot, kind: WorkItemKind, cx: &App) -> GitHubListView {
        let key = snapshot.source_signature();
        let mut view = GitHubListView { host_label: snapshot.host_label.clone(), repository: None,
            account: None, accounts: None, source: key.clone(), source_request: 0, auth_generation: 0,
            list_access_id: 0,
            rows: Vec::new(), loading: false, error: None, updated_at_unix_ms: None, interactive: false };
        let Some(source) = self.sources.get(&key) else { return view; };
        view.repository = source.repository.clone();
        view.account = source.selected.as_ref().map(|identity| identity.login().to_string());
        view.accounts = source.accounts.clone();
        view.source_request = source.request_id;
        view.list_access_id = source.list_access_id;
        view.auth_generation = source.auth_generation;
        view.loading = source.phase == SourcePhase::Loading;
        view.error = source.error.clone();
        if source.scope.as_ref().is_some_and(|scope| {
            self.auth_scopes.get(scope) != Some(&source.auth_generation)
                || saved_selection(self.store.read(cx).config(), scope).ok().flatten() != source.selected
        }) {
            view.error = Some(TasksError::changed());
            view.loading = false;
            return view;
        }
        if let Some(cache_key) = source.cache_key.as_ref()
            && source.error.as_ref().is_none_or(TasksError::retains_last_known)
            && let Some(slot) = self.repositories.get(cache_key).and_then(|cache| cache.lists.get(&kind)) {
            view.rows = slot.rows.clone();
            view.loading |= slot.loading;
            view.error = view.error.or(slot.error.clone());
            view.updated_at_unix_ms = slot.updated_at_unix_ms;
            view.interactive = source_is_ready(Some(source), cache_key)
                && !view.loading && view.error.is_none() && view.updated_at_unix_ms.is_some();
        }
        view
    }

    pub(super) fn row_is_current(&self, snapshot: &ProjectExecutionSnapshot, view: &GitHubListView,
        kind: WorkItemKind, cx: &App) -> bool {
        let current = self.list_view(snapshot, kind, cx);
        current_source_matches(self.store.read(cx), snapshot, &view.source)
            && current.interactive && current.source == view.source && current.source_request == view.source_request
            && current.list_access_id == view.list_access_id
            && current.repository == view.repository && current.account == view.account
            && current.auth_generation == view.auth_generation
    }

    pub(super) fn access_detail(&mut self, request: OpenGitHubWorkItem, cx: &mut Context<Self>) -> Option<RequestOwner> {
        if !github_project_tasks_enabled() { return None; }
        let key = request.repository_cache_key();
        if !source_has_identity(self.sources.get(&request.source), &key) { return None; }
        self.start_read(request.source, ReadTarget::Detail(request.summary.kind, request.summary.number), cx)
    }

    pub(super) fn detail_view(&self, request: &OpenGitHubWorkItem, access: Option<&RequestOwner>, cx: &App) -> GitHubDetailView {
        let key = request.repository_cache_key();
        let source = self.sources.get(&request.source);
        let current = source.is_some_and(|source| {
            current_source_matches(self.store.read(cx), &source.snapshot, &request.source)
                && source.scope.as_ref().is_some_and(|scope| {
                    self.auth_scopes.get(scope) == Some(&request.auth_generation)
                        && saved_selection(self.store.read(cx).config(), scope).ok().flatten() == source.selected
                })
        });
        if !current || !source_has_identity(source, &key)
            || !access.is_some_and(|access| owns_request(source, access)) {
            return GitHubDetailView { detail: None, loading: false,
                error: Some(source.and_then(|source| source.error.clone()).unwrap_or_else(TasksError::changed)) };
        }
        let slot = self.repositories.get(&key).and_then(|cache| cache.details.get(&(request.summary.kind, request.summary.number)));
        detail_slot_view(slot, access.expect("validated detail access"))
    }
}
