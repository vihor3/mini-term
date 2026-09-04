use std::sync::atomic::{AtomicU64, Ordering};

use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, PathPromptOptions, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use mt_config::SshConnection;
use mt_ui::icons::{FileIcon, Geom, Ink, Shape, VectorIcon};
use mt_ui::tooltip::Tooltip;

use super::{
    CreateMode, GitRelationship, HostPathProbe, HostSignature, HostStatus, LocalProjectOps,
    OnboardingError, OnboardingErrorKind, OnboardingOperationResult, OnboardingPage,
    OnboardingState, OperationOwner, OperationPhase, OperationResultAuthority, ProjectHostOps,
    ProjectHostSelection, VerifiedProjectLocation, add_existing_folder, checked_next,
    clone_from_url, create_new_project, infer_clone_folder_name, initialize_existing_folder,
    validate_portable_basename,
};
use crate::i18n::{t, tr};
use crate::menu::{self, MenuEntry, MenuItem};
use crate::prompt::{close_guarded, kind, open_guarded_with_close};
use crate::ssh_conn::{build_group_buckets, connection_summary};
use crate::store::AppStore;
use crate::ui;

const DIALOG_WIDTH: f32 = 560.0;
const DIALOG_HEIGHT: f32 = 520.0;

static NEXT_FORM_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

const BACK_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.09,
        Geom::Polyline(&[(0.58, 0.20), (0.28, 0.50), (0.58, 0.80)]),
    ),
    Shape::line(
        Ink::Current,
        0.09,
        Geom::Polyline(&[(0.30, 0.50), (0.82, 0.50)]),
    ),
];

const CLOSE_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.09,
        Geom::Polyline(&[(0.24, 0.24), (0.76, 0.76)]),
    ),
    Shape::line(
        Ink::Current,
        0.09,
        Geom::Polyline(&[(0.76, 0.24), (0.24, 0.76)]),
    ),
];

const CHEVRON_DOWN: &[Shape] = &[Shape::line(
    Ink::Current,
    0.10,
    Geom::Polyline(&[(0.22, 0.36), (0.50, 0.64), (0.78, 0.36)]),
)];

const COMPUTER_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.075,
        Geom::Rect {
            x: 0.14,
            y: 0.18,
            w: 0.72,
            h: 0.50,
            round: 0.05,
        },
    ),
    Shape::line(
        Ink::Current,
        0.075,
        Geom::Polyline(&[(0.50, 0.68), (0.50, 0.80)]),
    ),
    Shape::line(
        Ink::Current,
        0.075,
        Geom::Polyline(&[(0.32, 0.82), (0.68, 0.82)]),
    ),
];

const PLUS_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.10,
        Geom::Polyline(&[(0.18, 0.50), (0.82, 0.50)]),
    ),
    Shape::line(
        Ink::Current,
        0.10,
        Geom::Polyline(&[(0.50, 0.18), (0.50, 0.82)]),
    ),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerTarget {
    AddExisting,
    CloneParent,
    CreateParent,
    InitializeExisting,
}

#[derive(Clone, Debug)]
struct FormContextOwner {
    form_instance_id: u64,
    host_generation: u64,
    page: OnboardingPage,
    create_mode: CreateMode,
    host_signature: HostSignature,
}

#[derive(Clone, Debug)]
struct PickerOwner {
    context: FormContextOwner,
    request_id: u64,
    expected_connection_epoch: Option<u64>,
}

fn picker_request_is_current(active: Option<u64>, request_id: u64, busy: bool) -> bool {
    active == Some(request_id) && !busy
}

fn ssh_failure_epoch_is_current(expected: Option<u64>, current: Option<u64>) -> bool {
    expected.is_some() && expected == current
}

fn ssh_operation_authority_is_current(
    expected_fingerprint: u64,
    current_fingerprint: Option<u64>,
    expected_epoch: Option<u64>,
    current_epoch: Option<u64>,
) -> bool {
    current_fingerprint == Some(expected_fingerprint)
        && expected_epoch.is_some()
        && expected_epoch == current_epoch
}

#[derive(Clone, Debug)]
struct HostProbeOwner {
    form_instance_id: u64,
    host_generation: u64,
    host_signature: HostSignature,
}

#[derive(Clone, Debug)]
enum PendingOperation {
    AddExisting {
        path: String,
    },
    Clone {
        url: String,
        parent: String,
        name: String,
    },
    CreateNew {
        parent: String,
        name: String,
    },
    InitializeExisting {
        path: String,
    },
    ClassifyExisting {
        path: String,
    },
}

enum BackgroundCompletion {
    Operation(Result<OnboardingOperationResult, OnboardingError>),
    Classification(Result<HostPathProbe, OnboardingError>),
}

#[derive(Clone, Debug)]
enum ExistingProbeState {
    Unselected,
    Probing,
    Ready(HostPathProbe),
    Error(OnboardingError),
}

struct ProjectOnboardingView {
    store: Entity<AppStore>,
    target_group: Option<String>,
    flow: OnboardingState,
    clone_url: Entity<InputState>,
    clone_parent: Entity<InputState>,
    clone_name: Entity<InputState>,
    create_name: Entity<InputState>,
    create_parent: Entity<InputState>,
    existing_path: Entity<InputState>,
    existing_probe: ExistingProbeState,
    applying_existing_path: bool,
    next_picker_request_id: u64,
    active_picker_request_id: Option<u64>,
    _subscriptions: Vec<Subscription>,
    _host_task: Option<Task<()>>,
}

impl ProjectOnboardingView {
    fn new(
        store: Entity<AppStore>,
        target_group: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let clone_url = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("projectOnboarding", "clone.urlPlaceholder"))
        });
        let clone_parent = cx.new(|cx| InputState::new(window, cx));
        let clone_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("projectOnboarding", "clone.folderNamePlaceholder"))
        });
        let create_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("projectOnboarding", "create.namePlaceholder"))
        });
        let create_parent = cx.new(|cx| InputState::new(window, cx));
        let existing_path = cx.new(|cx| InputState::new(window, cx));

        let mut subscriptions = Vec::new();
        for input in [
            clone_url.clone(),
            clone_parent.clone(),
            clone_name.clone(),
            create_name.clone(),
            create_parent.clone(),
        ] {
            subscriptions.push(cx.subscribe(
                &input,
                |_this: &mut Self, _input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            ));
        }
        subscriptions.push(cx.subscribe(
            &existing_path,
            |this: &mut Self, _input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    if !this.applying_existing_path {
                        this.existing_probe = ExistingProbeState::Unselected;
                    }
                    cx.notify();
                }
            },
        ));

        let form_instance_id = allocate_form_instance_id().unwrap_or(0);
        let flow = OnboardingState::new(form_instance_id, ProjectHostSelection::Local);

        Self {
            store,
            target_group,
            flow,
            clone_url,
            clone_parent,
            clone_name,
            create_name,
            create_parent,
            existing_path,
            existing_probe: ExistingProbeState::Unselected,
            applying_existing_path: false,
            next_picker_request_id: 0,
            active_picker_request_id: None,
            _subscriptions: subscriptions,
            _host_task: None,
        }
    }

    fn context_owner(&self) -> FormContextOwner {
        FormContextOwner {
            form_instance_id: self.flow.form_instance_id,
            host_generation: self.flow.host_generation,
            page: self.flow.page,
            create_mode: self.flow.create_mode,
            host_signature: self.flow.host.signature(),
        }
    }

    fn context_matches(&self, owner: &FormContextOwner) -> bool {
        !self.flow.closed
            && self.flow.form_instance_id == owner.form_instance_id
            && self.flow.host_generation == owner.host_generation
            && self.flow.page == owner.page
            && self.flow.create_mode == owner.create_mode
            && self.flow.host.signature() == owner.host_signature
    }

    fn picker_context_matches(&self, owner: &PickerOwner) -> bool {
        self.context_matches(&owner.context)
            && picker_request_is_current(
                self.active_picker_request_id,
                owner.request_id,
                self.flow.phase.is_busy(),
            )
            && match &owner.context.host_signature {
                HostSignature::Local => owner.expected_connection_epoch.is_none(),
                HostSignature::Ssh { .. } => {
                    owner.expected_connection_epoch.is_some()
                        && self.flow.host_status.observed_epoch() == owner.expected_connection_epoch
                }
            }
    }

    fn operation_context_matches(&self, owner: &OperationOwner) -> bool {
        !self.flow.closed
            && self.flow.active_owner.as_ref() == Some(owner)
            && self.flow.form_instance_id == owner.form_instance_id
            && self.flow.host_generation == owner.host_generation
            && self.flow.page == owner.page
            && owner.create_mode
                == (self.flow.page == OnboardingPage::Create).then_some(self.flow.create_mode)
            && self.flow.host.signature() == owner.host_signature
    }
}

pub fn open(
    store: Entity<AppStore>,
    target_group: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    if crate::prompt::is_open(kind::ADD_PROJECT) {
        return;
    }
    let state = cx.new(|cx| ProjectOnboardingView::new(store, target_group, window, cx));
    let dialog_state = state.clone();
    let close_state = state.clone();
    open_guarded_with_close(
        kind::ADD_PROJECT,
        window,
        cx,
        move |dialog, window, cx| {
            let viewport = window.viewport_size();
            let width = ui::clamp_dialog_width(px(DIALOG_WIDTH), viewport);
            let height = ui::clamp_dialog_body_height(px(DIALOG_HEIGHT), viewport, 0.82, px(0.0));
            dialog
                .p_0()
                .close_button(false)
                .overlay_closable(true)
                .keyboard(true)
                .w(width)
                .child(render_shell(&dialog_state, height, cx))
        },
        move |_window, cx| {
            close_state.update(cx, |view, cx| {
                view.active_picker_request_id = None;
                let _ = view.flow.close();
                cx.notify();
            });
        },
    );
}

fn allocate_form_instance_id() -> Option<u64> {
    NEXT_FORM_INSTANCE_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .ok()
        .filter(|id| *id != 0)
}

fn close_modal(state: &Entity<ProjectOnboardingView>, window: &mut Window, cx: &mut App) {
    state.update(cx, |view, cx| {
        view.active_picker_request_id = None;
        let _ = view.flow.close();
        cx.notify();
    });
    close_guarded(kind::ADD_PROJECT, window, cx);
}

fn navigate_to(
    state: &Entity<ProjectOnboardingView>,
    page: OnboardingPage,
    window: &mut Window,
    cx: &mut App,
) {
    let focus = state.update(cx, |view, cx| {
        view.active_picker_request_id = None;
        if view.flow.navigate(page).is_err() {
            cx.notify();
            return None;
        }
        view.existing_probe = ExistingProbeState::Unselected;
        cx.notify();
        match page {
            OnboardingPage::Home => None,
            OnboardingPage::Clone => Some(view.clone_url.clone()),
            OnboardingPage::Create => Some(match view.flow.create_mode {
                CreateMode::NewFolder => view.create_name.clone(),
                CreateMode::InitializeExisting => view.existing_path.clone(),
            }),
        }
    });
    if let Some(input) = focus {
        crate::prompt::autofocus(&input, window, cx);
    }
}

fn switch_create_mode(
    state: &Entity<ProjectOnboardingView>,
    mode: CreateMode,
    window: &mut Window,
    cx: &mut App,
) {
    let (focus, existing_path) = state.update(cx, |view, cx| {
        view.active_picker_request_id = None;
        if view.flow.switch_create_mode(mode).is_err() {
            cx.notify();
            return (None, None);
        }
        view.existing_probe = ExistingProbeState::Unselected;
        cx.notify();
        match mode {
            CreateMode::NewFolder => (Some(view.create_name.clone()), None),
            CreateMode::InitializeExisting => {
                let path = view.existing_path.read(cx).value().trim().to_string();
                (
                    Some(view.existing_path.clone()),
                    (!path.is_empty()).then_some(path),
                )
            }
        }
    });
    if let Some(input) = focus {
        crate::prompt::autofocus(&input, window, cx);
    }
    if let Some(path) = existing_path {
        start_operation(
            state,
            PendingOperation::ClassifyExisting { path },
            window,
            cx,
        );
    }
}

fn select_host(
    state: &Entity<ProjectOnboardingView>,
    host: ProjectHostSelection,
    window: &mut Window,
    cx: &mut App,
) {
    let signature = host.signature();
    let (changed, clone_parent, create_parent, existing_path) = state.update(cx, |view, cx| {
        let same_ready = view.flow.host.signature() == signature
            && matches!(&view.flow.host_status, HostStatus::Ready { .. });
        if same_ready {
            return (false, None, None, None);
        }
        view.active_picker_request_id = None;
        if view.flow.switch_host(host.clone()).is_err() {
            cx.notify();
            return (false, None, None, None);
        }
        view.existing_probe = ExistingProbeState::Unselected;
        cx.notify();
        (
            true,
            Some(view.clone_parent.clone()),
            Some(view.create_parent.clone()),
            Some(view.existing_path.clone()),
        )
    });
    if !changed {
        return;
    }
    let parent_default = if matches!(&host, ProjectHostSelection::Ssh { .. }) {
        "~"
    } else {
        ""
    };
    if let Some(input) = clone_parent {
        input.update(cx, |input, cx| {
            input.set_value(parent_default.to_string(), window, cx)
        });
    }
    if let Some(input) = create_parent {
        input.update(cx, |input, cx| {
            input.set_value(parent_default.to_string(), window, cx)
        });
    }
    if let Some(input) = existing_path {
        input.update(cx, |input, cx| input.set_value(String::new(), window, cx));
    }
    if matches!(&host, ProjectHostSelection::Ssh { .. }) {
        connect_selected_host(state, window, cx);
    }
}

fn connect_selected_host(state: &Entity<ProjectOnboardingView>, window: &mut Window, cx: &mut App) {
    let Some((owner, connection)) = state.update(cx, |view, cx| {
        let connection = match &view.flow.host {
            ProjectHostSelection::Ssh { connection, .. } => connection.clone(),
            _ => return None,
        };
        view.flow.set_host_status(HostStatus::Connecting);
        let owner = HostProbeOwner {
            form_instance_id: view.flow.form_instance_id,
            host_generation: view.flow.host_generation,
            host_signature: view.flow.host.signature(),
        };
        cx.notify();
        Some((owner, connection))
    }) else {
        return;
    };

    let state_for_task = state.clone();
    let connection_for_probe = connection.clone();
    let task = window.spawn(cx, async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move { crate::remote_ssh::probe_connection(&connection_for_probe) })
            .await;
        let _ = cx.update(|window, cx| {
            complete_host_probe(&state_for_task, &owner, result, window, cx);
        });
    });
    state.update(cx, |view, _cx| view._host_task = Some(task));
}

fn complete_host_probe(
    state: &Entity<ProjectOnboardingView>,
    owner: &HostProbeOwner,
    result: Result<String, String>,
    window: &mut Window,
    cx: &mut App,
) {
    let defaults = state.update(cx, |view, cx| {
        if view.flow.closed
            || view.flow.form_instance_id != owner.form_instance_id
            || view.flow.host_generation != owner.host_generation
            || view.flow.host.signature() != owner.host_signature
        {
            return None;
        }
        let HostSignature::Ssh {
            connection_id,
            connection_fingerprint,
        } = &owner.host_signature
        else {
            return None;
        };
        let current = view
            .store
            .read(cx)
            .ssh_connections()
            .iter()
            .find(|connection| connection.id == *connection_id)
            .cloned();
        let Some(current) = current else {
            view.flow.set_host_status(HostStatus::Error(
                t("projectOnboarding", "error.disconnected").to_string(),
            ));
            cx.notify();
            return None;
        };
        if crate::remote_ssh::connection_fingerprint(&current) != *connection_fingerprint {
            view.flow.set_host_status(HostStatus::Error(
                t("projectOnboarding", "error.disconnected").to_string(),
            ));
            cx.notify();
            return None;
        }
        let defaults = match result {
            Ok(home) => match crate::remote_ssh::current_connection_epoch(connection_id) {
                Some(epoch) => {
                    view.flow.set_host_status(HostStatus::Ready {
                        observed_epoch: Some(epoch),
                    });
                    let clone_parent =
                        matches!(view.clone_parent.read(cx).value().trim(), "" | "~")
                            .then(|| view.clone_parent.clone());
                    let create_parent =
                        matches!(view.create_parent.read(cx).value().trim(), "" | "~")
                            .then(|| view.create_parent.clone());
                    Some((home, clone_parent, create_parent))
                }
                None => {
                    view.flow.set_host_status(HostStatus::Error(
                        t("projectOnboarding", "error.disconnected").to_string(),
                    ));
                    None
                }
            },
            Err(error) => {
                view.flow.set_host_status(HostStatus::Error(error));
                None
            }
        };
        cx.notify();
        defaults
    });
    if let Some((home, clone_parent, create_parent)) = defaults {
        if let Some(input) = clone_parent {
            input.update(cx, |input, cx| input.set_value(home.clone(), window, cx));
        }
        if let Some(input) = create_parent {
            input.update(cx, |input, cx| input.set_value(home, window, cx));
        }
    }
}

fn open_host_menu(
    state: &Entity<ProjectOnboardingView>,
    position: gpui::Point<gpui::Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if state.read(cx).flow.phase.is_busy() {
        return;
    }
    let (connections, groups, selected, selected_status) = {
        let view = state.read(cx);
        let store = view.store.read(cx);
        (
            store.ssh_connections().to_vec(),
            store.ssh_groups().to_vec(),
            view.flow.host.signature(),
            view.flow.host_status.clone(),
        )
    };
    let buckets = build_group_buckets(&connections, &groups);
    let mut entries = Vec::new();

    let local_state = state.clone();
    entries.push(
        MenuItem::new(t("projectOnboarding", "localHost"))
            .shortcut(t("projectOnboarding", "hostStatus.ready"))
            .on_click(move |window, cx| {
                select_host(&local_state, ProjectHostSelection::Local, window, cx);
            })
            .into(),
    );

    for bucket in buckets.display_order() {
        if bucket.items.is_empty() {
            continue;
        }
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::Header(match bucket.group {
            Some(group) => SharedString::from(group),
            None => SharedString::from(t("remoteProject", "ungrouped")),
        }));
        for connection in bucket.items {
            let fingerprint = crate::remote_ssh::connection_fingerprint(&connection);
            let signature = HostSignature::Ssh {
                connection_id: connection.id.clone(),
                connection_fingerprint: fingerprint,
            };
            let status = if selected == signature {
                match &selected_status {
                    HostStatus::Ready { .. } => t("projectOnboarding", "hostStatus.ready"),
                    HostStatus::Connecting => t("projectOnboarding", "hostStatus.connecting"),
                    HostStatus::NotConnected => t("projectOnboarding", "connect"),
                    HostStatus::Error(_) => t("projectOnboarding", "reconnect"),
                }
            } else if crate::remote_ssh::current_connection_epoch(&connection.id).is_some() {
                t("projectOnboarding", "hostStatus.ready")
            } else {
                t("projectOnboarding", "connect")
            };
            let label = if connection.name.trim().is_empty() {
                connection_summary(&connection)
            } else {
                format!("{}  {}", connection.name, connection_summary(&connection))
            };
            let item_state = state.clone();
            entries.push(
                MenuItem::new(label)
                    .shortcut(status)
                    .on_click(move |window, cx| {
                        select_host(
                            &item_state,
                            ProjectHostSelection::Ssh {
                                connection: connection.clone(),
                                connection_fingerprint: fingerprint,
                            },
                            window,
                            cx,
                        );
                    })
                    .into(),
            );
        }
    }

    entries.push(MenuEntry::Separator);
    let add_state = state.clone();
    entries.push(menu::item(
        t("projectOnboarding", "addRemoteHost"),
        move |window, cx| {
            let select_state = add_state.clone();
            crate::ssh_panel::open_add(
                move |connection, window, cx| {
                    let fingerprint = crate::remote_ssh::connection_fingerprint(&connection);
                    select_host(
                        &select_state,
                        ProjectHostSelection::Ssh {
                            connection,
                            connection_fingerprint: fingerprint,
                        },
                        window,
                        cx,
                    );
                },
                window,
                cx,
            );
        },
    ));
    entries.push(menu::item(
        t("projectOnboarding", "manageRemoteHosts"),
        |window, cx| crate::ssh_panel::open(window, cx),
    ));
    menu::show(position, entries, window, cx);
}

fn open_directory_picker(
    state: &Entity<ProjectOnboardingView>,
    target: PickerTarget,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((owner, host, initial_path)) = state.update(cx, |view, cx| {
        if view.flow.phase.is_busy() || view.flow.is_terminally_failed() {
            return None;
        }
        let request_id = match checked_next(view.next_picker_request_id) {
            Ok(request_id) => request_id,
            Err(error) => {
                view.active_picker_request_id = None;
                view.flow.enter_terminal_failure(error);
                cx.notify();
                return None;
            }
        };
        view.next_picker_request_id = request_id;
        view.active_picker_request_id = Some(request_id);
        let initial_path = match target {
            PickerTarget::AddExisting => String::new(),
            PickerTarget::CloneParent => view.clone_parent.read(cx).value().to_string(),
            PickerTarget::CreateParent => view.create_parent.read(cx).value().to_string(),
            PickerTarget::InitializeExisting => view.existing_path.read(cx).value().to_string(),
        };
        Some((
            PickerOwner {
                context: view.context_owner(),
                request_id,
                expected_connection_epoch: view.flow.host_status.observed_epoch(),
            },
            view.flow.host.clone(),
            initial_path,
        ))
    }) else {
        return;
    };
    match host {
        ProjectHostSelection::Local => {
            let prompt = match target {
                PickerTarget::AddExisting | PickerTarget::InitializeExisting => {
                    t("projectOnboarding", "home.addExisting")
                }
                PickerTarget::CloneParent => t("projectOnboarding", "clone.parentLabel"),
                PickerTarget::CreateParent => t("projectOnboarding", "create.parentLabel"),
            };
            let paths = cx.prompt_for_paths(PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some(prompt.into()),
            });
            let picker_state = state.clone();
            window
                .spawn(cx, async move |cx| {
                    let Ok(Ok(Some(paths))) = paths.await else {
                        return;
                    };
                    let Some(path) = paths.into_iter().next() else {
                        return;
                    };
                    let selected = path.to_string_lossy().to_string();
                    let _ = cx.update(|window, cx| {
                        apply_picker_selection(&picker_state, &owner, target, selected, window, cx);
                    });
                })
                .detach();
        }
        ProjectHostSelection::Ssh {
            connection,
            connection_fingerprint,
        } => {
            if ensure_picker_remote_authority(
                state,
                &owner,
                &connection,
                connection_fingerprint,
                cx,
            )
            .is_err()
            {
                return;
            }
            let picker_state = state.clone();
            crate::remote_directory_picker::open(
                connection,
                if initial_path.trim().is_empty() {
                    "~".to_string()
                } else {
                    initial_path
                },
                move |selected, window, cx| {
                    apply_picker_selection(&picker_state, &owner, target, selected, window, cx);
                },
                window,
                cx,
            );
        }
    }
}

fn ensure_picker_remote_authority(
    state: &Entity<ProjectOnboardingView>,
    owner: &PickerOwner,
    connection: &SshConnection,
    connection_fingerprint: u64,
    cx: &mut App,
) -> Result<u64, ()> {
    let current = state
        .read(cx)
        .store
        .read(cx)
        .ssh_connections()
        .iter()
        .find(|candidate| candidate.id == connection.id)
        .cloned();
    let epoch = crate::remote_ssh::current_connection_epoch(&connection.id);
    let valid_context = state.read(cx).picker_context_matches(owner)
        && current.as_ref().is_some_and(|candidate| {
            crate::remote_ssh::connection_fingerprint(candidate) == connection_fingerprint
        });
    if valid_context
        && epoch == owner.expected_connection_epoch
        && let Some(epoch) = epoch
    {
        return Ok(epoch);
    }
    state.update(cx, |view, cx| {
        if view.picker_context_matches(owner) {
            view.active_picker_request_id = None;
            view.flow.set_host_status(HostStatus::Error(
                t("projectOnboarding", "error.disconnected").to_string(),
            ));
            cx.notify();
        }
    });
    Err(())
}

fn apply_picker_selection(
    state: &Entity<ProjectOnboardingView>,
    owner: &PickerOwner,
    target: PickerTarget,
    selected: String,
    window: &mut Window,
    cx: &mut App,
) {
    if !state.read(cx).picker_context_matches(owner) {
        return;
    }
    if let HostSignature::Ssh {
        connection_id,
        connection_fingerprint,
    } = &owner.context.host_signature
    {
        let current = state
            .read(cx)
            .store
            .read(cx)
            .ssh_connections()
            .iter()
            .find(|connection| connection.id == *connection_id)
            .cloned();
        let epoch = match (
            current.as_ref().is_some_and(|connection| {
                crate::remote_ssh::connection_fingerprint(connection) == *connection_fingerprint
            }),
            crate::remote_ssh::current_connection_epoch(connection_id)
                .filter(|epoch| Some(*epoch) == owner.expected_connection_epoch),
        ) {
            (true, Some(epoch)) => epoch,
            _ => {
                state.update(cx, |view, cx| {
                    if view.picker_context_matches(owner) {
                        view.active_picker_request_id = None;
                        view.flow.set_host_status(HostStatus::Error(
                            t("projectOnboarding", "error.disconnected").to_string(),
                        ));
                        cx.notify();
                    }
                });
                return;
            }
        };
        let accepted = state.update(cx, |view, cx| {
            if !view.picker_context_matches(owner) {
                return false;
            }
            view.active_picker_request_id = None;
            view.flow.set_host_status(HostStatus::Ready {
                observed_epoch: Some(epoch),
            });
            cx.notify();
            true
        });
        if !accepted {
            return;
        }
    } else if !state.update(cx, |view, _cx| {
        if !view.picker_context_matches(owner) {
            return false;
        }
        view.active_picker_request_id = None;
        true
    }) {
        return;
    }

    match target {
        PickerTarget::AddExisting => start_operation(
            state,
            PendingOperation::AddExisting { path: selected },
            window,
            cx,
        ),
        PickerTarget::CloneParent => {
            let input = state.read(cx).clone_parent.clone();
            input.update(cx, |input, cx| input.set_value(selected, window, cx));
        }
        PickerTarget::CreateParent => {
            let input = state.read(cx).create_parent.clone();
            input.update(cx, |input, cx| input.set_value(selected, window, cx));
        }
        PickerTarget::InitializeExisting => {
            let input = state.read(cx).existing_path.clone();
            input.update(cx, |input, cx| {
                input.set_value(selected.clone(), window, cx)
            });
            start_operation(
                state,
                PendingOperation::ClassifyExisting { path: selected },
                window,
                cx,
            );
        }
    }
}

fn start_operation(
    state: &Entity<ProjectOnboardingView>,
    operation: PendingOperation,
    window: &mut Window,
    cx: &mut App,
) {
    let Some((owner, host)) = state.update(cx, |view, cx| {
        view.active_picker_request_id = None;
        if view.flow.is_terminally_failed() {
            cx.notify();
            return None;
        }
        if let ProjectHostSelection::Ssh {
            connection,
            connection_fingerprint,
        } = &view.flow.host
        {
            let current_fingerprint = view
                .store
                .read(cx)
                .ssh_connections()
                .iter()
                .find(|candidate| candidate.id == connection.id)
                .map(crate::remote_ssh::connection_fingerprint);
            if !ssh_operation_authority_is_current(
                *connection_fingerprint,
                current_fingerprint,
                view.flow.host_status.observed_epoch(),
                crate::remote_ssh::current_connection_epoch(&connection.id),
            ) {
                let error = OnboardingError::new(
                    OnboardingErrorKind::StaleOperation,
                    t("projectOnboarding", "error.disconnected"),
                );
                view.flow.phase = OperationPhase::Failure(error.clone());
                view.flow.set_host_status(HostStatus::Error(error.message));
                cx.notify();
                return None;
            }
        }
        if matches!(&operation, PendingOperation::ClassifyExisting { .. }) {
            view.existing_probe = ExistingProbeState::Probing;
        }
        let owner = match view.flow.begin_validation() {
            Ok(Some(owner)) => owner,
            Ok(None) | Err(_) => {
                cx.notify();
                return None;
            }
        };
        cx.notify();
        Some((owner, view.flow.host.clone()))
    }) else {
        return;
    };

    let task_state = state.clone();
    window.defer(cx, move |window, cx| {
        let starts = task_state.update(cx, |view, cx| {
            let starts = view.flow.mark_running(&owner);
            if starts {
                cx.notify();
            }
            starts
        });
        if !starts {
            return;
        }
        let expected_connection_epoch = owner.expected_connection_epoch;
        let completion_state = task_state.clone();
        window
            .spawn(cx, async move |cx| {
                let completion =
                    cx.background_executor()
                        .spawn(async move {
                            execute_operation(host, expected_connection_epoch, operation)
                        })
                        .await;
                let _ = cx.update(|window, cx| {
                    complete_operation(&completion_state, &owner, completion, window, cx);
                });
            })
            .detach();
    });
}

fn execute_operation(
    host: ProjectHostSelection,
    expected_connection_epoch: Option<u64>,
    operation: PendingOperation,
) -> BackgroundCompletion {
    match host {
        ProjectHostSelection::Local => execute_with_host(&LocalProjectOps, operation),
        ProjectHostSelection::Ssh {
            connection,
            connection_fingerprint,
        } => {
            let host = crate::remote_ssh::RemoteProjectContext::new(
                connection,
                connection_fingerprint,
                expected_connection_epoch,
            );
            execute_with_host(&host, operation)
        }
    }
}

fn execute_with_host(
    host: &impl ProjectHostOps,
    operation: PendingOperation,
) -> BackgroundCompletion {
    match operation {
        PendingOperation::AddExisting { path } => {
            BackgroundCompletion::Operation(add_existing_folder(host, &path))
        }
        PendingOperation::Clone { url, parent, name } => {
            BackgroundCompletion::Operation(clone_from_url(host, &url, &parent, &name))
        }
        PendingOperation::CreateNew { parent, name } => {
            BackgroundCompletion::Operation(create_new_project(host, &parent, &name))
        }
        PendingOperation::InitializeExisting { path } => {
            BackgroundCompletion::Operation(initialize_existing_folder(host, &path))
        }
        PendingOperation::ClassifyExisting { path } => {
            BackgroundCompletion::Classification(host.probe_existing_directory(&path, false, true))
        }
    }
}

fn complete_operation(
    state: &Entity<ProjectOnboardingView>,
    owner: &OperationOwner,
    completion: BackgroundCompletion,
    window: &mut Window,
    cx: &mut App,
) {
    if !state.read(cx).operation_context_matches(owner) {
        return;
    }

    let failure = match &completion {
        BackgroundCompletion::Operation(Err(error)) => Some((error.clone(), false)),
        BackgroundCompletion::Classification(Err(error)) => Some((error.clone(), true)),
        BackgroundCompletion::Operation(Ok(_)) | BackgroundCompletion::Classification(Ok(_)) => {
            None
        }
    };
    if let Some((error, classification)) = failure {
        let (failure_owner, failure_epoch) = if let Some(authority) = error.authority {
            let Some(reconciled) = reconcile_success_owner(state, owner, authority, cx) else {
                invalidate_changed_remote_authority(state, owner, window, cx);
                return;
            };
            let observed_epoch = authority.observed_connection_epoch;
            if !completion_authority_is_current(state, &reconciled, observed_epoch, cx) {
                invalidate_changed_remote_authority(state, &reconciled, window, cx);
                return;
            }
            (reconciled, observed_epoch)
        } else {
            if !failure_owner_identity_is_current(state, owner, cx) {
                invalidate_changed_remote_authority(state, owner, window, cx);
                return;
            }
            (owner.clone(), owner.expected_connection_epoch)
        };
        state.update(cx, |view, cx| {
            if view
                .flow
                .apply_failure(&failure_owner, failure_epoch, error.clone())
            {
                if classification {
                    view.existing_probe = ExistingProbeState::Error(error.clone());
                }
                if matches!(
                    error.kind,
                    OnboardingErrorKind::Authentication
                        | OnboardingErrorKind::DisconnectedBeforeDispatch
                ) {
                    view.flow
                        .set_host_status(HostStatus::Error(error.message.clone()));
                }
                cx.notify();
            }
        });
        return;
    }

    let authority = match &completion {
        BackgroundCompletion::Operation(Ok(result)) => result.authority(),
        BackgroundCompletion::Classification(Ok(probe)) => {
            OperationResultAuthority::normal(probe.observed_connection_epoch)
        }
        BackgroundCompletion::Operation(Err(_)) | BackgroundCompletion::Classification(Err(_)) => {
            unreachable!()
        }
    };
    let observed_epoch = authority.observed_connection_epoch;
    let Some(owner) = reconcile_success_owner(state, owner, authority, cx) else {
        invalidate_changed_remote_authority(state, owner, window, cx);
        return;
    };
    if !completion_authority_is_current(state, &owner, observed_epoch, cx) {
        invalidate_changed_remote_authority(state, &owner, window, cx);
        return;
    }

    match completion {
        BackgroundCompletion::Classification(Ok(probe)) => {
            complete_classification(state, &owner, observed_epoch, Ok(probe), window, cx);
        }
        BackgroundCompletion::Operation(Ok(OnboardingOperationResult::NestedRepository {
            selected_path,
            repository_root,
            common_dir,
            authority,
        })) => {
            let observed_connection_epoch = authority.observed_connection_epoch;
            state.update(cx, |view, cx| {
                if view
                    .flow
                    .apply_neutral_result(&owner, observed_connection_epoch)
                {
                    view.existing_probe = ExistingProbeState::Ready(HostPathProbe {
                        canonical_path: selected_path,
                        directory_empty: None,
                        git: GitRelationship::NestedInRepository {
                            top_level: repository_root,
                            common_dir,
                        },
                        observed_connection_epoch,
                    });
                    cx.notify();
                }
            });
        }
        BackgroundCompletion::Operation(Ok(OnboardingOperationResult::ReadyToRegister(
            location,
        ))) => register_completed_project(state, &owner, location, window, cx),
        BackgroundCompletion::Operation(Err(_)) | BackgroundCompletion::Classification(Err(_)) => {
            unreachable!()
        }
    }
}

fn failure_owner_identity_is_current(
    state: &Entity<ProjectOnboardingView>,
    owner: &OperationOwner,
    cx: &App,
) -> bool {
    if !state.read(cx).operation_context_matches(owner) {
        return false;
    }
    match &owner.host_signature {
        HostSignature::Local => state.read(cx).flow.owns(owner, None),
        HostSignature::Ssh {
            connection_id,
            connection_fingerprint,
        } => {
            if !ssh_failure_epoch_is_current(
                owner.expected_connection_epoch,
                crate::remote_ssh::current_connection_epoch(connection_id),
            ) {
                return false;
            }
            if !state
                .read(cx)
                .flow
                .owns(owner, owner.expected_connection_epoch)
            {
                return false;
            }
            state
                .read(cx)
                .store
                .read(cx)
                .ssh_connections()
                .iter()
                .find(|connection| connection.id == *connection_id)
                .is_some_and(|connection| {
                    crate::remote_ssh::connection_fingerprint(connection) == *connection_fingerprint
                })
        }
    }
}

fn reconcile_success_owner(
    state: &Entity<ProjectOnboardingView>,
    owner: &OperationOwner,
    authority: OperationResultAuthority,
    cx: &mut App,
) -> Option<OperationOwner> {
    let (current_connection_fingerprint, current_connection_epoch) = match &owner.host_signature {
        HostSignature::Local => (None, None),
        HostSignature::Ssh { connection_id, .. } => {
            let fingerprint = state
                .read(cx)
                .store
                .read(cx)
                .ssh_connections()
                .iter()
                .find(|connection| connection.id == *connection_id)
                .map(crate::remote_ssh::connection_fingerprint);
            (
                fingerprint,
                crate::remote_ssh::current_connection_epoch(connection_id),
            )
        }
    };
    state.update(cx, |view, _cx| {
        view.flow.reconcile_completion_owner(
            owner,
            authority,
            current_connection_fingerprint,
            current_connection_epoch,
        )
    })
}

fn complete_classification(
    state: &Entity<ProjectOnboardingView>,
    owner: &OperationOwner,
    observed_epoch: Option<u64>,
    result: Result<HostPathProbe, OnboardingError>,
    window: &mut Window,
    cx: &mut App,
) {
    match result {
        Ok(probe) => {
            let canonical = probe.canonical_path.clone();
            let input = state.update(cx, |view, cx| {
                if !view.flow.apply_neutral_result(owner, observed_epoch) {
                    return None;
                }
                view.existing_probe = ExistingProbeState::Ready(probe);
                view.applying_existing_path = true;
                cx.notify();
                Some(view.existing_path.clone())
            });
            if let Some(input) = input {
                input.update(cx, |input, cx| input.set_value(canonical, window, cx));
                state.update(cx, |view, _cx| view.applying_existing_path = false);
            }
        }
        Err(error) => {
            state.update(cx, |view, cx| {
                if view
                    .flow
                    .apply_failure(owner, observed_epoch, error.clone())
                {
                    view.existing_probe = ExistingProbeState::Error(error);
                    cx.notify();
                }
            });
        }
    }
}

fn completion_authority_is_current(
    state: &Entity<ProjectOnboardingView>,
    owner: &OperationOwner,
    observed_epoch: Option<u64>,
    cx: &App,
) -> bool {
    match &owner.host_signature {
        HostSignature::Local => observed_epoch.is_none() && state.read(cx).flow.owns(owner, None),
        HostSignature::Ssh {
            connection_id,
            connection_fingerprint,
        } => {
            let Some(epoch) = observed_epoch else {
                return false;
            };
            if owner.expected_connection_epoch != Some(epoch)
                || crate::remote_ssh::current_connection_epoch(connection_id) != Some(epoch)
                || !state.read(cx).flow.owns(owner, Some(epoch))
            {
                return false;
            }
            state
                .read(cx)
                .store
                .read(cx)
                .ssh_connections()
                .iter()
                .find(|connection| connection.id == *connection_id)
                .is_some_and(|connection| {
                    crate::remote_ssh::connection_fingerprint(connection) == *connection_fingerprint
                })
        }
    }
}

fn invalidate_changed_remote_authority(
    state: &Entity<ProjectOnboardingView>,
    owner: &OperationOwner,
    window: &mut Window,
    cx: &mut App,
) {
    let inputs = state.update(cx, |view, cx| {
        if !view.operation_context_matches(owner) {
            return None;
        }
        let latest_host = match &owner.host_signature {
            HostSignature::Local => return None,
            HostSignature::Ssh { connection_id, .. } => view
                .store
                .read(cx)
                .ssh_connections()
                .iter()
                .find(|connection| connection.id == *connection_id)
                .cloned()
                .map(|connection| ProjectHostSelection::Ssh {
                    connection_fingerprint: crate::remote_ssh::connection_fingerprint(&connection),
                    connection,
                })
                .unwrap_or_else(|| view.flow.host.clone()),
        };
        if view.flow.switch_host(latest_host).is_err() {
            cx.notify();
            return None;
        }
        let error = OnboardingError::new(
            OnboardingErrorKind::StaleOperation,
            t("projectOnboarding", "error.disconnected"),
        );
        view.flow.phase = OperationPhase::Failure(error);
        view.flow.set_host_status(HostStatus::Error(
            t("projectOnboarding", "error.disconnected").to_string(),
        ));
        view.existing_probe = ExistingProbeState::Unselected;
        cx.notify();
        Some((
            view.clone_parent.clone(),
            view.create_parent.clone(),
            view.existing_path.clone(),
        ))
    });
    if let Some((clone_parent, create_parent, existing_path)) = inputs {
        clone_parent.update(cx, |input, cx| input.set_value("~", window, cx));
        create_parent.update(cx, |input, cx| input.set_value("~", window, cx));
        existing_path.update(cx, |input, cx| input.set_value(String::new(), window, cx));
    }
}

fn register_completed_project(
    state: &Entity<ProjectOnboardingView>,
    owner: &OperationOwner,
    location: VerifiedProjectLocation,
    window: &mut Window,
    cx: &mut App,
) {
    let observed_epoch = location.authority.observed_connection_epoch;
    if !completion_authority_is_current(state, owner, observed_epoch, cx) {
        invalidate_changed_remote_authority(state, owner, window, cx);
        return;
    }
    let (store, target_group) = {
        let view = state.read(cx);
        (view.store.clone(), view.target_group.clone())
    };
    let registration = store.update(cx, |store, cx| {
        store.register_or_activate_project(
            location.key,
            &location.canonical_path,
            Some(&location.suggested_name),
            target_group.as_deref(),
            cx,
        )
    });
    match registration {
        Ok(outcome) => {
            let should_close = state.update(cx, |view, cx| {
                if !view.flow.apply_success(owner, observed_epoch) {
                    return false;
                }
                let _ = view.flow.close();
                cx.notify();
                true
            });
            if !should_close {
                return;
            }
            if close_guarded(kind::ADD_PROJECT, window, cx) {
                crate::workbench_area::reactivate_active_page(
                    &outcome.project_id,
                    &outcome.worktree_id,
                    window,
                    cx,
                );
            }
        }
        Err(error) => {
            state.update(cx, |view, cx| {
                let error = OnboardingError::new(OnboardingErrorKind::Registration, error);
                if view.flow.apply_failure(owner, observed_epoch, error) {
                    cx.notify();
                }
            });
        }
    }
}

fn render_shell(
    state: &Entity<ProjectOnboardingView>,
    height: gpui::Pixels,
    cx: &mut App,
) -> AnyElement {
    let page = state.read(cx).flow.page;
    let body = match page {
        OnboardingPage::Home => render_home(state, cx),
        OnboardingPage::Clone => render_clone_page(state, cx),
        OnboardingPage::Create => render_create_page(state, cx),
    };
    div()
        .h(height)
        .w_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(ui::bg_elevated())
        .text_color(ui::text_primary())
        .child(render_header(state, page, cx))
        .child(
            div()
                .id("project-onboarding-body-scroll")
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .px(px(22.0))
                .py(px(18.0))
                .child(body),
        )
        .into_any_element()
}

fn render_header(
    state: &Entity<ProjectOnboardingView>,
    page: OnboardingPage,
    cx: &App,
) -> AnyElement {
    let title = match page {
        OnboardingPage::Home => t("projectOnboarding", "title"),
        OnboardingPage::Clone => t("projectOnboarding", "clone.title"),
        OnboardingPage::Create => t("projectOnboarding", "create.title"),
    };
    let terminally_failed = state.read(cx).flow.is_terminally_failed();
    let close_state = state.clone();
    let mut left = div().w(px(86.0)).flex_none();
    if page != OnboardingPage::Home {
        let back_state = state.clone();
        left = left.child(
            div()
                .id("project-onboarding-back")
                .h(px(28.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded(px(4.0))
                .text_size(ui::font_px(12.0))
                .text_color(ui::text_secondary())
                .opacity(if terminally_failed { 0.4 } else { 1.0 })
                .when(!terminally_failed, |el| {
                    el.cursor_pointer()
                        .hover(|el| el.bg(ui::bg_overlay()).text_color(ui::text_primary()))
                })
                .child(VectorIcon::new(BACK_ICON, px(13.0)).ink(ui::text_secondary()))
                .child(t("projectOnboarding", "back"))
                .on_click(move |_: &ClickEvent, window, cx| {
                    if !terminally_failed {
                        navigate_to(&back_state, OnboardingPage::Home, window, cx);
                    }
                }),
        );
    }
    div()
        .h(px(52.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .px(px(18.0))
        .border_b_1()
        .border_color(ui::border_subtle())
        .child(left)
        .child(
            div()
                .flex_1()
                .text_center()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(ui::font_px(15.0))
                .child(title),
        )
        .child(
            div().w(px(86.0)).flex_none().flex().justify_end().child(
                div()
                    .id("project-onboarding-close")
                    .w(px(28.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .hover(|el| el.bg(ui::bg_overlay()))
                    .tooltip(|window, cx| {
                        Tooltip::new(t("projectOnboarding", "close")).build(window, cx)
                    })
                    .child(VectorIcon::new(CLOSE_ICON, px(13.0)).ink(ui::text_muted()))
                    .on_click(move |_: &ClickEvent, window, cx| {
                        close_modal(&close_state, window, cx);
                    }),
            ),
        )
        .into_any_element()
}

fn render_home(state: &Entity<ProjectOnboardingView>, cx: &mut App) -> AnyElement {
    let (ready, blocked, host_error) = {
        let view = state.read(cx);
        (
            matches!(&view.flow.host_status, HostStatus::Ready { .. }),
            view.flow.phase.is_busy() || view.flow.is_terminally_failed(),
            match &view.flow.host_status {
                HostStatus::Error(error) => Some(error.clone()),
                _ => None,
            },
        )
    };
    let enabled = ready && !blocked;
    let add_state = state.clone();
    let clone_state = state.clone();
    let create_state = state.clone();

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(field_label(t("projectOnboarding", "hostLabel")))
                .child(render_host_row(state, true, cx))
                .when_some(host_error, |el, error| {
                    el.child(
                        div()
                            .text_size(ui::font_px(11.0))
                            .text_color(ui::color_error())
                            .child(error),
                    )
                }),
        )
        .child(
            onboarding_action_row(
                "project-onboarding-add-existing",
                t("projectOnboarding", "home.addExisting"),
                t("projectOnboarding", "home.addExistingDescription"),
                FileIcon::folder(false)
                    .size(px(22.0))
                    .color(ui::color_folder())
                    .into_any_element(),
                true,
                enabled,
            )
            .on_click(move |_: &ClickEvent, window, cx| {
                if enabled {
                    open_directory_picker(&add_state, PickerTarget::AddExisting, window, cx);
                }
            }),
        )
        .child(div().h(px(1.0)).mx(px(8.0)).bg(ui::border_subtle()))
        .child(
            onboarding_action_row(
                "project-onboarding-clone",
                t("projectOnboarding", "home.clone"),
                t("projectOnboarding", "home.cloneDescription"),
                VectorIcon::new(crate::activity_bar::GIT, px(20.0))
                    .ink(ui::color_info())
                    .into_any_element(),
                false,
                enabled,
            )
            .on_click(move |_: &ClickEvent, window, cx| {
                if enabled {
                    navigate_to(&clone_state, OnboardingPage::Clone, window, cx);
                }
            }),
        )
        .child(
            onboarding_action_row(
                "project-onboarding-create",
                t("projectOnboarding", "home.create"),
                t("projectOnboarding", "home.createDescription"),
                VectorIcon::new(PLUS_ICON, px(20.0))
                    .ink(ui::color_success())
                    .into_any_element(),
                false,
                enabled,
            )
            .on_click(move |_: &ClickEvent, window, cx| {
                if enabled {
                    navigate_to(&create_state, OnboardingPage::Create, window, cx);
                }
            }),
        )
        .child(render_phase_status(state, cx))
        .into_any_element()
}

fn onboarding_action_row(
    id: &'static str,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    icon: AnyElement,
    prominent: bool,
    enabled: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w_full()
        .min_h(if prominent { px(78.0) } else { px(66.0) })
        .flex()
        .items_center()
        .gap(px(14.0))
        .px(px(14.0))
        .py(px(12.0))
        .rounded(px(7.0))
        .border_1()
        .border_color(if prominent {
            ui::accent()
        } else {
            ui::border_default()
        })
        .bg(if prominent {
            ui::accent_subtle()
        } else {
            ui::bg_base()
        })
        .opacity(if enabled { 1.0 } else { 0.45 })
        .when(enabled, |el| {
            el.cursor_pointer()
                .hover(|el| el.border_color(ui::accent()).bg(ui::bg_overlay()))
        })
        .child(
            div()
                .w(px(38.0))
                .h(px(38.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .bg(ui::bg_elevated())
                .child(icon),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(ui::font_px(13.0))
                        .text_color(ui::text_primary())
                        .child(title.into()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(ui::font_px(11.0))
                        .text_color(ui::text_muted())
                        .child(description.into()),
                ),
        )
        .child(
            VectorIcon::new(BACK_ICON, px(13.0))
                .ink(ui::text_muted())
                .rotation(0.5),
        )
}

fn render_clone_page(state: &Entity<ProjectOnboardingView>, cx: &mut App) -> AnyElement {
    let (url, parent, typed_name, blocked, host_ready, url_input, parent_input, name_input) = {
        let view = state.read(cx);
        (
            view.clone_url.read(cx).value().trim().to_string(),
            view.clone_parent.read(cx).value().trim().to_string(),
            view.clone_name.read(cx).value().trim().to_string(),
            view.flow.phase.is_busy() || view.flow.is_terminally_failed(),
            matches!(&view.flow.host_status, HostStatus::Ready { .. }),
            view.clone_url.clone(),
            view.clone_parent.clone(),
            view.clone_name.clone(),
        )
    };
    let inferred = infer_clone_folder_name(&url).ok();
    let effective_name = if typed_name.is_empty() {
        inferred.clone().unwrap_or_default()
    } else {
        typed_name.clone()
    };
    let suggested_name = typed_name.is_empty().then(|| inferred.clone()).flatten();
    let url_error = (!url.is_empty() && inferred.is_none())
        .then_some(t("projectOnboarding", "error.invalidUrl").to_string());
    let name_error = (!effective_name.is_empty()
        && validate_portable_basename(&effective_name).is_err())
    .then_some(t("projectOnboarding", "error.invalidName").to_string());
    let target = target_preview(&state.read(cx).flow.host, &parent, &effective_name);
    let enabled = host_ready
        && !blocked
        && !url.is_empty()
        && inferred.is_some()
        && !parent.is_empty()
        && !effective_name.is_empty()
        && name_error.is_none();
    let browse_state = state.clone();
    let submit_state = state.clone();
    let suggested_input = name_input.clone();

    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(15.0))
        .child(render_host_row(state, false, cx))
        .child(render_input_field(
            t("projectOnboarding", "clone.urlLabel"),
            &url_input,
            blocked,
            url_error,
        ))
        .child(render_path_field(
            "project-onboarding-clone-parent-browse",
            t("projectOnboarding", "clone.parentLabel"),
            &parent_input,
            blocked,
            move |window, cx| {
                open_directory_picker(&browse_state, PickerTarget::CloneParent, window, cx);
            },
        ))
        .child(render_input_field(
            t("projectOnboarding", "clone.folderNameLabel"),
            &name_input,
            blocked,
            name_error,
        ))
        .when_some(suggested_name, |el, suggested_name| {
            let value = suggested_name.clone();
            el.child(
                div()
                    .id("project-onboarding-use-suggested-name")
                    .w_full()
                    .min_h(px(28.0))
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .overflow_hidden()
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::accent())
                    .opacity(if blocked { 0.4 } else { 1.0 })
                    .when(!blocked, |el| {
                        el.cursor_pointer().hover(|el| el.bg(ui::accent_subtle()))
                    })
                    .child(div().flex_1().min_w(px(0.0)).truncate().child(tr!(
                        "projectOnboarding",
                        "clone.useSuggestedName",
                        name = suggested_name
                    )))
                    .on_click(move |_: &ClickEvent, window, cx| {
                        if !blocked {
                            suggested_input
                                .update(cx, |input, cx| input.set_value(value.clone(), window, cx));
                        }
                    }),
            )
        })
        .child(render_target_preview(
            t("projectOnboarding", "clone.targetLabel"),
            target,
        ))
        .child(
            div().flex().justify_end().child(
                ui::primary_button(
                    "project-onboarding-clone-submit",
                    t("projectOnboarding", "clone.submit"),
                )
                .opacity(if enabled { 1.0 } else { 0.4 })
                .on_click(move |_: &ClickEvent, window, cx| {
                    if !enabled {
                        return;
                    }
                    start_operation(
                        &submit_state,
                        PendingOperation::Clone {
                            url: url.clone(),
                            parent: parent.clone(),
                            name: effective_name.clone(),
                        },
                        window,
                        cx,
                    );
                }),
            ),
        )
        .child(render_phase_status(state, cx))
        .into_any_element()
}

fn render_create_page(state: &Entity<ProjectOnboardingView>, cx: &mut App) -> AnyElement {
    let (mode, blocked) = {
        let view = state.read(cx);
        (
            view.flow.create_mode,
            view.flow.phase.is_busy() || view.flow.is_terminally_failed(),
        )
    };
    let new_state = state.clone();
    let existing_state = state.clone();
    let body = match mode {
        CreateMode::NewFolder => render_new_folder_form(state, cx),
        CreateMode::InitializeExisting => render_existing_folder_form(state, cx),
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(15.0))
        .child(render_host_row(state, false, cx))
        .child(
            div()
                .w_full()
                .flex()
                .gap(px(4.0))
                .child(
                    ui::choice_button(
                        "project-onboarding-create-new-mode",
                        t("projectOnboarding", "create.newFolderMode"),
                        mode == CreateMode::NewFolder,
                    )
                    .opacity(if blocked { 0.55 } else { 1.0 })
                    .on_click(move |_: &ClickEvent, window, cx| {
                        if !blocked {
                            switch_create_mode(&new_state, CreateMode::NewFolder, window, cx);
                        }
                    }),
                )
                .child(
                    ui::choice_button(
                        "project-onboarding-create-existing-mode",
                        t("projectOnboarding", "create.initializeExistingMode"),
                        mode == CreateMode::InitializeExisting,
                    )
                    .opacity(if blocked { 0.55 } else { 1.0 })
                    .on_click(move |_: &ClickEvent, window, cx| {
                        if !blocked {
                            switch_create_mode(
                                &existing_state,
                                CreateMode::InitializeExisting,
                                window,
                                cx,
                            );
                        }
                    }),
                ),
        )
        .child(body)
        .child(render_phase_status(state, cx))
        .into_any_element()
}

fn render_new_folder_form(state: &Entity<ProjectOnboardingView>, cx: &mut App) -> AnyElement {
    let (name, parent, blocked, host_ready, name_input, parent_input) = {
        let view = state.read(cx);
        (
            view.create_name.read(cx).value().trim().to_string(),
            view.create_parent.read(cx).value().trim().to_string(),
            view.flow.phase.is_busy() || view.flow.is_terminally_failed(),
            matches!(&view.flow.host_status, HostStatus::Ready { .. }),
            view.create_name.clone(),
            view.create_parent.clone(),
        )
    };
    let name_error = (!name.is_empty() && validate_portable_basename(&name).is_err())
        .then_some(t("projectOnboarding", "error.invalidName").to_string());
    let target = target_preview(&state.read(cx).flow.host, &parent, &name);
    let enabled =
        host_ready && !blocked && !parent.is_empty() && !name.is_empty() && name_error.is_none();
    let browse_state = state.clone();
    let submit_state = state.clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(15.0))
        .child(render_input_field(
            t("projectOnboarding", "create.nameLabel"),
            &name_input,
            blocked,
            name_error,
        ))
        .child(render_path_field(
            "project-onboarding-create-parent-browse",
            t("projectOnboarding", "create.parentLabel"),
            &parent_input,
            blocked,
            move |window, cx| {
                open_directory_picker(&browse_state, PickerTarget::CreateParent, window, cx);
            },
        ))
        .child(render_target_preview(
            t("projectOnboarding", "create.targetLabel"),
            target,
        ))
        .child(
            div().flex().justify_end().child(
                ui::primary_button(
                    "project-onboarding-create-submit",
                    t("projectOnboarding", "create.createAndInitialize"),
                )
                .opacity(if enabled { 1.0 } else { 0.4 })
                .on_click(move |_: &ClickEvent, window, cx| {
                    if enabled {
                        start_operation(
                            &submit_state,
                            PendingOperation::CreateNew {
                                parent: parent.clone(),
                                name: name.clone(),
                            },
                            window,
                            cx,
                        );
                    }
                }),
            ),
        )
        .into_any_element()
}

fn render_existing_folder_form(state: &Entity<ProjectOnboardingView>, cx: &mut App) -> AnyElement {
    let (path, blocked, host_ready, input, probe) = {
        let view = state.read(cx);
        (
            view.existing_path.read(cx).value().trim().to_string(),
            view.flow.phase.is_busy() || view.flow.is_terminally_failed(),
            matches!(&view.flow.host_status, HostStatus::Ready { .. }),
            view.existing_path.clone(),
            view.existing_probe.clone(),
        )
    };
    let (label, enabled, action_path, relationship) = match &probe {
        ExistingProbeState::Ready(HostPathProbe {
            canonical_path,
            git: GitRelationship::NotGit,
            ..
        }) => (
            t("projectOnboarding", "create.initializeAndAdd"),
            host_ready && !blocked,
            Some(canonical_path.clone()),
            Some(
                div()
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_secondary())
                    .child(t("projectOnboarding", "create.nonGitDetected"))
                    .into_any_element(),
            ),
        ),
        ExistingProbeState::Ready(HostPathProbe {
            canonical_path,
            git: GitRelationship::RepositoryRoot { .. },
            ..
        }) => (
            t("projectOnboarding", "create.addProject"),
            host_ready && !blocked,
            Some(canonical_path.clone()),
            Some(
                div()
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::color_success())
                    .child(t("projectOnboarding", "create.repositoryRootDetected"))
                    .into_any_element(),
            ),
        ),
        ExistingProbeState::Ready(HostPathProbe {
            git:
                GitRelationship::NestedInRepository {
                    top_level,
                    common_dir: _,
                },
            ..
        }) => (
            t("projectOnboarding", "create.useRepositoryRoot"),
            host_ready && !blocked,
            Some(top_level.clone()),
            Some(
                div()
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::color_warning())
                    .child(tr!(
                        "projectOnboarding",
                        "create.nestedRepository",
                        path = top_level.clone()
                    ))
                    .into_any_element(),
            ),
        ),
        ExistingProbeState::Probing => (
            t("projectOnboarding", "create.inspectFolder"),
            false,
            None,
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(7.0))
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    .child(ui::spinner(px(12.0), ui::text_muted()))
                    .child(t("projectOnboarding", "status.validating"))
                    .into_any_element(),
            ),
        ),
        ExistingProbeState::Error(error) => (
            t("projectOnboarding", "create.inspectFolder"),
            host_ready && !blocked && !path.is_empty(),
            (!path.is_empty()).then_some(path.clone()),
            Some(
                div()
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::color_error())
                    .child(localized_error(error))
                    .into_any_element(),
            ),
        ),
        ExistingProbeState::Unselected => (
            t("projectOnboarding", "create.inspectFolder"),
            host_ready && !blocked && !path.is_empty(),
            (!path.is_empty()).then_some(path.clone()),
            None,
        ),
    };
    let browse_state = state.clone();
    let submit_state = state.clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .child(render_path_field(
            "project-onboarding-existing-folder-browse",
            t("projectOnboarding", "create.existingFolderLabel"),
            &input,
            blocked,
            move |window, cx| {
                open_directory_picker(&browse_state, PickerTarget::InitializeExisting, window, cx);
            },
        ))
        .when_some(relationship, |el, relationship| el.child(relationship))
        .child(
            div().flex().justify_end().child(
                ui::primary_button("project-onboarding-existing-submit", label)
                    .opacity(if enabled { 1.0 } else { 0.4 })
                    .on_click(move |_: &ClickEvent, window, cx| {
                        if !enabled {
                            return;
                        }
                        let Some(path) = action_path.clone() else {
                            return;
                        };
                        let operation = match &probe {
                            ExistingProbeState::Ready(HostPathProbe {
                                git: GitRelationship::NestedInRepository { .. },
                                ..
                            }) => PendingOperation::AddExisting { path },
                            ExistingProbeState::Unselected | ExistingProbeState::Error(_) => {
                                PendingOperation::ClassifyExisting { path }
                            }
                            _ => PendingOperation::InitializeExisting { path },
                        };
                        start_operation(&submit_state, operation, window, cx);
                    }),
            ),
        )
        .when(path.is_empty(), |el| {
            el.child(
                div()
                    .text_size(ui::font_px(11.0))
                    .text_color(ui::text_muted())
                    .child(t("projectOnboarding", "error.invalidPath")),
            )
        })
        .into_any_element()
}

fn render_host_row(
    state: &Entity<ProjectOnboardingView>,
    selectable: bool,
    cx: &mut App,
) -> AnyElement {
    let (host, status, blocked) = {
        let view = state.read(cx);
        (
            view.flow.host.clone(),
            view.flow.host_status.clone(),
            view.flow.phase.is_busy() || view.flow.is_terminally_failed(),
        )
    };
    let (name, summary, icon) = match host {
        ProjectHostSelection::Local => (
            t("projectOnboarding", "localHost").to_string(),
            String::new(),
            VectorIcon::new(COMPUTER_ICON, px(17.0))
                .ink(ui::text_secondary())
                .into_any_element(),
        ),
        ProjectHostSelection::Ssh { connection, .. } => {
            let summary = connection_summary(&connection);
            let (name, detail) = if connection.name.trim().is_empty() {
                (summary, String::new())
            } else {
                (connection.name.clone(), summary)
            };
            (
                name,
                detail,
                VectorIcon::new(crate::activity_bar::SSH, px(17.0))
                    .ink(ui::text_secondary())
                    .into_any_element(),
            )
        }
    };
    let (status_text, status_color) = match status {
        HostStatus::Ready { .. } => (
            t("projectOnboarding", "hostStatus.ready").to_string(),
            ui::color_success(),
        ),
        HostStatus::Connecting => (
            t("projectOnboarding", "hostStatus.connecting").to_string(),
            ui::color_warning(),
        ),
        HostStatus::NotConnected => (
            t("projectOnboarding", "hostStatus.notConnected").to_string(),
            ui::text_muted(),
        ),
        HostStatus::Error(_) => (
            t("projectOnboarding", "hostStatus.error").to_string(),
            ui::color_error(),
        ),
    };
    let menu_state = state.clone();
    div()
        .id(if selectable {
            "project-onboarding-host-selector"
        } else {
            "project-onboarding-host-summary"
        })
        .w_full()
        .min_h(px(54.0))
        .flex()
        .items_center()
        .gap(px(11.0))
        .px(px(12.0))
        .py(px(9.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(ui::border_default())
        .bg(ui::bg_base())
        .when(selectable && !blocked, |el| {
            el.cursor_pointer()
                .hover(|el| el.border_color(ui::accent()).bg(ui::bg_overlay()))
        })
        .child(
            div()
                .w(px(28.0))
                .h(px(28.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.0))
                .bg(ui::bg_elevated())
                .child(icon),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .truncate()
                        .text_size(ui::font_px(12.0))
                        .text_color(ui::text_primary())
                        .child(name),
                )
                .when(!summary.is_empty(), |el| {
                    el.child(
                        div()
                            .truncate()
                            .font_family("monospace")
                            .text_size(ui::font_px(10.0))
                            .text_color(ui::text_muted())
                            .child(summary),
                    )
                }),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(div().w(px(6.0)).h(px(6.0)).rounded_full().bg(status_color))
                .child(
                    div()
                        .text_size(ui::font_px(10.0))
                        .text_color(ui::text_muted())
                        .child(status_text),
                )
                .when(selectable, |el| {
                    el.child(VectorIcon::new(CHEVRON_DOWN, px(11.0)).ink(ui::text_muted()))
                }),
        )
        .on_click(move |event: &ClickEvent, window, cx| {
            if selectable && !blocked {
                open_host_menu(&menu_state, event.position(), window, cx);
            }
        })
        .into_any_element()
}

fn render_input_field(
    label: impl Into<SharedString>,
    input: &Entity<InputState>,
    disabled: bool,
    error: Option<String>,
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(field_label(label))
        .child(Input::new(input).disabled(disabled))
        .when_some(error, |el, error| {
            el.child(
                div()
                    .text_size(ui::font_px(10.0))
                    .text_color(ui::color_error())
                    .child(error),
            )
        })
        .into_any_element()
}

fn render_path_field(
    browse_id: &'static str,
    label: impl Into<SharedString>,
    input: &Entity<InputState>,
    disabled: bool,
    on_browse: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(field_label(label))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(Input::new(input).disabled(disabled)),
                )
                .child(
                    ui::ghost_button(browse_id, t("projectOnboarding", "browse"))
                        .opacity(if disabled { 0.4 } else { 1.0 })
                        .on_click(move |_: &ClickEvent, window, cx| {
                            if !disabled {
                                on_browse(window, cx);
                            }
                        }),
                ),
        )
        .into_any_element()
}

fn field_label(label: impl Into<SharedString>) -> AnyElement {
    div()
        .text_size(ui::font_px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ui::text_secondary())
        .child(label.into())
        .into_any_element()
}

fn render_target_preview(label: impl Into<SharedString>, target: Option<String>) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(field_label(label))
        .child(
            div()
                .w_full()
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(5.0))
                .border_1()
                .border_color(ui::border_subtle())
                .bg(ui::bg_base())
                .truncate()
                .font_family("monospace")
                .text_size(ui::font_px(11.0))
                .text_color(if target.is_some() {
                    ui::text_secondary()
                } else {
                    ui::text_muted()
                })
                .child(target.unwrap_or_else(|| "-".to_string())),
        )
        .into_any_element()
}

fn target_preview(host: &ProjectHostSelection, parent: &str, name: &str) -> Option<String> {
    if parent.is_empty() || name.is_empty() {
        return None;
    }
    Some(match host {
        ProjectHostSelection::Local => std::path::Path::new(parent)
            .join(name)
            .to_string_lossy()
            .to_string(),
        ProjectHostSelection::Ssh { .. } => crate::remote_ssh::join_posix(parent, name),
    })
}

fn render_phase_status(state: &Entity<ProjectOnboardingView>, cx: &App) -> AnyElement {
    let phase = state.read(cx).flow.phase.clone();
    match phase {
        OperationPhase::Idle | OperationPhase::Success => div().into_any_element(),
        OperationPhase::Validating => div()
            .flex()
            .items_center()
            .gap(px(7.0))
            .text_size(ui::font_px(11.0))
            .text_color(ui::text_muted())
            .child(ui::spinner(px(12.0), ui::text_muted()))
            .child(t("projectOnboarding", "status.validating"))
            .into_any_element(),
        OperationPhase::Running => div()
            .flex()
            .items_center()
            .gap(px(7.0))
            .text_size(ui::font_px(11.0))
            .text_color(ui::text_secondary())
            .child(ui::spinner(px(12.0), ui::accent()))
            .child(t("projectOnboarding", "status.running"))
            .into_any_element(),
        OperationPhase::Failure(error) => div()
            .w_full()
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(5.0))
            .bg(ui::bg_base())
            .border_1()
            .border_color(ui::color_error())
            .text_size(ui::font_px(11.0))
            .text_color(ui::color_error())
            .child(localized_error(&error))
            .into_any_element(),
    }
}

fn localized_error(error: &OnboardingError) -> String {
    match error.kind {
        OnboardingErrorKind::Collision => {
            format!(
                "{}: {}",
                t("projectOnboarding", "error.targetExists"),
                error.message
            )
        }
        OnboardingErrorKind::GitUnavailable => format!(
            "{}: {}",
            t("projectOnboarding", "error.gitUnavailable"),
            error.message
        ),
        OnboardingErrorKind::Authentication => format!(
            "{}: {}",
            t("projectOnboarding", "error.authentication"),
            error.message
        ),
        OnboardingErrorKind::DisconnectedBeforeDispatch | OnboardingErrorKind::StaleOperation => {
            format!(
                "{}: {}",
                t("projectOnboarding", "error.disconnected"),
                error.message
            )
        }
        OnboardingErrorKind::RemoteOutcomeUncertain => tr!(
            "projectOnboarding",
            "error.outcomeUncertain",
            detail = error.message.clone()
        ),
        OnboardingErrorKind::Registration => tr!(
            "projectOnboarding",
            "error.registrationFailed",
            detail = error.message.clone()
        ),
        OnboardingErrorKind::Validation
        | OnboardingErrorKind::GitFailure
        | OnboardingErrorKind::PostconditionFailed
        | OnboardingErrorKind::GenerationOverflow => tr!(
            "projectOnboarding",
            "error.operationFailed",
            detail = error.message.clone()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        picker_request_is_current, ssh_failure_epoch_is_current, ssh_operation_authority_is_current,
    };

    #[test]
    fn project_onboarding_picker_accepts_only_the_latest_idle_request() {
        assert!(picker_request_is_current(Some(7), 7, false));
        assert!(!picker_request_is_current(Some(8), 7, false));
        assert!(!picker_request_is_current(None, 7, false));
        assert!(!picker_request_is_current(Some(7), 7, true));
    }

    #[test]
    fn project_onboarding_failure_requires_the_original_current_ssh_epoch() {
        assert!(ssh_failure_epoch_is_current(Some(7), Some(7)));
        assert!(!ssh_failure_epoch_is_current(Some(7), Some(8)));
        assert!(!ssh_failure_epoch_is_current(Some(7), None));
        assert!(!ssh_failure_epoch_is_current(None, None));
    }

    #[test]
    fn project_onboarding_dispatch_requires_current_ssh_fingerprint_and_epoch() {
        assert!(ssh_operation_authority_is_current(
            11,
            Some(11),
            Some(7),
            Some(7)
        ));
        assert!(!ssh_operation_authority_is_current(
            11,
            Some(12),
            Some(7),
            Some(7)
        ));
        assert!(!ssh_operation_authority_is_current(
            11,
            Some(11),
            Some(7),
            Some(8)
        ));
    }
}
