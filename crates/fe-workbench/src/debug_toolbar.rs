//! The floating debug toolbar (docs/decisions/r1b9dw3g0g-debugger.md §5;
//! `docs/design/design-system.md`, Debugger, "Without a title bar"; card
//! D8).
//!
//! - **Where.** While a session exists (starting, or not yet ended) and the
//!   window has **no title bar** (Linux, and macOS without the unified
//!   title bar), the session's controls float centred over the editor
//!   area, 4 px below its tab strip, over the editors. With a title bar
//!   they're the title bar's run widget's (D0; D8b builds it), and the
//!   toolbar never shows: the test is whether there's a title bar, not the
//!   platform.
//! - **Look.** The floating surface (`surface.raised`, the design's
//!   overlay surface) with the floating shadow and a hairline,
//!   6 px radius, 30 px tall: the configuration's label, then 22 px icon
//!   buttons 2 px apart: Continue or Pause in one place (both while the
//!   session is uncertain), Step Over, Step Into, Step Out, 6 px of space,
//!   Restart and Stop.
//! - **Under modals.** While a picker or prompt shows, it isn't drawn: they
//!   open over the panes' top, where it floats, and it would cover their
//!   input or first rows.
//! - **No logic of its own.** Each button dispatches its `debug.*` command
//!   (`debug_session::commands`), as its key would, and the label
//!   `debug.show_session`. Which are enabled is D2b's control-state query
//!   ([`Debugger::controls`]): no stepping while running, resuming or
//!   uncertain, and nothing but Stop while resuming or starting.
//! - **Names.** Each button is named for what it does (Continue, Pause,
//!   Step Over, Step Into, Step Out, Restart, Stop Debugging), its tooltip
//!   adding its key; the label names the session and its state.

use fe_command::{CommandId, ContextFrame, ContextStack, Source, Value};
use fe_dap::{Control, Controls, Disabled, SessionState};
use fe_ui::{ActiveTheme as _, Button, Icon, IconButton, height, radius};
use gpui::{
    AnyElement, App, Bounds, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, SharedString, Styled as _, Window, div, point, px,
};
use kiln_l10n::{Text, tr};

use crate::{
    KEY_CONTEXT, Workbench,
    debug_session::{DEBUG_RUNNING, DEBUG_SESSION, DEBUG_STOPPED, Debugger},
    workbench::git_commands::git_log::float_shadow,
};

/// How far below the top of the editor area (its tab strip's bottom) the
/// toolbar floats.
const TOP: Pixels = px(4.);
/// Between the buttons.
const GAP: Pixels = px(2.);
/// Before Restart.
const GROUP_GAP: Pixels = px(6.);
/// Inside the bar, all round.
const PADDING: Pixels = px(4.);

/// The toolbar's element id and debug selector.
pub(crate) const TOOLBAR_ID: &str = "debug-toolbar";
/// Its session label's.
const SESSION_LABEL_ID: &str = "debug-toolbar-session";

/// What the last frame's toolbar drew, for tests: `None` if it drew none.
#[cfg(test)]
#[derive(Clone, Debug, Default)]
pub(crate) struct Drawn {
    /// The label's accessible name.
    pub label: SharedString,
    /// Each button: its id, name, whether it's enabled and its key.
    pub buttons: Vec<(&'static str, SharedString, bool, Option<String>)>,
}

#[cfg(test)]
thread_local! {
    pub(crate) static DRAWN: std::cell::RefCell<Option<Drawn>> =
        const { std::cell::RefCell::new(None) };
}

/// A button of the toolbar, and of the title bar's run widget during a
/// session (`run_widget_debug.rs`): its id, icon, name, command and
/// control.
pub(crate) struct Spec {
    pub(crate) id: &'static str,
    pub(crate) icon: Icon,
    pub(crate) name: SharedString,
    pub(crate) command: &'static str,
    pub(crate) control: Control,
    /// The command whose key its tooltip shows: its own, or for Continue
    /// `debug.start`'s, F5, which continues while stopped.
    pub(crate) key: &'static str,
    /// The key context flag that key is bound under.
    pub(crate) flag: &'static str,
}

/// Which of Continue and Pause show: by the session's state, a resume
/// asked for and not yet taken reading as Resuming, as the control-state
/// query has it.
fn continue_and_pause(debugger: &Debugger, controls: &Controls) -> (bool, bool) {
    let resuming = controls.continue_.reason() == Some(Disabled::Resuming);
    match debugger.state() {
        _ if debugger.starting().is_some() => (false, true),
        Some(SessionState::Stopped) if !resuming => (true, false),
        Some(SessionState::Uncertain) if !resuming => (true, true),
        _ => (false, true),
    }
}

/// The buttons, in order, and where the gap goes (before the index).
pub(crate) fn specs(debugger: &Debugger) -> (Vec<Spec>, usize) {
    let controls = debugger.controls();
    let (show_continue, show_pause) = continue_and_pause(debugger, &controls);
    let mut specs = Vec::new();
    if show_continue {
        specs.push(Spec {
            id: "debug-toolbar-continue",
            icon: Icon::Play,
            name: tr!("workbench-debug-toolbar-continue"),
            command: "debug.continue",
            control: controls.continue_,
            key: "debug.start",
            flag: DEBUG_STOPPED,
        });
    }
    if show_pause {
        specs.push(Spec {
            id: "debug-toolbar-pause",
            icon: Icon::Pause,
            name: tr!("workbench-debug-toolbar-pause"),
            command: "debug.pause",
            control: controls.pause,
            key: "debug.pause",
            flag: DEBUG_RUNNING,
        });
    }
    let step = |id, icon, name, command| Spec {
        id,
        icon,
        name,
        command,
        control: controls.step,
        key: command,
        flag: DEBUG_STOPPED,
    };
    specs.push(step(
        "debug-toolbar-step-over",
        Icon::RedoDot,
        tr!("workbench-debug-toolbar-step-over"),
        "debug.step_over",
    ));
    specs.push(step(
        "debug-toolbar-step-into",
        Icon::ArrowDownToDot,
        tr!("workbench-debug-toolbar-step-into"),
        "debug.step_into",
    ));
    specs.push(step(
        "debug-toolbar-step-out",
        Icon::ArrowUpFromDot,
        tr!("workbench-debug-toolbar-step-out"),
        "debug.step_out",
    ));
    let gap = specs.len();
    specs.push(Spec {
        id: "debug-toolbar-restart",
        icon: Icon::RotateCcw,
        name: tr!("workbench-debug-toolbar-restart"),
        command: "debug.restart",
        control: controls.restart,
        key: "debug.restart",
        flag: DEBUG_SESSION,
    });
    specs.push(Spec {
        id: "debug-toolbar-stop",
        icon: Icon::Square,
        name: tr!("workbench-debug-toolbar-stop"),
        command: "debug.stop",
        control: controls.stop,
        key: "debug.stop",
        flag: DEBUG_SESSION,
    });
    (specs, gap)
}

/// The first single key that runs `command` in the workbench with the
/// debugger's `flag` set, as this platform prints it: read from the
/// keymap now, so it follows reloads.
pub(crate) fn key(command: &str, flag: &str, cx: &App) -> Option<String> {
    let command = CommandId::parse(command).ok()?;
    let frame = ContextFrame::new(KEY_CONTEXT)
        .flag(DEBUG_SESSION)
        .flag(flag);
    let stack = ContextStack::new().with(frame);
    let keymap = fe_gpui::current_keymap(cx);
    fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack)
        .iter()
        .find(|keys| keys.keystrokes().len() == 1)
        .map(fe_keymap::shortcut_label)
}

/// The label's accessible name: the session, and its state.
pub(crate) fn label_name(debugger: &Debugger, name: &str) -> SharedString {
    if debugger.starting().is_some() {
        return tr!("workbench-debug-toolbar-session-starting", name = name);
    }
    match (debugger.state(), debugger.stop_location()) {
        (Some(SessionState::Stopped), Some(stop)) => tr!(
            "workbench-debug-toolbar-session-stopped-at",
            name = name,
            line = stop.row + 1
        ),
        (Some(SessionState::Stopped), None) => {
            tr!("workbench-debug-toolbar-session-stopped", name = name)
        }
        (
            Some(SessionState::Launching | SessionState::Initializing | SessionState::Configuring),
            _,
        ) => tr!("workbench-debug-toolbar-session-starting", name = name),
        _ => tr!("workbench-debug-toolbar-session-running", name = name),
    }
}

/// The toolbar's bar: the label and the buttons.
fn bar(debugger: &Debugger, cx: &App) -> AnyElement {
    let colors = cx.theme().colors;
    let name = (debugger.starting())
        .or_else(|| debugger.configuration())
        .unwrap_or_default()
        .to_owned();
    let label = Button::new(SESSION_LABEL_ID, Text::Verbatim(name.clone().into()))
        .ghost()
        .small()
        .icon(Icon::Bug)
        .name(label_name(debugger, &name))
        .flex_shrink(1.)
        .min_w_0()
        .max_w(fe_ui::width::TITLE_LABEL_MAX)
        .debug_selector(|| SESSION_LABEL_ID.into())
        .on_click(|_, window, cx| {
            crate::pane_toolbar::dispatch("debug.show_session", Source::Mouse, window, cx)
        });
    let (specs, gap) = specs(debugger);
    #[cfg(test)]
    let mut drawn = Drawn {
        label: label_name(debugger, &name),
        buttons: Vec::new(),
    };
    let buttons = specs.into_iter().enumerate().map(|(ix, spec)| {
        let command = spec.command;
        let enabled = spec.control.is_enabled();
        let shortcut = key(spec.key, spec.flag, cx);
        #[cfg(test)]
        (drawn.buttons).push((spec.id, spec.name.clone(), enabled, shortcut.clone()));
        let mut button = IconButton::new(spec.id, spec.icon, spec.name, &fe_ui::ui_theme(cx))
            .disabled(!enabled)
            .on_click(move |_, window, cx| {
                crate::pane_toolbar::dispatch(command, Source::Mouse, window, cx)
            });
        if let Some(key) = shortcut {
            button = button.shortcut(key);
        }
        let space = if ix == gap { GROUP_GAP - GAP } else { px(0.) };
        div().flex_none().ml(space).child(button)
    });
    let bar = div()
        .id(TOOLBAR_ID)
        .debug_selector(|| TOOLBAR_ID.into())
        .flex()
        .items_center()
        .gap(GAP)
        .h(height::TOOLBAR)
        .p(PADDING)
        .rounded(radius::MENU)
        .border_1()
        .border_color(colors.border.default)
        .bg(colors.surface.raised)
        .shadow(vec![float_shadow(colors.surface.raised)])
        // Clicks on it stay on it, not on the editor under it.
        .occlude()
        .child(label)
        .children(buttons.collect::<Vec<_>>());
    #[cfg(test)]
    DRAWN.with_borrow_mut(|last| *last = Some(drawn));
    bar.into_any_element()
}

/// The toolbar floating over the panes, if it shows: see the
/// [module](self).
pub(crate) fn floating(workbench: &Workbench, cx: &mut Context<Workbench>) -> Option<AnyElement> {
    #[cfg(test)]
    DRAWN.with_borrow_mut(|last| *last = None);
    if workbench.own_title_bar {
        return None;
    }
    // A picker or prompt is over the panes where the bar floats: the bar
    // gives way to it, rather than cover its input or rows.
    if workbench.shell().read(cx).modal(cx).is_some() {
        return None;
    }
    let debugger = workbench.debugger(cx)?;
    let debugger = debugger.read(cx);
    if !debugger.is_active() {
        return None;
    }
    let child = bar(debugger, cx);
    let this = cx.weak_entity();
    let area = Box::new(move |cx: &App| {
        this.read_with(cx, |workbench, cx| workbench.editor_area(cx))
            .ok()
            .flatten()
    });
    Some(Floating { child, area }.into_any_element())
}

/// Reads the editor area as this frame laid it out.
type AreaFn = Box<dyn Fn(&App) -> Option<Bounds<Pixels>>>;

/// Draws its child centred at the top of the editor area, [`TOP`] in: the
/// area read in prepaint, after the panes recorded theirs, so a dock
/// opening or resizing moves it in the same frame. With no area, nothing.
struct Floating {
    child: AnyElement,
    area: AreaFn,
}

impl gpui::Element for Floating {
    type RequestLayoutState = ();
    type PrepaintState = bool;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        // Nothing of its own: the child is laid out in prepaint, where the
        // area is known.
        let style = gpui::Style {
            position: gpui::Position::Absolute,
            ..gpui::Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Some(area) = (self.area)(cx) else {
            return false;
        };
        let size = self.child.layout_as_root(
            gpui::size(
                gpui::AvailableSpace::Definite(area.size.width),
                gpui::AvailableSpace::MinContent,
            ),
            window,
            cx,
        );
        let left = (area.center().x - size.width / 2.).max(area.left());
        self.child
            .prepaint_at(point(left, area.top() + TOP), window, cx);
        true
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        placed: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *placed {
            self.child.paint(window, cx);
        }
    }
}

impl IntoElement for Floating {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}
