//! The unified macOS title bar the standalone app draws itself, which is
//! kiln-shell's [`kiln_shell::TitleBar`], with the workbench's items in its
//! slots (the design's title bar, `docs/design/design-system.md`):
//!
//! - **Left**, after the traffic lights: back and forward (`nav.back`,
//!   `nav.forward`), each off while the navigation history has nowhere to
//!   go; then the project widget, the root folder's name and a chevron,
//!   which opens Open Recent (`workbench.open_recent`); and beside it, no
//!   slash between them, the branch widget, HEAD's branch (its short id
//!   when detached), its marks and ahead/behind counts (`git_branch`) and a
//!   chevron, which opens the branch picker (`git.switch_branch`), shown
//!   only in a repository; its tooltip names the branch's upstream.
//! - **Right**: search, which runs `quick_open.toggle` as its key does
//!   (Go to File, and the palette by its prefixes), and a settings gear
//!   (`workbench.open_settings_ui`).
//!
//! Each is a ghost button, filled while hovered, with an accessible name
//! and a tooltip naming it and its key (as the keymap has it when the
//! tooltip shows). A click runs its command as from the mouse, as the
//! pane toolbar's buttons do, and leaves focus where it was.

use std::{path::Path, rc::Rc};

use fe_command::Source;
use fe_ui::{Button, Icon, IconButton, Tooltip};
use gpui::{
    AnyElement, App, AppContext as _, Empty, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity,
    Window, div, px,
};
use kiln_l10n::{Text, tr};
use kiln_shell::{TitleBarItems, TitleBarSlot};

use crate::{
    Workbench,
    pane_toolbar::{self, NavButtons},
};

// The unified title bar's metrics and window options are kiln's.
pub use kiln_platform::window_options::{
    TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, TRAFFIC_LIGHTS, titlebar_options,
};

/// What the bar reads with no root folder.
const NO_ROOT: &str = fe_config::APP_NAME;

/// The gap between back and forward, and between the two widgets.
const GAP: gpui::Pixels = px(2.);

/// The gap between forward and the project widget.
const NAV_GAP: gpui::Pixels = px(6.);

/// The widest the project and branch widgets get: a longer name ends in an
/// ellipsis.
const PROJECT_MAX_WIDTH: gpui::Pixels = px(240.);
const BRANCH_MAX_WIDTH: gpui::Pixels = px(200.);

/// What the bar shows, from the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) nav: NavButtons,
    /// The root folder's name: see [`name`].
    pub(crate) project: SharedString,
    /// HEAD's branch, or its short id when detached; none outside a
    /// repository.
    pub(crate) branch: Option<SharedString>,
    /// The branch's upstream, as `origin/main`, if it has one.
    pub(crate) upstream: Option<SharedString>,
}

/// What a mouse down on the bar asked the window to do, under test, where
/// the platform's window move is unimplemented.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Move,
    DoubleClick,
}

#[cfg(test)]
thread_local! {
    pub(crate) static REQUESTS: std::cell::RefCell<Vec<Request>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The text of the last tooltip one of the bar's buttons built.
    pub(crate) static LAST_TOOLTIP: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
    /// Each button the bar last drew, left to right: its id and its
    /// accessible name.
    pub(crate) static DRAWN: std::cell::RefCell<Vec<(&'static str, SharedString)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Records the title bar's requests of the window in [`REQUESTS`].
#[cfg(test)]
pub(crate) fn record_requests(cx: &mut gpui::App) {
    use kiln_shell::window_frame::{WindowRequest, intercept_window_requests};
    intercept_window_requests(cx, |request| {
        let request = match request {
            WindowRequest::Move => Request::Move,
            WindowRequest::TitlebarDoubleClick => Request::DoubleClick,
            _ => return,
        };
        REQUESTS.with_borrow_mut(|requests| requests.push(request));
    });
}

/// The root folder's name, or Tenmoku with no root.
pub(crate) fn name(root: Option<&Path>) -> SharedString {
    root.and_then(Path::file_name).map_or_else(
        || SharedString::from(NO_ROOT),
        |name| SharedString::from(name.to_string_lossy().into_owned()),
    )
}

/// The bar's items for `workbench`, which read its state each time the bar
/// draws.
pub(crate) fn items(workbench: WeakEntity<Workbench>) -> TitleBarItems {
    let state = move |cx: &App| {
        let workbench = workbench.upgrade()?;
        Some(workbench.read(cx).title_bar_state())
    };
    let state = Rc::new(state);
    let left: TitleBarSlot = Rc::new({
        let state = state.clone();
        move |_, cx| match state(cx) {
            Some(state) => left(&state, cx),
            None => Empty.into_any_element(),
        }
    });
    let right: TitleBarSlot = Rc::new(move |_, cx| match state(cx) {
        Some(_) => right(cx),
        None => Empty.into_any_element(),
    });
    TitleBarItems {
        left: Some(left),
        right: Some(right),
    }
}

/// Back, forward, the project widget and the branch widget.
fn left(state: &State, cx: &App) -> AnyElement {
    #[cfg(test)]
    DRAWN.with_borrow_mut(Vec::clear);
    let nav = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(GAP)
        .child(icon_button(
            "title-back",
            Icon::ArrowLeft,
            tr!("workbench-chrome-back"),
            "nav.back",
            !state.nav.back,
            cx,
        ))
        .child(icon_button(
            "title-forward",
            Icon::ArrowRight,
            tr!("workbench-chrome-forward"),
            "nav.forward",
            !state.nav.forward,
            cx,
        ));
    let project = widget(
        "title-project",
        PROJECT_MAX_WIDTH,
        state.project.clone(),
        tr!("workbench-chrome-project", name = &state.project),
        tr!("workbench-chrome-open-recent"),
        "workbench.open_recent",
    );
    let branch = state.branch.clone().map(|branch| {
        widget(
            "title-branch",
            BRANCH_MAX_WIDTH,
            branch.clone(),
            tr!("workbench-chrome-branch", name = &branch),
            match &state.upstream {
                Some(upstream) => tr!("workbench-git-sync-switch-branch", upstream = upstream),
                None => tr!("workbench-chrome-switch-branch"),
            },
            "git.switch_branch",
        )
    });
    div()
        .flex()
        .min_w_0()
        .items_center()
        .gap(NAV_GAP)
        .child(nav)
        .child(
            div()
                .flex()
                .min_w_0()
                .items_center()
                .gap(GAP)
                .child(project)
                .children(branch),
        )
        .into_any_element()
}

/// Search and the settings gear.
fn right(cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(GAP)
        .child(icon_button(
            "title-search",
            Icon::Search,
            tr!("workbench-chrome-go-to-file"),
            "quick_open.toggle",
            false,
            cx,
        ))
        .child(icon_button(
            "title-settings",
            Icon::Settings,
            tr!("workbench-chrome-settings"),
            "workbench.open_settings_ui",
            false,
            cx,
        ))
        .into_any_element()
}

/// An icon button named `name` that runs `command`, its key in its tooltip.
fn icon_button(
    id: &'static str,
    icon: Icon,
    name: SharedString,
    command: &'static str,
    disabled: bool,
    cx: &App,
) -> IconButton {
    #[cfg(test)]
    DRAWN.with_borrow_mut(|drawn| drawn.push((id, name.clone())));
    let ui = fe_ui::ui_theme(cx);
    let button = IconButton::new(id, icon, name.clone(), &ui)
        .disabled(disabled)
        .tooltip(tooltip(name, command))
        .on_click(move |_, window, cx| run(command, window, cx));
    match pane_toolbar::shortcut(command, cx) {
        Some(key) => button.shortcut(key),
        None => button,
    }
}

/// A labelled ghost button with a chevron after its `label`, named `name`,
/// that runs `command`, its `title` and key in its tooltip.
fn widget(
    id: &'static str,
    max_width: gpui::Pixels,
    label: SharedString,
    name: SharedString,
    title: SharedString,
    command: &'static str,
) -> Button {
    #[cfg(test)]
    DRAWN.with_borrow_mut(|drawn| drawn.push((id, name.clone())));
    Button::new(id, Text::Verbatim(label))
        .ghost()
        .icon(Icon::ChevronDown)
        .name(name)
        // The chevron after the label.
        .flex_row_reverse()
        // Up to `max_width`, then in a narrow bar both widgets give way,
        // their labels ending in an ellipsis, their chevrons kept.
        .flex_shrink(1.)
        .min_w_0()
        .max_w(max_width)
        .debug_selector(move || id.into())
        .tooltip(tooltip(title, command))
        .on_click(move |_, window, cx| run(command, window, cx))
}

/// A tooltip of `title` and `command`'s key, as the keymap has it when it
/// shows.
fn tooltip(
    title: SharedString,
    command: &'static str,
) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
    move |_, cx| {
        let shortcut = pane_toolbar::shortcut(command, cx);
        let tooltip = Tooltip::new(title.clone(), shortcut, &fe_ui::ui_theme(cx));
        #[cfg(test)]
        LAST_TOOLTIP.with_borrow_mut(|last| *last = Some(tooltip.text()));
        cx.new(|_| tooltip).into()
    }
}

/// Runs `command` from the mouse, as a key would.
fn run(command: &'static str, window: &mut Window, cx: &mut App) {
    pane_toolbar::dispatch(command, Source::Mouse, window, cx);
}
