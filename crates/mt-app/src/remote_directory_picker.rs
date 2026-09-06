//! Host-aware, read-only folder browser for the existing onboarding flow.

use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Pixels, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::scroll::Scrollbar;
use mt_ui::icon_tooltip::IconTooltips;
use mt_ui::icons::{FileIcon, Geom, Ink, Shape, VectorIcon};
use mt_ui::tooltip::Tooltip;

use crate::i18n::t;
use crate::project_onboarding::local::{browse_local_directory, local_browser_places};
use crate::project_onboarding::{
    DirectoryBrowseError, DirectoryBrowseErrorKind, DirectoryEntry, DirectoryListing,
    DirectoryLocation, DirectorySource, ProjectHostSelection,
};
use crate::prompt::{close_guarded, kind, open_guarded_with_close};
use crate::ui;

const HOME_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.075,
        Geom::Polyline(&[(0.12, 0.46), (0.50, 0.14), (0.88, 0.46)]),
    ),
    Shape::line(
        Ink::Current,
        0.075,
        Geom::Polyline(&[
            (0.24, 0.40),
            (0.24, 0.84),
            (0.43, 0.84),
            (0.43, 0.58),
            (0.60, 0.58),
            (0.60, 0.84),
            (0.76, 0.84),
            (0.76, 0.40),
        ]),
    ),
];
const UP_ICON: &[Shape] = &[
    Shape::line(
        Ink::Current,
        0.085,
        Geom::Polyline(&[(0.22, 0.44), (0.50, 0.16), (0.78, 0.44)]),
    ),
    Shape::line(
        Ink::Current,
        0.085,
        Geom::Polyline(&[(0.50, 0.18), (0.50, 0.84)]),
    ),
];
const CHEVRON_ICON: &[Shape] = &[Shape::line(
    Ink::Current,
    0.085,
    Geom::Polyline(&[(0.36, 0.24), (0.62, 0.50), (0.36, 0.76)]),
)];

type SelectCallback = Rc<dyn Fn(String, &mut Window, &mut App)>;
type CancelCallback = Rc<dyn Fn(&mut Window, &mut App)>;
type OwnerGuard = Rc<dyn Fn(&App) -> bool>;

pub(crate) struct DirectoryPickerOptions {
    pub host: ProjectHostSelection,
    pub initial_path: String,
    pub canonical_home: Option<String>,
    pub expected_connection_epoch: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryRequest {
    id: u64,
    location: DirectoryLocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectorySelection {
    request: DirectoryRequest,
    location: DirectoryLocation,
}

impl DirectorySelection {
    fn capture(
        requests: &DirectoryRequests,
        listing: Option<&DirectoryListing>,
        selected: Option<&DirectoryLocation>,
        ready: bool,
    ) -> Option<Self> {
        let request = requests.active.as_ref()?;
        let listing = listing?;
        let selected = selected?;
        if !requests.owns(request) || listing.location.source != request.location.source {
            return None;
        }
        selected_host_path(Some(listing), Some(selected), ready)?;
        Some(Self {
            request: request.clone(),
            location: selected.clone(),
        })
    }

    fn host_path(
        &self,
        requests: &DirectoryRequests,
        listing: Option<&DirectoryListing>,
        selected: Option<&DirectoryLocation>,
        ready: bool,
    ) -> Option<String> {
        let current = Self::capture(requests, listing, selected, ready)?;
        if current != *self {
            return None;
        }
        self.location.host_path().ok()
    }
}

#[derive(Default)]
struct DirectoryRequests {
    next_id: u64,
    active: Option<DirectoryRequest>,
    closed: bool,
    exhausted: bool,
}

impl DirectoryRequests {
    fn begin(&mut self, location: DirectoryLocation) -> Option<DirectoryRequest> {
        if self.closed || self.exhausted {
            return None;
        }
        let Some(id) = self.next_id.checked_add(1) else {
            self.exhausted = true;
            self.active = None;
            return None;
        };
        self.next_id = id;
        let request = DirectoryRequest { id, location };
        self.active = Some(request.clone());
        Some(request)
    }

    fn owns(&self, request: &DirectoryRequest) -> bool {
        !self.closed && !self.exhausted && self.active.as_ref() == Some(request)
    }

    fn accepts_callback(&self, id: u64) -> bool {
        self.active
            .as_ref()
            .is_some_and(|request| request.id == id && self.owns(request))
    }

    fn close(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.closed = true;
        self.active = None;
        true
    }
}

struct PickerState {
    options: DirectoryPickerOptions,
    location: DirectoryLocation,
    listing: Option<DirectoryListing>,
    selected: Option<DirectoryLocation>,
    input: Entity<InputState>,
    scroll: ScrollHandle,
    tooltips: Entity<IconTooltips>,
    places: Vec<(String, DirectoryLocation)>,
    requests: DirectoryRequests,
    loading: bool,
    error: Option<DirectoryBrowseError>,
    is_current: OwnerGuard,
    on_select: SelectCallback,
    on_cancel: CancelCallback,
    _subscriptions: Vec<Subscription>,
    _task: Option<Task<()>>,
    _places_task: Option<Task<()>>,
}

impl PickerState {
    fn is_live(&self, cx: &App) -> bool {
        !self.requests.closed
            && !self.requests.exhausted
            && (self.is_current)(cx)
            && source_epoch_is_current(&self.location.source)
    }

    fn load(&mut self, requested: DirectoryLocation, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_live(cx) {
            return;
        }
        let request = self.requests.begin(requested.clone());
        self._task = None;
        self.location = requested.clone();
        self.listing = None;
        self.selected = None;
        self.error = None;
        self.loading = request.is_some();
        self.scroll = ScrollHandle::new();
        self.input.update(cx, |input, cx| {
            input.set_value(requested.path.clone(), window, cx);
        });
        IconTooltips::reset(&self.tooltips, window, cx);
        cx.notify();
        window.refresh();
        let Some(request) = request else {
            return;
        };
        let host = self.options.host.clone();
        let home = self.options.canonical_home.clone();
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let location = request.location.clone();
            let result = cx
                .background_executor()
                .spawn(async move { browse_directory(&host, home.as_deref(), &location) })
                .await;
            let _ = this.update_in(cx, |state, window, cx| {
                if !state.requests.owns(&request) {
                    return;
                }
                if !state.is_live(cx) {
                    window.refresh();
                    return;
                }
                state.loading = false;
                match result {
                    Ok(listing) if listing.location.source == request.location.source => {
                        if state.input.read(cx).value().as_ref() == request.location.path.as_str() {
                            state.input.update(cx, |input, cx| {
                                input.set_value(listing.location.path.clone(), window, cx);
                            });
                        }
                        state.location = listing.location.clone();
                        state.selected = Some(listing.location.clone());
                        state.listing = Some(listing);
                    }
                    Ok(_) => {
                        state.error = Some(DirectoryBrowseError::new(
                            DirectoryBrowseErrorKind::Unavailable,
                            "The directory listing belongs to a different filesystem",
                        ));
                    }
                    Err(error) => state.error = Some(error),
                }
                cx.notify();
                window.refresh();
            });
        }));
    }

    fn submit_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_live(cx) {
            return;
        }
        let input = self.input.read(cx).value().to_string();
        match resolve_input(&self.location, &input) {
            Ok(location) => self.load(location, window, cx),
            Err(error) => {
                // Even an invalid submission supersedes a pending listing.
                let _ = self.requests.begin(self.location.clone());
                self._task = None;
                self.loading = false;
                self.listing = None;
                self.selected = None;
                self.error = Some(error);
                cx.notify();
                window.refresh();
            }
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.requests.close() {
            return false;
        }
        self._task = None;
        self._places_task = None;
        self.selected = None;
        IconTooltips::reset(&self.tooltips, window, cx);
        cx.notify();
        true
    }
}

fn source_epoch_is_current(source: &DirectorySource) -> bool {
    match source {
        DirectorySource::Local | DirectorySource::Wsl { .. } => true,
        DirectorySource::Ssh {
            connection_id,
            connection_epoch,
            ..
        } => crate::remote_ssh::current_connection_epoch(connection_id) == Some(*connection_epoch),
    }
}

fn browse_directory(
    host: &ProjectHostSelection,
    home: Option<&str>,
    requested: &DirectoryLocation,
) -> Result<DirectoryListing, DirectoryBrowseError> {
    match (host, &requested.source) {
        (ProjectHostSelection::Local, DirectorySource::Local | DirectorySource::Wsl { .. }) => {
            browse_local_directory(requested)
        }
        (
            ProjectHostSelection::Ssh {
                connection,
                connection_fingerprint,
            },
            DirectorySource::Ssh {
                connection_id,
                connection_fingerprint: expected,
                connection_epoch,
            },
        ) if connection.id == *connection_id && connection_fingerprint == expected => {
            if !source_epoch_is_current(&requested.source) {
                return Err(unavailable_source());
            }
            let path = if requested.path == "~" || requested.path.starts_with("~/") {
                let home = home.ok_or_else(unavailable_source)?;
                if requested.path == "~" {
                    home.to_string()
                } else {
                    crate::remote_ssh::join_posix(home, &requested.path[2..])
                }
            } else {
                requested.path.clone()
            };
            let context = crate::remote_ssh::RemoteProjectContext::new(
                connection.clone(),
                *connection_fingerprint,
                Some(*connection_epoch),
            );
            let result = crate::remote_ssh::browse_directory(&context, &path);
            if !source_epoch_is_current(&requested.source) {
                return Err(unavailable_source());
            }
            let listing = result.map_err(|error| {
                let kind = match error.kind {
                    crate::remote_ssh::RemoteDirectoryBrowseErrorKind::InvalidPath => {
                        DirectoryBrowseErrorKind::InvalidPath
                    }
                    crate::remote_ssh::RemoteDirectoryBrowseErrorKind::Unavailable => {
                        DirectoryBrowseErrorKind::Unavailable
                    }
                };
                DirectoryBrowseError::new(kind, error.message)
            })?;
            if !listing_authority_matches(&requested.source, &listing) {
                return Err(unavailable_source());
            }
            Ok(DirectoryListing {
                location: DirectoryLocation {
                    source: requested.source.clone(),
                    path: listing.canonical_path,
                },
                directories: listing
                    .directories
                    .into_iter()
                    .map(|entry| DirectoryEntry {
                        name: entry.name,
                        location: DirectoryLocation {
                            source: requested.source.clone(),
                            path: entry.path,
                        },
                        is_symlink: entry.is_symlink,
                    })
                    .collect(),
            })
        }
        _ => Err(unavailable_source()),
    }
}

fn listing_authority_matches(
    source: &DirectorySource,
    listing: &crate::remote_ssh::RemoteDirectoryListing,
) -> bool {
    matches!(
        source,
        DirectorySource::Ssh {
            connection_epoch,
            connection_fingerprint,
            ..
        } if listing.connection_epoch == *connection_epoch
            && listing.connection_fingerprint == *connection_fingerprint
    )
}

fn unavailable_source() -> DirectoryBrowseError {
    DirectoryBrowseError::new(
        DirectoryBrowseErrorKind::Unavailable,
        "The selected execution host changed or disconnected",
    )
}

fn resolve_input(
    current: &DirectoryLocation,
    input: &str,
) -> Result<DirectoryLocation, DirectoryBrowseError> {
    if input.is_empty() || input.contains('\0') {
        return Err(DirectoryBrowseError::new(
            DirectoryBrowseErrorKind::InvalidPath,
            "A directory path is required",
        ));
    }
    if !matches!(&current.source, DirectorySource::Ssh { .. }) {
        if let Some(wsl) = mt_core::parse_wsl_unc(&input.replace('/', "\\")) {
            return Ok(DirectoryLocation {
                source: DirectorySource::Wsl { distro: wsl.distro },
                path: wsl.unix_path,
            });
        }
        if Path::new(input).is_absolute()
            && (matches!(&current.source, DirectorySource::Local) || cfg!(windows))
        {
            return Ok(DirectoryLocation {
                source: DirectorySource::Local,
                path: input.to_string(),
            });
        }
    }
    let path = if input == "~" || input.starts_with("~/") {
        input.to_string()
    } else {
        match &current.source {
            DirectorySource::Local => {
                if !Path::new(&current.path).is_absolute()
                    || Path::new(input)
                        .components()
                        .any(|part| matches!(part, Component::Prefix(_)))
                {
                    return Err(DirectoryBrowseError::new(
                        DirectoryBrowseErrorKind::InvalidPath,
                        "An absolute directory path is required",
                    ));
                }
                Path::new(&current.path)
                    .join(input)
                    .to_string_lossy()
                    .into_owned()
            }
            DirectorySource::Wsl { .. } | DirectorySource::Ssh { .. } => {
                if input.starts_with('/') {
                    input.to_string()
                } else if current.path.starts_with('/') {
                    crate::remote_ssh::join_posix(&current.path, input)
                } else {
                    return Err(DirectoryBrowseError::new(
                        DirectoryBrowseErrorKind::InvalidPath,
                        "An absolute directory path is required",
                    ));
                }
            }
        }
    };
    Ok(DirectoryLocation {
        source: current.source.clone(),
        path,
    })
}

fn parent_location(location: &DirectoryLocation) -> Option<DirectoryLocation> {
    let path = match &location.source {
        DirectorySource::Local => Path::new(&location.path).parent()?.to_str()?.to_string(),
        DirectorySource::Wsl { .. } | DirectorySource::Ssh { .. } => {
            crate::remote_ssh::parent_posix(&location.path)?
        }
    };
    (!path.is_empty()).then(|| DirectoryLocation {
        source: location.source.clone(),
        path,
    })
}

fn breadcrumbs(location: &DirectoryLocation) -> Vec<(String, DirectoryLocation)> {
    let mut crumbs = Vec::new();
    match &location.source {
        DirectorySource::Local => {
            let mut path = PathBuf::new();
            let mut prefix = String::new();
            for component in Path::new(&location.path).components() {
                path.push(component.as_os_str());
                match component {
                    Component::Prefix(_) => {
                        prefix = component.as_os_str().to_string_lossy().into_owned();
                    }
                    Component::RootDir | Component::Normal(_) => {
                        let label = if matches!(component, Component::RootDir) {
                            format!("{prefix}{}", std::path::MAIN_SEPARATOR)
                        } else {
                            component.as_os_str().to_string_lossy().into_owned()
                        };
                        crumbs.push((
                            label,
                            DirectoryLocation {
                                source: location.source.clone(),
                                path: path.to_string_lossy().into_owned(),
                            },
                        ));
                    }
                    Component::CurDir | Component::ParentDir => {}
                }
            }
        }
        DirectorySource::Wsl { .. } | DirectorySource::Ssh { .. } => {
            if !location.path.starts_with('/') {
                return crumbs;
            }
            let mut path = "/".to_string();
            crumbs.push((
                path.clone(),
                DirectoryLocation {
                    source: location.source.clone(),
                    path: path.clone(),
                },
            ));
            for name in location.path.split('/').filter(|name| !name.is_empty()) {
                path = crate::remote_ssh::join_posix(&path, name);
                crumbs.push((
                    name.to_string(),
                    DirectoryLocation {
                        source: location.source.clone(),
                        path: path.clone(),
                    },
                ));
            }
        }
    }
    crumbs
}

fn filter_directories<'a>(listing: &'a DirectoryListing, query: &str) -> Vec<&'a DirectoryEntry> {
    let query = if query == listing.location.path {
        ""
    } else {
        query
    };
    let query = query.to_lowercase();
    listing
        .directories
        .iter()
        .filter(|entry| entry.name.to_lowercase().contains(&query))
        .collect()
}

fn selected_host_path(
    listing: Option<&DirectoryListing>,
    selected: Option<&DirectoryLocation>,
    ready: bool,
) -> Option<String> {
    if !ready {
        return None;
    }
    let listing = listing?;
    let selected = selected?;
    if selected != &listing.location
        && !listing
            .directories
            .iter()
            .any(|entry| &entry.location == selected)
    {
        return None;
    }
    selected.host_path().ok()
}

fn picker_top_margin(viewport_height: Pixels) -> Pixels {
    // gpui-component 0.5.1 adds 16px for the parent dialog and 30px of animation.
    // Include the border when reserving the space around the bounded body.
    (viewport_height / 10.0 - px(48.0)).max(px(0.0))
}

pub(crate) fn open(
    options: DirectoryPickerOptions,
    is_current: impl Fn(&App) -> bool + 'static,
    on_select: impl Fn(String, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    if crate::prompt::is_open(kind::REMOTE_DIRECTORY_PICKER) || !is_current(cx) {
        return;
    }
    let source = match &options.host {
        ProjectHostSelection::Local => DirectorySource::Local,
        ProjectHostSelection::Ssh {
            connection,
            connection_fingerprint,
        } => {
            let Some(connection_epoch) = options.expected_connection_epoch else {
                return;
            };
            DirectorySource::Ssh {
                connection_id: connection.id.clone(),
                connection_fingerprint: *connection_fingerprint,
                connection_epoch,
            }
        }
    };
    let initial = DirectoryLocation {
        source,
        path: "~".into(),
    };
    let initial = if options.initial_path.is_empty() {
        initial
    } else {
        resolve_input(&initial, &options.initial_path).unwrap_or(DirectoryLocation {
            source: initial.source,
            path: options.initial_path.clone(),
        })
    };
    let requested = initial.clone();
    let state = cx.new(|cx| {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t("projectOnboarding", "picker.pathPlaceholder"))
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |state: &mut PickerState, _, event, window, cx| match event {
                InputEvent::PressEnter { .. } => state.submit_path(window, cx),
                InputEvent::Change => {
                    if state.is_live(cx) {
                        state.selected = state
                            .listing
                            .as_ref()
                            .map(|listing| listing.location.clone());
                        state.scroll = ScrollHandle::new();
                        cx.notify();
                        window.refresh();
                    }
                }
                _ => {}
            },
        );
        PickerState {
            options,
            location: initial,
            listing: None,
            selected: None,
            input,
            scroll: ScrollHandle::new(),
            tooltips: cx.new(|_| IconTooltips::default()),
            places: Vec::new(),
            requests: DirectoryRequests::default(),
            loading: false,
            error: None,
            is_current: Rc::new(is_current),
            on_select: Rc::new(on_select),
            on_cancel: Rc::new(on_cancel),
            _subscriptions: vec![subscription],
            _task: None,
            _places_task: None,
        }
    });
    let dialog_state = state.clone();
    let close_state = state.clone();
    if !open_guarded_with_close(
        kind::REMOTE_DIRECTORY_PICKER,
        window,
        cx,
        move |dialog, window, cx| {
            dialog
                .p_0()
                .w(ui::clamp_dialog_width(px(720.0), window.viewport_size()))
                .margin_top(picker_top_margin(window.viewport_size().height))
                .overlay_closable(false)
                .keyboard(true)
                .on_ok(|_, _, _| false)
                .child(render_body(&dialog_state, window, cx))
        },
        move |window, cx| {
            let on_cancel = close_state.read(cx).on_cancel.clone();
            if close_state.update(cx, |state, cx| state.close(window, cx)) {
                on_cancel(window, cx);
            }
        },
    ) {
        return;
    }
    state.update(cx, |state, cx| {
        state.load(requested, window, cx);
        if matches!(&state.options.host, ProjectHostSelection::Local) {
            state._places_task = Some(cx.spawn_in(window, async move |this, cx| {
                let places = cx
                    .background_executor()
                    .spawn(async { local_browser_places() })
                    .await;
                let _ = this.update_in(cx, |state, window, cx| {
                    if state.is_live(cx) {
                        state.places = places;
                        cx.notify();
                        window.refresh();
                    }
                });
            }));
        }
    });
    window.defer(cx, move |window, cx| {
        let picker = state.read(cx);
        if picker.is_live(cx)
            && crate::overlay::is_top(crate::overlay::key(kind::REMOTE_DIRECTORY_PICKER))
        {
            let input = picker.input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    });
}

fn navigate(
    state: &Entity<PickerState>,
    expected_request: u64,
    location: DirectoryLocation,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |state, cx| {
        if state.requests.accepts_callback(expected_request) && state.is_live(cx) {
            state.load(location, window, cx);
        }
    });
}

fn finish(
    state: &Entity<PickerState>,
    selection: Option<&DirectorySelection>,
    window: &mut Window,
    cx: &mut App,
) {
    if !crate::overlay::is_top(crate::overlay::key(kind::REMOTE_DIRECTORY_PICKER)) {
        return;
    }
    let picker = state.read(cx);
    if picker.requests.closed {
        return;
    }
    let selected = if let Some(selection) = selection {
        let Some(selected) = selection.host_path(
            &picker.requests,
            picker.listing.as_ref(),
            picker.selected.as_ref(),
            picker.is_live(cx) && !picker.loading && picker.error.is_none(),
        ) else {
            return;
        };
        Some(selected)
    } else {
        None
    };
    let on_select = picker.on_select.clone();
    let on_cancel = picker.on_cancel.clone();
    if !state.update(cx, |state, cx| state.close(window, cx)) {
        return;
    }
    if close_guarded(kind::REMOTE_DIRECTORY_PICKER, window, cx) {
        if let Some(selected) = selected {
            on_select(selected, window, cx);
        } else {
            on_cancel(window, cx);
        }
    }
}

fn render_body(state: &Entity<PickerState>, window: &mut Window, cx: &mut App) -> AnyElement {
    let picker = state.read(cx);
    let live = picker.is_live(cx);
    let request = picker.requests.next_id;
    let location = picker.location.clone();
    let input = picker.input.clone();
    let tooltips = picker.tooltips.clone();
    let scroll = picker.scroll.clone();
    let selected = picker.selected.clone();
    let selection = DirectorySelection::capture(
        &picker.requests,
        picker.listing.as_ref(),
        selected.as_ref(),
        live && !picker.loading && picker.error.is_none(),
    );
    let can_select = selection.is_some();
    let host_label = match &location.source {
        DirectorySource::Local => t("projectOnboarding", "localHost").to_string(),
        DirectorySource::Wsl { distro } => format!("WSL: {distro}"),
        DirectorySource::Ssh { .. } => match &picker.options.host {
            ProjectHostSelection::Ssh { connection, .. } => {
                format!(
                    "{} ({})",
                    connection.name,
                    crate::ssh_conn::connection_summary(connection)
                )
            }
            ProjectHostSelection::Local => String::new(),
        },
    };
    let status = if picker.requests.exhausted {
        Some(t("projectOnboarding", "error.requestOverflow").to_string())
    } else if !live {
        Some(t("projectOnboarding", "picker.unavailable").to_string())
    } else if picker.loading {
        Some(t("projectOnboarding", "picker.loading").to_string())
    } else {
        picker.error.as_ref().map(|error| {
            let key = match error.kind {
                DirectoryBrowseErrorKind::InvalidPath => "picker.invalidPath",
                DirectoryBrowseErrorKind::PermissionDenied => "picker.permissionDenied",
                DirectoryBrowseErrorKind::Unavailable => "picker.unavailable",
            };
            format!("{}\n{}", t("projectOnboarding", key), error.detail)
        })
    };
    let entries = picker
        .listing
        .as_ref()
        .map(|listing| {
            filter_directories(listing, input.read(cx).value().as_ref())
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unfiltered_empty = picker
        .listing
        .as_ref()
        .is_none_or(|listing| listing.directories.is_empty());
    let loading = picker.loading;
    let has_error = picker.error.is_some() || !live;
    let places = picker.places.clone();
    let local = matches!(&picker.options.host, ProjectHostSelection::Local);

    let mut tool = |id: &'static str, caption: &'static str, icon: VectorIcon, enabled: bool| {
        IconTooltips::button(
            &tooltips,
            id,
            t("projectOnboarding", caption),
            div()
                .id(id)
                .relative()
                .size(px(30.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .opacity(if enabled { 1.0 } else { 0.4 })
                .when(enabled, |el| {
                    el.cursor_pointer().hover(|el| el.bg(ui::bg_overlay()))
                })
                .child(icon.ink(ui::text_secondary())),
            window,
            cx,
        )
    };
    let home_state = state.clone();
    let home = DirectoryLocation {
        source: location.source.clone(),
        path: "~".into(),
    };
    let home_button = tool(
        "directory-home",
        "picker.home",
        VectorIcon::new(HOME_ICON, px(16.0)),
        live,
    )
    .on_click(move |_, window, cx| navigate(&home_state, request, home.clone(), window, cx));
    let up_state = state.clone();
    let up = parent_location(&location);
    let up_button = tool(
        "directory-up",
        "picker.up",
        VectorIcon::new(UP_ICON, px(16.0)),
        live && up.is_some(),
    )
    .on_click(move |_, window, cx| {
        if let Some(up) = &up {
            navigate(&up_state, request, up.clone(), window, cx);
        }
    });
    let go_state = state.clone();
    let go_button = tool(
        "directory-go",
        "picker.go",
        VectorIcon::new(UP_ICON, px(16.0)).rotation(0.25),
        live,
    )
    .on_click(move |_, window, cx| {
        go_state.update(cx, |state, cx| {
            if state.requests.accepts_callback(request) {
                state.submit_path(window, cx);
            }
        });
    });
    let close_state = state.clone();
    let close_button = tool(
        "directory-close",
        "close",
        VectorIcon::new(crate::title_bar::ICON_CLOSE, px(14.0)),
        true,
    )
    .on_click(move |_, window, cx| finish(&close_state, None, window, cx));

    let mut path_segments = div()
        .id("directory-breadcrumbs")
        .flex_1()
        .min_w(px(0.0))
        .h(px(30.0))
        .flex()
        .items_center()
        .overflow_x_scroll();
    for (index, (label, target)) in breadcrumbs(&location).into_iter().enumerate() {
        let state = state.clone();
        let inspect_path = target.path.clone();
        path_segments = path_segments
            .when(index > 0, |el| {
                el.child(VectorIcon::new(CHEVRON_ICON, px(12.0)).ink(ui::text_muted()))
            })
            .child(
                div()
                    .id(SharedString::from(format!("directory-crumb-{index}")))
                    .flex_none()
                    .max_w(px(160.0))
                    .px(px(5.0))
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .hover(|el| el.bg(ui::bg_overlay()))
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(inspect_path.clone())).into())
                    .on_click(move |_, window, cx| {
                        navigate(&state, request, target.clone(), window, cx);
                    })
                    .child(div().truncate().child(label)),
            );
    }
    let locations_state = state.clone();
    let locations_tips = tooltips.clone();
    let locations = div()
        .id("directory-locations")
        .min_w(px(0.0))
        .flex_none()
        .h(px(28.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .when(local && live, |el| {
            el.cursor_pointer().hover(|el| el.bg(ui::bg_overlay()))
        })
        .on_click(move |_, window, cx| {
            let picker = locations_state.read(cx);
            if !local || !picker.is_live(cx) || !picker.requests.accepts_callback(request) {
                return;
            }
            IconTooltips::reset(&locations_tips, window, cx);
            let mut targets = vec![(
                t("projectOnboarding", "localHost").to_string(),
                DirectoryLocation {
                    source: DirectorySource::Local,
                    path: "~".into(),
                },
            )];
            targets.extend(places.clone());
            let entries = targets
                .into_iter()
                .map(|(label, location)| {
                    let state = locations_state.clone();
                    crate::menu::item(label, move |window, cx| {
                        navigate(&state, request, location.clone(), window, cx);
                    })
                })
                .collect();
            crate::menu::show(window.mouse_position(), entries, window, cx);
        })
        .child(
            div()
                .flex_none()
                .text_color(ui::text_muted())
                .child(t("projectOnboarding", "hostLabel")),
        )
        .child(div().min_w(px(0.0)).truncate().child(host_label.clone()))
        .when(local, |el| {
            el.child(
                VectorIcon::new(CHEVRON_ICON, px(12.0))
                    .rotation(0.25)
                    .ink(ui::text_muted()),
            )
        })
        .tooltip(move |_, cx| cx.new(|_| Tooltip::new(host_label.clone())).into());
    let toolbar = div()
        .id("directory-tools")
        .flex_none()
        .min_w(px(0.0))
        .px(px(16.0))
        .pb(px(10.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .h(px(42.0))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(ui::font_px(15.0))
                        .child(t("projectOnboarding", "picker.title")),
                )
                .child(close_button),
        )
        .child(locations)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(home_button)
                .child(up_button)
                .child(path_segments),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(Input::new(&input).disabled(!live)),
                )
                .child(go_button),
        );
    let toolbar = IconTooltips::group(&tooltips, toolbar, window, cx);

    let mut list = div()
        .id("directory-list")
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .overflow_y_scroll()
        .track_scroll(&scroll)
        .scrollbar_width(px(16.0))
        .flex()
        .flex_col();
    if let Some(status) = status {
        list = list.child(
            div()
                .p(px(16.0))
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .text_color(if has_error {
                    ui::color_error()
                } else {
                    ui::text_muted()
                })
                .when(loading && live, |el| {
                    el.child(ui::spinner(px(16.0), ui::text_muted()))
                })
                .children(status.lines().map(|line| {
                    div()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .child(line.to_string())
                })),
        );
    } else if entries.is_empty() {
        list = list.child(div().p(px(16.0)).text_color(ui::text_muted()).child(t(
            "projectOnboarding",
            if unfiltered_empty {
                "picker.empty"
            } else {
                "picker.noMatches"
            },
        )));
    } else {
        for (index, entry) in entries.into_iter().enumerate() {
            let row_state = state.clone();
            let target = entry.location.clone();
            let inspect_path = entry.location.path.clone();
            let is_selected = selected.as_ref() == Some(&entry.location);
            list = list.child(
                div()
                    .id(SharedString::from(format!(
                        "directory-row-{request}-{index}"
                    )))
                    .h(px(34.0))
                    .flex_none()
                    .min_w(px(0.0))
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when(is_selected, |el| el.bg(ui::accent_subtle()))
                    .cursor_pointer()
                    .hover(|el| el.bg(ui::bg_overlay()))
                    .tooltip(move |_, cx| cx.new(|_| Tooltip::new(inspect_path.clone())).into())
                    .on_click(move |event: &ClickEvent, window, cx| {
                        row_state.update(cx, |state, cx| {
                            if !state.is_live(cx)
                                || state.loading
                                || !state.requests.accepts_callback(request)
                            {
                                return;
                            }
                            if event.click_count() >= 2 {
                                state.load(target.clone(), window, cx);
                            } else {
                                state.selected = Some(target.clone());
                                cx.notify();
                                window.refresh();
                            }
                        });
                    })
                    .child(FileIcon::new(&entry.name, true, false).size(px(17.0)))
                    .child(div().flex_1().min_w(px(0.0)).truncate().child(entry.name))
                    .when(entry.is_symlink, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .text_size(ui::font_px(10.0))
                                .text_color(ui::text_muted())
                                .child(t("projectOnboarding", "picker.link")),
                        )
                    })
                    .child(VectorIcon::new(CHEVRON_ICON, px(12.0)).ink(ui::text_muted())),
            );
        }
    }
    let cancel_state = state.clone();
    let select_state = state.clone();
    let selected_path = selected.map(|location| location.path).unwrap_or_default();
    let height = ui::clamp_dialog_body_height(px(560.0), window.viewport_size(), 0.80, px(0.0));
    div()
        .h(height)
        .flex_none()
        .w_full()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(ui::bg_elevated())
        .text_color(ui::text_primary())
        .text_size(ui::font_px(12.0))
        .child(toolbar)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h(px(0.0))
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .overflow_hidden()
                .border_t_1()
                .border_b_1()
                .border_color(ui::border_subtle())
                .bg(ui::bg_surface())
                .child(list)
                .child(
                    div()
                        .absolute()
                        .top(px(0.0))
                        .right(px(0.0))
                        .bottom(px(0.0))
                        .w(px(16.0))
                        .child(Scrollbar::vertical(&scroll).id("directory-scrollbar")),
                ),
        )
        .child(
            div()
                .px(px(16.0))
                .py(px(10.0))
                .flex_none()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(
                    div()
                        .id("directory-selected-path")
                        .h(px(24.0))
                        .flex_none()
                        .w_full()
                        .overflow_hidden()
                        .overflow_x_scroll()
                        .whitespace_nowrap()
                        .text_color(ui::text_secondary())
                        .child(selected_path),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.0))
                        .child(
                            ui::ghost_button("directory-cancel", t("projectOnboarding", "cancel"))
                                .on_click(move |_, window, cx| {
                                    finish(&cancel_state, None, window, cx)
                                }),
                        )
                        .child(
                            ui::primary_button(
                                "directory-select",
                                t("projectOnboarding", "picker.select"),
                            )
                            .opacity(if can_select { 1.0 } else { 0.4 })
                            .on_click(move |_, window, cx| {
                                if let Some(selection) = &selection {
                                    finish(&select_state, Some(selection), window, cx);
                                }
                            }),
                        ),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(path: &str) -> DirectoryLocation {
        DirectoryLocation {
            source: DirectorySource::Ssh {
                connection_id: "host-a".into(),
                connection_fingerprint: 31,
                connection_epoch: 7,
            },
            path: path.into(),
        }
    }

    fn listing() -> DirectoryListing {
        DirectoryListing {
            location: remote("/home/User"),
            directories: [".config", ".git", "Workspace", "work2", "work10"]
                .into_iter()
                .map(|name| DirectoryEntry {
                    name: name.into(),
                    location: remote(&format!("/home/User/{name}")),
                    is_symlink: false,
                })
                .collect(),
        }
    }

    #[test]
    fn newer_navigation_and_return_to_the_same_path_reject_old_listings() {
        let mut requests = DirectoryRequests::default();
        let first = requests.begin(remote("/a")).unwrap();
        let second = requests.begin(remote("/b")).unwrap();
        let third = requests.begin(remote("/a")).unwrap();
        assert!(!requests.owns(&first));
        assert!(!requests.owns(&second));
        assert!(requests.owns(&third));
        assert!(third.id > second.id && second.id > first.id);
    }

    #[test]
    fn a_remote_location_never_falls_back_to_the_client_filesystem() {
        let error = browse_directory(&ProjectHostSelection::Local, None, &remote("/")).unwrap_err();
        assert_eq!(error.kind, DirectoryBrowseErrorKind::Unavailable);
        let error = browse_local_directory(&remote("~")).unwrap_err();
        assert_eq!(error.kind, DirectoryBrowseErrorKind::Unavailable);
    }

    #[test]
    fn requests_reject_every_source_identity_mismatch() {
        let mut requests = DirectoryRequests::default();
        let request = requests.begin(remote("/same")).unwrap();
        for source in [
            DirectorySource::Local,
            DirectorySource::Wsl {
                distro: "Ubuntu".into(),
            },
            DirectorySource::Ssh {
                connection_id: "host-b".into(),
                connection_fingerprint: 31,
                connection_epoch: 7,
            },
            DirectorySource::Ssh {
                connection_id: "host-a".into(),
                connection_fingerprint: 32,
                connection_epoch: 7,
            },
            DirectorySource::Ssh {
                connection_id: "host-a".into(),
                connection_fingerprint: 31,
                connection_epoch: 8,
            },
        ] {
            let mut other = request.clone();
            other.location.source = source;
            assert!(!requests.owns(&other));
        }
        let mut other = request.clone();
        other.location.path = "/elsewhere".into();
        assert!(!requests.owns(&other));
        assert!(requests.owns(&request));
    }

    #[test]
    fn listing_results_require_the_captured_fingerprint_and_epoch() {
        let source = remote("/").source;
        let mut result = crate::remote_ssh::RemoteDirectoryListing {
            canonical_path: "/".into(),
            directories: Vec::new(),
            connection_epoch: 7,
            connection_fingerprint: 31,
        };
        assert!(listing_authority_matches(&source, &result));
        result.connection_epoch = 8;
        assert!(!listing_authority_matches(&source, &result));
        result.connection_epoch = 7;
        result.connection_fingerprint = 32;
        assert!(!listing_authority_matches(&source, &result));
        assert!(!listing_authority_matches(&DirectorySource::Local, &result));
    }

    #[test]
    fn cancellation_and_overflow_are_terminal_for_the_picker_instance() {
        let mut requests = DirectoryRequests::default();
        let request = requests.begin(remote("/a")).unwrap();
        requests.close();
        assert!(!requests.owns(&request));
        assert!(requests.begin(remote("/b")).is_none());

        let mut requests = DirectoryRequests {
            next_id: u64::MAX - 1,
            ..DirectoryRequests::default()
        };
        let last = requests.begin(remote("/a")).unwrap();
        assert!(requests.owns(&last));
        assert!(requests.begin(remote("/b")).is_none());
        assert!(requests.exhausted);
        assert!(!requests.owns(&last));
        assert!(requests.begin(remote("/a")).is_none());
    }

    #[test]
    fn typing_filters_only_the_loaded_names_including_hidden_directories() {
        let listing = listing();
        assert_eq!(filter_directories(&listing, "").len(), 5);
        assert_eq!(filter_directories(&listing, "/home/User").len(), 5);
        let names = filter_directories(&listing, "WORK")
            .into_iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Workspace", "work2", "work10"]);
        assert_eq!(filter_directories(&listing, ".git")[0].name, ".git");
        assert!(filter_directories(&listing, "/not-loaded").is_empty());
        assert_eq!(listing.location.path, "/home/User");
        assert_eq!(
            resolve_input(&listing.location, "Workspace").unwrap().path,
            "/home/User/Workspace"
        );
    }

    #[test]
    fn select_accepts_only_a_ready_current_directory_or_its_captured_child() {
        let listing = listing();
        let child = &listing.directories[0].location;
        assert_eq!(
            selected_host_path(Some(&listing), Some(child), true),
            Some("/home/User/.config".into())
        );
        assert_eq!(
            selected_host_path(Some(&listing), Some(&listing.location), true),
            Some("/home/User".into())
        );
        assert!(selected_host_path(Some(&listing), Some(child), false).is_none());
        assert!(selected_host_path(None, Some(child), true).is_none());
        assert!(selected_host_path(Some(&listing), Some(&remote("/other")), true).is_none());
        let mut changed = child.clone();
        changed.source = DirectorySource::Local;
        assert!(selected_host_path(Some(&listing), Some(&changed), true).is_none());
    }

    #[test]
    fn select_callback_retains_its_request_and_selection_across_navigation_aba() {
        let listing = listing();
        let mut requests = DirectoryRequests::default();
        let first = requests.begin(listing.location.clone()).unwrap();
        let child = &listing.directories[0].location;
        let selection =
            DirectorySelection::capture(&requests, Some(&listing), Some(child), true).unwrap();
        assert_eq!(
            selection.host_path(&requests, Some(&listing), Some(child), true),
            Some(child.path.clone())
        );
        assert!(
            selection
                .host_path(&requests, Some(&listing), Some(&listing.location), true)
                .is_none()
        );
        assert!(
            selection
                .host_path(&requests, Some(&listing), Some(child), false)
                .is_none()
        );
        assert!(
            selection
                .host_path(&requests, None, Some(child), true)
                .is_none()
        );

        requests.begin(remote("/other")).unwrap();
        let latest = requests.begin(listing.location.clone()).unwrap();
        assert!(!requests.accepts_callback(first.id));
        assert!(requests.accepts_callback(latest.id));
        assert!(
            selection
                .host_path(&requests, Some(&listing), Some(child), true)
                .is_none()
        );
        let successor =
            DirectorySelection::capture(&requests, Some(&listing), Some(child), true).unwrap();
        assert_eq!(
            successor.host_path(&requests, Some(&listing), Some(child), true),
            Some(child.path.clone())
        );

        let mut rebound = listing.clone();
        rebound.location.source = DirectorySource::Local;
        assert!(
            DirectorySelection::capture(&requests, Some(&rebound), Some(child), true).is_none()
        );
        assert!(requests.close());
        assert!(
            successor
                .host_path(&requests, Some(&listing), Some(child), true)
                .is_none()
        );
    }

    #[test]
    fn old_cancel_cannot_close_a_successor_even_when_request_numbers_match() {
        let mut closed = DirectoryRequests::default();
        let old = closed.begin(remote("/same")).unwrap();
        assert!(closed.close());
        let mut successor = DirectoryRequests::default();
        let current = successor.begin(remote("/same")).unwrap();
        assert_eq!(old, current);
        if closed.close() {
            successor.close();
        }
        assert!(successor.owns(&current));
        assert!(!closed.accepts_callback(old.id));
        assert!(!DirectoryRequests::default().accepts_callback(0));
    }

    #[test]
    fn remote_navigation_preserves_case_and_posix_filename_characters() {
        let location = remote(r"/home/User/a\b:folder");
        let crumbs = breadcrumbs(&location);
        assert_eq!(
            crumbs
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["/", "home", "User", r"a\b:folder"]
        );
        assert_eq!(crumbs.last().unwrap().1, location);
        assert_eq!(parent_location(&location).unwrap().path, "/home/User");
        assert!(parent_location(&remote("/")).is_none());
        assert_eq!(
            resolve_input(&location, "/Case/Sensitive").unwrap().path,
            "/Case/Sensitive"
        );
        let windows_spelling = resolve_input(&location, r"C:\client\folder").unwrap();
        assert_eq!(windows_spelling.source, location.source);
        assert_eq!(
            windows_spelling.path,
            r"/home/User/a\b:folder/C:\client\folder"
        );
        assert!(resolve_input(&location, "").is_err());
        assert!(resolve_input(&location, "/nul\0path").is_err());
    }

    #[test]
    fn wsl_navigation_keeps_posix_display_and_unc_registration() {
        let native = DirectoryLocation {
            source: DirectorySource::Local,
            path: "~".into(),
        };
        let location = resolve_input(&native, r"\\wsl$\Ubuntu\home\User\Project").unwrap();
        assert_eq!(
            location.source,
            DirectorySource::Wsl {
                distro: "Ubuntu".into()
            }
        );
        assert_eq!(location.path, "/home/User/Project");
        assert_eq!(parent_location(&location).unwrap().path, "/home/User");
        assert_eq!(breadcrumbs(&location).last().unwrap().1, location);
        assert_eq!(
            location.host_path().unwrap(),
            r"\\wsl.localhost\Ubuntu\home\User\Project"
        );
        assert_eq!(
            resolve_input(&location, "/etc").unwrap().source,
            location.source
        );
        let other = resolve_input(&location, r"\\wsl.localhost\Debian\srv").unwrap();
        assert_ne!(other.source, location.source);
    }

    #[test]
    fn long_paths_keep_every_breadcrumb_target() {
        let path = format!("/home/{}/{}", "long".repeat(100), "deep/".repeat(80));
        let location = remote(path.trim_end_matches('/'));
        let crumbs = breadcrumbs(&location);
        assert_eq!(crumbs.len(), 83);
        assert_eq!(crumbs.last().unwrap().1, location);
    }

    #[test]
    fn browser_dialog_reserves_nested_offset_animation_and_border_on_short_viewports() {
        for height in [400.0, 480.0, 600.0, 800.0, 1080.0] {
            let viewport = gpui::size(px(320.0), px(height));
            let body = ui::clamp_dialog_body_height(px(560.0), viewport, 0.80, px(0.0));
            assert!(picker_top_margin(viewport.height) + px(48.0) + body <= viewport.height);
            assert!(body >= px(320.0));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_drives_and_unc_roots_remain_native_and_do_not_escape_their_root() {
        for path in [r"C:\", r"\\server\share\"] {
            let root = DirectoryLocation {
                source: DirectorySource::Local,
                path: path.into(),
            };
            assert!(parent_location(&root).is_none());
            let crumbs = breadcrumbs(&root);
            assert_eq!(crumbs.len(), 1);
            assert_eq!(crumbs[0].1.source, DirectorySource::Local);
            assert!(Path::new(&crumbs[0].1.path).is_absolute());
        }
        let location = DirectoryLocation {
            source: DirectorySource::Local,
            path: r"C:\Users\Me".into(),
        };
        assert_eq!(parent_location(&location).unwrap().path, r"C:\Users");
        assert_eq!(
            resolve_input(&location, r"D:\repo").unwrap().path,
            r"D:\repo"
        );
        assert!(resolve_input(&location, "D:relative").is_err());
        let share = resolve_input(&location, r"\\server\share\repo").unwrap();
        assert_eq!(share.source, DirectorySource::Local);
        assert_eq!(share.host_path().unwrap(), r"\\server\share\repo");
    }
}
