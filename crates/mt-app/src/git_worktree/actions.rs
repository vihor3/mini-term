use super::*;
use crate::git_backend::{GitWritePhase, GitWriteState, UncertainReview};
use crate::remote_directory_picker::DirectoryPickerOptions;
use crate::project_onboarding::ProjectHostSelection;
use crate::store::GitWorktreeRemovalGuard;

const STALE: &str = "Worktree source or dialog changed. Review the current target before trying again.";

fn owned(state: &WorktreeModal, request: u64, cx: &App) -> bool {
    state.is_live(cx) && state.write_request == request
}

fn begin_manager_close(view_lifetime: &GitLifetime, is_top: bool) -> bool {
    if !view_lifetime.is_valid() || !is_top { return false; }
    view_lifetime.invalidate();
    true
}

fn close_manager(state: &Entity<WorktreeModal>, window: &mut Window, cx: &mut App) {
    if !begin_manager_close(&state.read(cx).view_lifetime, crate::overlay::is_top(crate::overlay::key(kind::GIT_WORKTREE))) { return; }
    state.read(cx).dispose();
    crate::prompt::close_guarded(kind::GIT_WORKTREE, window, cx);
}

pub(super) fn manager_title(state: &Entity<WorktreeModal>, title: String) -> AnyElement {
    let state = state.clone();
    div().w_full().flex().items_center().justify_between().gap(px(12.0))
        .child(div().flex_1().truncate().child(title))
        .child(div().id("worktree-owned-close").w(px(20.0)).h(px(20.0)).flex_none()
            .flex().items_center().justify_center().cursor_pointer().child("x")
            .on_click(move |_: &ClickEvent, window, cx| {
                close_manager(&state, window, cx);
            }))
        .into_any_element()
}

fn form_error(state: &Entity<WorktreeModal>, error: String, cx: &mut App) {
    state.update(cx, |s, cx| { s.create_error = Some(error); cx.notify(); });
}

#[derive(Clone, PartialEq, Eq)]
struct Draft {
    mode: Mode,
    branch: String,
    base: String,
    path: String,
    selected: Vec<String>,
    add_as_project: bool,
}

impl Draft {
    fn capture(state: &WorktreeModal, cx: &App) -> Self {
        Self {
            mode: state.mode,
            branch: match state.mode { Mode::Existing => state.sel_branch.clone(), Mode::New => state.new_branch.read(cx).value().to_string() },
            base: state.base_branch.clone(),
            path: state.wt_path.read(cx).value().to_string(),
            selected: state.selected_keys.clone(),
            add_as_project: state.add_as_project,
        }
    }

    fn may_register_completion(&self, current: &Self) -> bool {
        self.add_as_project && self == current
    }
}

#[derive(Clone, PartialEq, Eq)]
struct RegistrationAlias {
    project_id: String,
    config: serde_json::Value,
    source: Option<execution_host::ExecutionSourceSignature>,
}

#[derive(Clone, PartialEq, Eq)]
struct RegistrationTarget {
    location: ProjectLocationKey,
    aliases: Vec<RegistrationAlias>,
}

impl RegistrationTarget {
    fn capture(store: &AppStore, backend: &ExecutionBackend, path: &str) -> Result<Self, String> {
        let (location, _) = project_location(backend, path)?;
        let ids = store.project_ids_for_location(&location);
        // Keep configuration order: central registration activates its first
        // matching alias. A later alias must not become this click's target.
        let aliases = store.projects().iter().filter(|project| ids.contains(&project.id))
            .map(|project| Ok(RegistrationAlias {
                project_id: project.id.clone(),
                config: serde_json::to_value(project).map_err(|_| STALE.to_string())?,
                source: store.project_execution_snapshot(&project.id).ok().map(|source| source.source_signature()),
            })).collect::<Result<_, String>>()?;
        Ok(Self { location, aliases })
    }

    fn is_current(&self, store: &AppStore, backend: &ExecutionBackend, path: &str, busy: Option<GitWritePhase>) -> bool {
        Self::capture(store, backend, path).is_ok_and(|current| self.may_register(&current, busy))
    }

    fn may_register(&self, current: &Self, busy: Option<GitWritePhase>) -> bool {
        busy.is_none() && self == current
    }
}

fn selected_host_path(backend: &ExecutionBackend, selected: &str) -> Result<String, String> {
    match backend {
        ExecutionBackend::Wsl { distro } => {
            let path = mt_core::parse_wsl_unc(&selected.replace('/', "\\")).ok_or("Select a folder in the captured WSL distribution")?;
            if !path.distro.eq_ignore_ascii_case(distro) { return Err("Selected folder belongs to another WSL distribution".into()); }
            execution_host::normalize_absolute_posix_path(&path.unix_path)
        }
        ExecutionBackend::Ssh { .. } => execution_host::normalize_absolute_posix_path(selected),
        ExecutionBackend::Local => {
            if mt_core::parse_wsl_unc(&selected.replace('/', "\\")).is_some() { return Err("Selected folder belongs to WSL, not the captured local host".into()); }
            Ok(selected.to_string())
        }
    }
}

pub(super) fn browse_destination(state: &Entity<WorktreeModal>, window: &mut Window, cx: &mut App) {
    let Some((request, generation, draft, snapshot)) = state.update(cx, |s, cx| {
        if !s.idle(cx) { return None; }
        let request = s.bump_picker()?;
        Some((request, s.load_generation, Draft::capture(s, cx), s.snapshot.clone()))
    }) else { return; };
    let initial = if draft.path.is_empty() { snapshot.canonical_path.clone() }
        else if draft.selected.len() == 1 { host_parent(&snapshot.backend, &draft.path).to_string() }
        else { draft.path.clone() };
    let initial = match &snapshot.backend {
        ExecutionBackend::Wsl { .. } => match project_location(&snapshot.backend, &initial) {
            Ok((_, path)) => path,
            Err(error) => { form_error(state, error, cx); return; }
        },
        _ => initial,
    };
    let (host, epoch) = match &snapshot.backend {
        ExecutionBackend::Ssh { connection, connection_fingerprint, connection_epoch } => (
            ProjectHostSelection::Ssh { connection: connection.clone(), connection_fingerprint: *connection_fingerprint }, *connection_epoch,
        ),
        ExecutionBackend::Local | ExecutionBackend::Wsl { .. } => (ProjectHostSelection::Local, None),
    };
    let guard_state = state.clone();
    let guard_draft = draft.clone();
    let selected_state = state.clone();
    crate::remote_directory_picker::open(
        DirectoryPickerOptions { host, initial_path: initial, canonical_home: None, expected_connection_epoch: epoch },
        move |cx| {
            let s = guard_state.read(cx);
            s.idle(cx) && s.picker_request == request && s.load_generation == generation && Draft::capture(s, cx) == guard_draft
        },
        move |path, window, cx| {
            let s = selected_state.read(cx);
            if !s.idle(cx) || s.picker_request != request || s.load_generation != generation || Draft::capture(s, cx) != draft { return; }
            let path = match selected_host_path(&snapshot.backend, &path) {
                Ok(path) => path,
                Err(error) => { form_error(&selected_state, error, cx); return; }
            };
            selected_state.update(cx, |s, cx| {
                s.path_edited = true;
                s.wt_path.update(cx, |input, cx| input.set_value(path, window, cx));
                cx.notify();
            });
        },
        |_, _| {}, window, cx,
    );
}

fn normal_completion(state: GitWriteState, error: bool, reconciliation_error: bool, retained: bool) -> bool {
    state == GitWriteState::Completed && !error && !reconciliation_error && !retained
}

fn verified_postcondition(outcome: &GitWriteOutcome) -> Option<&GitPostcondition> {
    normal_completion(outcome.state, outcome.error.is_some(), outcome.reconciliation_error.is_some(), outcome.lease_retained)
        .then(|| outcome.reconciliation.as_ref().map(|result| &result.postcondition)).flatten()
}

fn outcome_current(state: &WorktreeModal, outcome: &GitWriteOutcome, expected: &GitRepository, operation_id: u64, cx: &App) -> bool {
    state.repositories.values().any(|current| {
        current.authority() == expected.authority()
            && current.backend().snapshot().source_signature() == expected.backend().snapshot().source_signature()
            && host_ui::repository_current(current, &state.store, cx)
            && outcome.is_current(expected, operation_id)
    })
}

fn register(
    state: &Entity<WorktreeModal>, repository: &GitRepository, target: &RegistrationTarget,
    path: &str, name: Option<&str>, cx: &mut App,
) -> Result<crate::store::ProjectRegistrationOutcome, String> {
    let s = state.read(cx);
    if !s.is_live(cx) || !host_ui::repository_current(repository, &s.store, cx)
        || !target.is_current(s.store.read(cx), &repository.backend().snapshot().backend, path, repository.busy().map(|busy| busy.phase))
    { return Err(STALE.into()); }
    let snapshot = repository.backend().snapshot();
    let (location, canonical) = project_location(&snapshot.backend, path)?;
    let root_id = snapshot.root_project_id.clone();
    let store = s.store.clone();
    store.update(cx, |store, cx| store.register_or_activate_project_with_placement(
        location, &canonical, name, ProjectPlacement::ChildWorktree { root_project_id: &root_id }, cx,
    ))
}

pub(super) fn open_worktree(
    state: &Entity<WorktreeModal>, group: &RepoGroup, wt: &WorktreeInfo, terminal: bool, window: &mut Window, cx: &mut App,
) {
    let Some(repository) = state.read(cx).read_repository(group, cx).filter(|repository| repository.busy().is_none()) else { return; };
    if !state.read(cx).idle(cx) || !wt.is_valid { return; }
    let target = match RegistrationTarget::capture(state.read(cx).store.read(cx), &repository.backend().snapshot().backend, &wt.path) {
        Ok(target) => target,
        Err(error) => { form_error(state, error, cx); return; }
    };
    let request = match repository.request(GitRead::Worktrees) {
        Ok(request) => request,
        Err(error) => { form_error(state, error.to_string(), cx); return; }
    };
    let request_id = request.id();
    let Some(owner) = state.update(cx, |s, cx| { let id = s.next_write()?; s.creating = true; cx.notify(); Some(id) }) else { return; };
    let (state, wt) = (state.clone(), wt.clone());
    window.spawn(cx, async move |cx| {
        let result = cx.background_executor().spawn(async move { request.execute() }).await;
        let _ = cx.update(|window, cx| {
            if !owned(state.read(cx), owner, cx) || !host_ui::repository_current(&repository, &state.read(cx).store, cx) { return; }
            state.update(cx, |s, cx| { s.creating = false; cx.notify(); });
            let result = result.map_err(|error| error.to_string()).and_then(|result| {
                if !result.is_current(&repository, request_id) { return Err(STALE.into()); }
                let GitReadValue::Worktrees(inventory) = result.value else { return Err(STALE.into()); };
                if inventory.scan.source == mt_project::worktree::WorktreeScanSource::LastKnown
                    || !inventory.entries.iter().any(|current| current.path == wt.path && current.branch == wt.branch && current.is_main == wt.is_main && current.is_valid) {
                    return Err("Worktree changed or is unavailable; refresh its inventory".into());
                }
                register(&state, &repository, &target, &wt.path, None, cx)
            });
            match result {
                Err(error) => form_error(&state, error, cx),
                Ok(registration) => {
                    if terminal {
                        let store = state.read(cx).store.clone();
                        let cwd = match project_location(&repository.backend().snapshot().backend, &wt.path) {
                            Ok((_, path)) => path,
                            Err(error) => { form_error(&state, error, cx); return; }
                        };
                        let opened = store.update(cx, |store, cx| {
                            if store.worktree_id_for_project(&registration.project_id) != Some(&registration.worktree_id) { return false; }
                            let pane = store.new_terminal_with_cwd(&registration.project_id, None, None, Some(cwd), window, cx);
                            if let Some(pane) = pane.as_ref() {
                                store.rename_pane(&registration.project_id, pane, wt.branch.as_deref().unwrap_or(&wt.name), cx);
                            }
                            pane.is_some()
                        });
                        if !opened { form_error(&state, "Terminal opening was not completed; the verified project was retained.".into(), cx); return; }
                    }
                    close_manager(&state, window, cx);
                    if terminal { crate::workbench_area::activate_terminal_page(window, cx); }
                }
            }
        });
    }).detach();
}

fn create_plans(state: &WorktreeModal, groups: &[RepoGroup], draft: &Draft, cx: &App) -> Result<Vec<(GitRepository, GitWrite, Option<RegistrationTarget>)>, String> {
    if draft.path.is_empty() || draft.branch.trim().is_empty() { return Err("Choose a branch and worktree destination".into()); }
    let selected = groups.iter().filter(|group| draft.selected.contains(&group.key)).collect::<Vec<_>>();
    if selected.is_empty() { return Err("Select a repository".into()); }
    let mut plans = Vec::new();
    let mut targets = std::collections::HashSet::new();
    for group in &selected {
        let repository = state.repository(group, cx).ok_or(STALE)?;
        if repository.busy().is_some() { return Err("A conflicting Git operation is running".into()); }
        let branches = state.branches_by_repo.get(&group.key).ok_or("Branches are unavailable")?;
        let branch = match draft.mode {
            Mode::New => GitRef::local(draft.branch.trim()),
            Mode::Existing => {
                let branch = branches.iter().find(|branch| !branch.is_remote && branch.name == draft.branch).ok_or("Selected branch changed")?;
                GitRef::from_branch(branch)
            }
        }.map_err(|error| error.to_string())?;
        let base = if draft.mode == Mode::New && !draft.base.is_empty() {
            let mut matches = branches.iter().filter(|branch| branch.name == draft.base);
            let base = matches.next().ok_or("Selected base branch changed")?;
            if matches.next().is_some() { return Err("Base branch name is ambiguous".into()); }
            Some(ObjectId::parse(&base.commit_hash).map_err(|error| error.to_string())?)
        } else { None };
        let target = if selected.len() == 1 { draft.path.clone() } else {
            host_join(&state.snapshot.backend, &draft.path, &format!("{}-{}", group.name, sanitize_branch_for_dir(&draft.branch)))
        };
        if !targets.insert(host_path_key(&state.snapshot.backend, &target)) { return Err("Several repositories resolve to the same destination".into()); }
        let registration = draft.add_as_project.then(|| RegistrationTarget::capture(state.store.read(cx), &state.snapshot.backend, &target)).transpose()?;
        plans.push((repository, GitWrite::WorktreeAdd { target, branch, create_branch: draft.mode == Mode::New, base }, registration));
    }
    Ok(plans)
}

pub(super) fn create(state: &Entity<WorktreeModal>, groups: &[RepoGroup], window: &mut Window, cx: &mut App) {
    if !state.read(cx).idle(cx) { return; }
    let draft = Draft::capture(state.read(cx), cx);
    let plans = match create_plans(state.read(cx), groups, &draft, cx) {
        Ok(plans) => plans,
        Err(error) => { form_error(state, error, cx); return; }
    };
    let Some(owner) = state.update(cx, |s, cx| {
        let owner = s.next_write()?;
        s.creating = true;
        s.create_error = None;
        s.create_results.clear();
        cx.notify(); Some(owner)
    }) else { return; };
    let state = state.clone();
    window.spawn(cx, async move |cx| {
        let mut results = Vec::new();
        for (repository, operation, registration) in plans {
            if cx.update(|_, cx| owned(state.read(cx), owner, cx) && host_ui::repository_current(&repository, &state.read(cx).store, cx)).unwrap_or(false) {
                let result = cx.background_executor().spawn(async move {
                    let prepared = repository.prepare_write(operation).map_err(|error| error.to_string())?;
                    Ok::<_, String>((prepared.repository().clone(), prepared.id(), prepared.execute(), registration))
                }).await;
                results.push(result);
            } else { break; }
        }
        let _ = cx.update(|window, cx| {
            if !owned(state.read(cx), owner, cx) { return; }
            state.update(cx, |s, cx| { s.creating = false; cx.notify(); });
            let may_register = draft.may_register_completion(&Draft::capture(state.read(cx), cx));
            let mut failures = Vec::new();
            let mut first_registration = None;
            for result in results {
                let (repository, operation_id, outcome, registration) = match result {
                    Ok(result) => result,
                    Err(error) => { failures.push(("Worktree".into(), error)); continue; }
                };
                let GitWrite::WorktreeAdd { target, branch, .. } = outcome.operation() else { continue; };
                let source_current = outcome_current(state.read(cx), &outcome, &repository, operation_id, cx);
                let verified = source_current && matches!(verified_postcondition(&outcome),
                    Some(GitPostcondition::WorktreeCreated { authority, branch: created, .. })
                        if authority.worktree_root == *target && authority.common_dir == outcome.repository().authority().common_dir && created == branch);
                if !verified {
                    failures.push((target.clone(), host_ui::outcome_error(&outcome).unwrap_or_else(|| "Worktree creation was not verified; no project was registered.".into())));
                    continue;
                }
                if let Some(registration) = registration {
                    if !may_register {
                        failures.push((target.clone(), "Worktree created. Project registration was skipped because the draft changed.".into()));
                        continue;
                    }
                    match register(&state, outcome.repository(), &registration, target, None, cx) {
                        Ok(registration) => { first_registration.get_or_insert(registration); }
                        Err(error) => failures.push((target.clone(), error)),
                    }
                }
            }
            let on_changed = state.read(cx).on_changed.clone();
            on_changed(cx);
            if failures.is_empty() && let Some(registration) = first_registration {
                let store = state.read(cx).store.clone();
                store.update(cx, |store, cx| {
                    if store.worktree_id_for_project(&registration.project_id) == Some(&registration.worktree_id) {
                        store.set_active_project(&registration.project_id, cx);
                    }
                });
                close_manager(&state, window, cx);
                return;
            }
            state.update(cx, |s, cx| {
                if failures.is_empty() && Draft::capture(s, cx) == draft {
                    s.new_branch.update(cx, |input, cx| input.set_value("", window, cx));
                    s.sel_branch.clear();
                    s.path_edited = false;
                }
                s.create_results = failures;
                cx.notify();
            });
            load(&state, cx);
        });
    }).detach();
}

struct RemoveForm {
    lifetime: GitLifetime,
    request: u64,
    force: bool,
    preparing: bool,
    removing: bool,
    error: Option<String>,
    prepared: Option<PreparedGitWrite>,
    guard: Option<GitWorktreeRemovalGuard>,
}

impl Drop for RemoveForm {
    fn drop(&mut self) { self.lifetime.invalidate(); }
}

impl Render for RemoveForm {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement { div() }
}

fn removal_live(state: &WorktreeModal, form: &RemoveForm, owner: u64, cx: &App) -> bool {
    owned(state, owner, cx) && form.lifetime.is_valid()
}

fn prepare_remove(
    state: &Entity<WorktreeModal>, form: &Entity<RemoveForm>, repository: &GitRepository,
    path: &str, owner: u64, cx: &mut App,
) {
    if !removal_live(state.read(cx), form.read(cx), owner, cx) || form.read(cx).removing { return; }
    let guard = project_location(&state.read(cx).snapshot.backend, path)
        .and_then(|(location, _)| state.read(cx).store.read(cx).prepare_git_worktree_removal(&location, cx));
    let Some((request, force, lifetime)) = form.update(cx, |form, cx| {
        let Some(request) = form.request.checked_add(1) else { form.lifetime.invalidate(); return None; };
        form.request = request;
        form.prepared = None;
        form.guard = None;
        match guard {
            Ok(guard) => { form.guard = Some(guard); form.error = None; form.preparing = true; }
            Err(error) => { form.error = Some(error); form.preparing = false; cx.notify(); return None; }
        }
        cx.notify(); Some((request, form.force, form.lifetime.clone()))
    }) else { return; };
    let (state, form, expected, path) = (state.clone(), form.clone(), repository.clone(), path.to_string());
    cx.spawn(async move |cx| {
        let result = cx.background_executor().spawn(async move {
            // A separate backend lifetime makes closing this confirmation
            // invalidate a queued preflight/write without closing the manager.
            let repository = host_ui::resolve_repository(expected.backend().snapshot().clone(), &expected.authority().worktree_root, lifetime)?;
            if repository.authority() != expected.authority()
                || repository.backend().snapshot().source_signature() != expected.backend().snapshot().source_signature() {
                return Err(STALE.into());
            }
            repository.prepare_write(GitWrite::WorktreeRemove { target: path, force }).map_err(|error| error.to_string())
        }).await;
        let _ = cx.update(|cx| {
            if !removal_live(state.read(cx), form.read(cx), owner, cx) || form.read(cx).request != request { return; }
            form.update(cx, |f, cx| {
                f.preparing = false;
                match result {
                    Ok(prepared) => {
                        let valid = host_ui::repository_current(prepared.repository(), &state.read(cx).store, cx)
                            && f.guard.as_ref().is_some_and(|guard| state.read(cx).store.read(cx).git_worktree_removal_is_current(guard, cx));
                        if valid { f.prepared = Some(prepared); } else { f.error = Some(STALE.into()); }
                    }
                    Err(error) => f.error = Some(error),
                }
                cx.notify();
            });
        });
    }).detach();
}

pub(super) fn open_remove_confirm(
    state: &Entity<WorktreeModal>, group: &RepoGroup, wt: &WorktreeInfo, window: &mut Window, cx: &mut App,
) {
    if wt.is_main || !wt.is_valid || !state.read(cx).idle(cx) || crate::prompt::is_open(kind::GIT_WORKTREE_REMOVE) { return; }
    let Some(repository) = state.read(cx).repository(group, cx).filter(|repository| repository.busy().is_none()) else { return; };
    let lifetime = GitLifetime::new();
    let Some(owner) = state.update(cx, |s, cx| {
        let owner = s.next_write()?;
        s.removing = true;
        s.child_lifetimes.retain(GitLifetime::is_valid);
        s.child_lifetimes.push(lifetime.clone());
        cx.notify(); Some(owner)
    }) else { return; };
    let form = cx.new(|_| RemoveForm { lifetime, request: 0, force: false, preparing: false, removing: false, error: None, prepared: None, guard: None });
    prepare_remove(state, &form, &repository, &wt.path, owner, cx);
    let (close_state, close_form) = (state.clone(), form.clone());
    let (state, wt) = (state.clone(), wt.clone());
    open_guarded_with_close(kind::GIT_WORKTREE_REMOVE, window, cx, move |dialog, _, cx| {
        let f = form.read(cx);
        let (force, preparing, removing, error) = (f.force, f.preparing, f.removing, f.error.clone());
        let path = f.prepared.as_ref().and_then(|prepared| match prepared.operation() {
            GitWrite::WorktreeRemove { target, .. } => Some(target.clone()), _ => None,
        }).unwrap_or_else(|| wt.path.clone());
        let (toggle_state, toggle_form, toggle_repository, toggle_path) = (state.clone(), form.clone(), repository.clone(), wt.path.clone());
        let (ok_state, ok_form) = (state.clone(), form.clone());
        let mut body = div().px(px(16.0)).flex().flex_col().gap(px(6.0))
            .child(div().text_size(ui::font_px(13.0)).child(tr!("worktree", "removeConfirmMessage", name = wt.name.clone())))
            .child(div().text_size(ui::font_px(11.0)).text_color(ui::text_muted()).child(path))
            .child(div().id("worktree-force").flex().items_center().gap(px(6.0)).cursor_pointer()
                .text_size(ui::font_px(12.0)).child(if force { "[x]" } else { "[ ]" }).child(t("worktree", "forceRemove"))
                .on_click(move |_: &ClickEvent, _, cx| {
                    if toggle_form.read(cx).removing { return; }
                    toggle_form.update(cx, |f, cx| { f.force = !f.force; cx.notify(); });
                    prepare_remove(&toggle_state, &toggle_form, &toggle_repository, &toggle_path, owner, cx);
                }));
        if let Ok((location, _)) = project_location(&state.read(cx).snapshot.backend, &wt.path) {
            let store = state.read(cx).store.read(cx);
            for id in store.project_ids_for_location(&location) {
                if let Some(project) = store.project(&id) {
                    body = body.child(hint_line(tr!("worktree", "removeAlsoProject", name = project.name.clone()), ui::color_warning()));
                }
            }
        }
        if let Some(error) = error {
            body = body.child(hint_line(error, ui::color_error()));
            let (state, form, repository, path) = (state.clone(), form.clone(), repository.clone(), wt.path.clone());
            body = body.child(ui::ghost_button("worktree-revalidate-remove", "Review target")
                .on_click(move |_: &ClickEvent, _, cx| prepare_remove(&state, &form, &repository, &path, owner, cx)));
        }
        dialog.title(t("worktree", "removeConfirmTitle")).w(px(440.0)).confirm()
            .button_props(gpui_component::dialog::DialogButtonProps::default()
                .ok_text(if preparing { "Validating..." } else if removing { t("worktree", "removing") } else { t("worktree", "removeConfirm") })
                .cancel_text(t("worktree", "cancel")))
            .child(body)
            .on_ok(move |_: &ClickEvent, window, cx| {
                remove_worktree(&ok_state, &ok_form, owner, window, cx);
                false
            })
    }, move |_, cx| {
        close_form.read(cx).lifetime.invalidate();
        close_state.update(cx, |s, cx| { if s.write_request == owner { s.removing = false; cx.notify(); } });
        load(&close_state, cx);
    });
}

fn remove_worktree(state: &Entity<WorktreeModal>, form: &Entity<RemoveForm>, owner: u64, window: &mut Window, cx: &mut App) {
    if !removal_live(state.read(cx), form.read(cx), owner, cx) || form.read(cx).preparing || form.read(cx).removing { return; }
    let store = state.read(cx).store.clone();
    let valid = form.read(cx).guard.as_ref().is_some_and(|guard| store.read(cx).git_worktree_removal_is_current(guard, cx))
        && form.read(cx).prepared.as_ref().is_some_and(|prepared| host_ui::repository_current(prepared.repository(), &store, cx) && prepared.repository().busy().is_none());
    if !valid { form.update(cx, |f, cx| { f.error = Some(STALE.into()); cx.notify(); }); return; }
    let Some((prepared, guard)) = form.update(cx, |f, cx| {
        let prepared = f.prepared.take()?;
        let guard = f.guard.take()?;
        f.removing = true; f.error = None; cx.notify(); Some((prepared, guard))
    }) else { return; };
    let target = match prepared.operation() {
        GitWrite::WorktreeRemove { target, .. } => target.clone(),
        _ => return,
    };
    // Native preparation may canonicalize the target. Do not carry a guard
    // across a different location merely because its display path was similar.
    let location_matches = project_location(&state.read(cx).snapshot.backend, &target)
        .is_ok_and(|(location, _)| guard.matches_location(&location));
    if !location_matches { form.update(cx, |f, cx| { f.removing = false; f.error = Some(STALE.into()); cx.notify(); }); return; }
    let source = prepared.repository().backend().snapshot().clone();
    let lifetime = form.read(cx).lifetime.clone();
    let close = store.update(cx, |store, cx| store.close_git_worktree_terminals(guard, source, lifetime, cx));
    let (state, form) = (state.clone(), form.clone());
    window.spawn(cx, async move |cx| {
        let guard = match close.await {
            Ok(guard) => guard,
            Err(error) => {
                let _ = cx.update(|_, cx| {
                    if removal_live(state.read(cx), form.read(cx), owner, cx) {
                        form.update(cx, |f, cx| { f.removing = false; f.error = Some(error); cx.notify(); });
                    }
                });
                return;
            }
        };
        let ready = cx.update(|_, cx| {
            removal_live(state.read(cx), form.read(cx), owner, cx)
                && host_ui::repository_current(prepared.repository(), &store, cx)
                && store.read(cx).git_worktree_removal_is_current(&guard, cx)
        }).unwrap_or(false);
        if !ready {
            let _ = cx.update(|_, cx| {
                if form.read(cx).lifetime.is_valid() {
                    form.update(cx, |f, cx| { f.removing = false; f.error = Some(STALE.into()); cx.notify(); });
                }
            });
            return;
        }
        let operation_id = prepared.id();
        let repository = prepared.repository().clone();
        let outcome = cx.background_executor().spawn(async move { prepared.execute() }).await;
        let _ = cx.update(|window, cx| {
            if !removal_live(state.read(cx), form.read(cx), owner, cx) {
                // A cancelled confirmation cannot finalize anything. Its late
                // effects may still refresh the original, still-live manager.
                if state.read(cx).is_live(cx) {
                    let on_changed = state.read(cx).on_changed.clone();
                    on_changed(cx);
                    load(&state, cx);
                }
                return;
            }
            let current = outcome_current(state.read(cx), &outcome, &repository, operation_id, cx);
            let verified = current && matches!(verified_postcondition(&outcome), Some(GitPostcondition::WorktreeRemoved { path }) if path == &target);
            let error = if verified {
                store.update(cx, |store, cx| store.finish_git_worktree_removal(guard, cx)).err()
            } else {
                Some(host_ui::outcome_error(&outcome).unwrap_or_else(|| "Worktree removal was not verified. Project configuration was kept.".into()))
            };
            if let Some(error) = error {
                form.update(cx, |f, cx| { f.removing = false; f.error = Some(error); cx.notify(); });
                state.update(cx, |s, cx| {
                    if let Some(groups) = &mut s.groups {
                        for group in groups { group.readable = false; group.authoritative = false; }
                    }
                    s.branches_by_repo.clear();
                    cx.notify();
                });
                let on_changed = state.read(cx).on_changed.clone();
                on_changed(cx);
            } else {
                // The exact live form was checked before synchronous store
                // finalization, which can itself remove this dialog's source.
                if crate::prompt::close_guarded(kind::GIT_WORKTREE_REMOVE, window, cx) {
                    form.read(cx).lifetime.invalidate();
                }
                state.update(cx, |s, cx| { if s.write_request == owner { s.removing = false; cx.notify(); } });
                let on_changed = state.read(cx).on_changed.clone();
                on_changed(cx);
                load(&state, cx);
            }
        });
    }).detach();
}

pub(super) fn prune(state: &Entity<WorktreeModal>, group: &RepoGroup, cx: &mut App) {
    if !state.read(cx).idle(cx) { return; }
    let Some(repository) = state.read(cx).repository(group, cx).filter(|repo| repo.busy().is_none()) else { return; };
    let store = state.read(cx).store.clone();
    let guards = group.worktrees.iter().filter(|wt| !wt.is_main).map(|wt| {
        let guard = project_location(&state.read(cx).snapshot.backend, &wt.path).and_then(|(location, _)| {
            let guard = store.read(cx).prepare_git_worktree_removal(&location, cx)?;
            if !guard.terminals_empty() { return Err("Terminal records remain; project configuration was kept.".into()); }
            Ok(guard)
        });
        (wt.path.clone(), guard)
    }).collect::<HashMap<_, _>>();
    let Some(owner) = state.update(cx, |s, cx| {
        let owner = s.next_write()?;
        s.pruning_key = Some(group.key.clone()); s.create_error = None; cx.notify(); Some(owner)
    }) else { return; };
    let state = state.clone();
    cx.spawn(async move |cx| {
        let result = cx.background_executor().spawn(async move {
            repository.prepare_write(GitWrite::WorktreePrune).map(|prepared| (prepared.repository().clone(), prepared.id(), prepared.execute())).map_err(|error| error.to_string())
        }).await;
        let _ = cx.update(|cx| {
            if !owned(state.read(cx), owner, cx) { return; }
            state.update(cx, |s, cx| { s.pruning_key = None; cx.notify(); });
            let mut errors = Vec::new();
            match result {
                Err(error) => errors.push(error),
                Ok((repository, operation_id, outcome)) => {
                    let current = outcome_current(state.read(cx), &outcome, &repository, operation_id, cx);
                    if let Some(GitPostcondition::WorktreesPruned { paths }) = verified_postcondition(&outcome).filter(|_| current) {
                        for path in paths {
                            match guards.get(path) {
                                Some(Ok(guard)) => {
                                    if let Err(error) = store.update(cx, |store, cx| store.finish_git_worktree_removal(guard.clone(), cx)) { errors.push(format!("{path}: {error}")); }
                                }
                                Some(Err(error)) => errors.push(format!("{path}: {error}")),
                                None => errors.push(format!("{path}: No captured cleanup authority; configuration was kept.")),
                            }
                        }
                    } else { errors.push(host_ui::outcome_error(&outcome).unwrap_or_else(|| "Prune was not verified; project configuration was kept.".into())); }
                }
            }
            if !errors.is_empty() { form_error(&state, errors.join("\n"), cx); }
            let on_changed = state.read(cx).on_changed.clone();
            on_changed(cx);
            load(&state, cx);
        });
    }).detach();
}

pub(super) fn review_uncertain(state: &Entity<WorktreeModal>, group: &RepoGroup, operation_id: u64, window: &mut Window, cx: &mut App) {
    let Some(repository) = state.read(cx).repositories.get(&group.key).cloned() else { return; };
    if !state.read(cx).idle(cx) || !host_ui::repository_current(&repository, &state.read(cx).store, cx) { return; }
    let state = state.clone();
    crate::prompt::Confirm::new("Review uncertain Git operation", "Confirm that the original Git process has stopped on its execution host. This review will not replay the operation or remove project configuration.")
        .ok_text("Process stopped; review")
        .open(move |_, cx| {
            if !state.read(cx).idle(cx) || !host_ui::repository_current(&repository, &state.read(cx).store, cx)
                || !repository.busy().is_some_and(|busy| busy.operation_id == operation_id && busy.phase == GitWritePhase::Uncertain) { return; }
            let Some(owner) = state.update(cx, |s, cx| { let id = s.next_write()?; s.creating = true; cx.notify(); Some(id) }) else { return; };
            let (state, repository) = (state.clone(), repository.clone());
            cx.spawn(async move |cx| {
                let result = cx.background_executor().spawn(async move {
                    repository.review_uncertain(operation_id, UncertainReview::UserConfirmedOriginalOperationStopped)
                }).await;
                let _ = cx.update(|cx| {
                    if !owned(state.read(cx), owner, cx) { return; }
                    state.update(cx, |s, cx| { s.creating = false; cx.notify(); });
                    if let Err(error) = result { form_error(&state, error.to_string(), cx); }
                    load(&state, cx);
                });
            }).detach();
        }, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> Draft {
        Draft { mode: Mode::New, branch: "feature".into(), base: "main".into(), path: "/repo/feature".into(), selected: vec!["/repo".into()], add_as_project: true }
    }

    #[test]
    fn creation_completion_cannot_register_or_close_over_a_newer_draft() {
        let original = draft();
        assert!(original.may_register_completion(&original.clone()));
        let mut changed = vec![original.clone(); 6];
        changed[0].branch = "newer-feature".into();
        changed[1].path = "/repo/another".into();
        changed[2].base = "origin/main".into();
        changed[3].mode = Mode::Existing;
        changed[4].selected.push("/another-repo".into());
        changed[5].add_as_project = false;
        for current in changed { assert!(!original.may_register_completion(&current)); }
        let mut no_registration = original;
        no_registration.add_as_project = false;
        assert!(!no_registration.may_register_completion(&no_registration));
    }

    #[test]
    fn registration_keeps_captured_alias_order_source_and_uncertain_exclusion() {
        use execution_host::{ExecutionBackendSignature, ExecutionSourceSignature};
        use mt_identity::{ExecutionHostId, HostInstallId, RepoId, WorktreeId};

        let host = ExecutionHostId::derive("registration-test", &HostInstallId::new());
        let repo = RepoId::derive(&host, "/repo/.git");
        let source = ExecutionSourceSignature {
            execution_host_id: host, root_project_id: "root".into(), root_source_path: "/repo".into(),
            worktree_id: WorktreeId::derive(&repo, "/repo/feature", None), canonical_path: "/repo/feature".into(),
            backend: ExecutionBackendSignature::Ssh { connection_id: "ssh-a".into(), connection_fingerprint: 1, connection_epoch: Some(7) },
        };
        let alias = |id: &str| RegistrationAlias {
            project_id: id.into(), config: serde_json::json!({"id": id, "path": "/configured/alias"}), source: Some(source.clone()),
        };
        let original = RegistrationTarget {
            location: ProjectLocationKey::Ssh { connection_id: "ssh-a".into(), normalized_posix_path: "/repo/feature".into() },
            aliases: vec![alias("first"), alias("second")],
        };
        assert!(original.may_register(&original.clone(), None));
        for phase in [GitWritePhase::Validating, GitWritePhase::Running, GitWritePhase::Reconciling, GitWritePhase::Uncertain] {
            assert!(!original.may_register(&original, Some(phase)));
        }
        let mut changed = vec![original.clone(); 7];
        changed[0].aliases.reverse();
        changed[1].aliases.push(alias("new"));
        changed[2].aliases.remove(0);
        changed[3].aliases[0].config = serde_json::json!({"id": "first", "path": "/repointed"});
        changed[4].aliases[0].source = Some(source.with_connection_epoch(Some(8)));
        changed[5].aliases[0].source = None;
        changed[6].location = ProjectLocationKey::Ssh { connection_id: "ssh-b".into(), normalized_posix_path: "/repo/feature".into() };
        for current in changed { assert!(!original.may_register(&current, None)); }
        let mut unregistered = original.clone();
        unregistered.aliases.clear();
        assert!(unregistered.may_register(&unregistered, None));
        assert!(!unregistered.may_register(&original, None), "a later alias cannot replace the original no-target decision");
        let mut canonicalized = unregistered.clone();
        canonicalized.location = ProjectLocationKey::Ssh { connection_id: "ssh-a".into(), normalized_posix_path: "/elsewhere/feature".into() };
        assert!(!unregistered.may_register(&canonicalized, None));
    }

    #[test]
    fn source_invalid_manager_can_close_but_disposed_or_covered_instance_cannot() {
        let source = GitLifetime::new();
        let view = GitLifetime::new();
        source.invalidate();
        assert!(!begin_manager_close(&view, false));
        assert!(view.is_valid());
        assert!(begin_manager_close(&view, true));
        let successor = GitLifetime::new();
        assert!(!begin_manager_close(&view, true));
        assert!(successor.is_valid());
        assert!(begin_manager_close(&successor, true));
    }

    #[test]
    fn only_normal_verified_completion_can_authorize_registration_or_cleanup() {
        assert!(normal_completion(GitWriteState::Completed, false, false, false));
        for state in [GitWriteState::NotDispatched, GitWriteState::Completed, GitWriteState::Uncertain] {
            for error in [true, false] {
                for reconciliation_error in [true, false] {
                    for retained in [true, false] {
                        assert_eq!(normal_completion(state, error, reconciliation_error, retained),
                            state == GitWriteState::Completed && !error && !reconciliation_error && !retained);
                    }
                }
            }
        }
    }

    #[test]
    fn directory_selection_keeps_wsl_distribution_and_local_ownership() {
        let wsl = ExecutionBackend::Wsl { distro: "Ubuntu".into() };
        assert_eq!(selected_host_path(&wsl, r"\\wsl.localhost\Ubuntu\srv\worktree").unwrap(), "/srv/worktree");
        assert!(selected_host_path(&wsl, r"\\wsl.localhost\Debian\srv\worktree").is_err());
        assert!(selected_host_path(&wsl, "/srv/worktree").is_err());
        assert!(selected_host_path(&ExecutionBackend::Local, r"\\wsl$\Ubuntu\srv\worktree").is_err());
        assert_eq!(selected_host_path(&ExecutionBackend::Local, "/srv/worktree").unwrap(), "/srv/worktree");
    }
}
