//! The project tree's right-click menu on the `src` folder, as a right
//! click on its row opens it: New File, New Folder | Rename, Duplicate,
//! Move to Trash | Cut, Copy | Copy Path, Copy Relative Path | Collapse
//! All Folders, each with its key, Rename highlighted from the keyboard.

use gpui::{
    Focusable as _, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, point, px,
};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "project_tree_menu",
    steps: |_| {
        vec![
            pointer(SRC_ROW),
            right_click(SRC_ROW),
            idle(),
            key("down"),
            key("down"),
            key("down"),
        ]
    },
    probes: PROBES,
    ..DEFAULT
};

/// Within the `src` row, the root's first child, at the scene's size.
const SRC_ROW: Point<Pixels> = point(px(200.), px(108.));

const PROBES: &[Probe] = &[
    // The menu, right of its items' titles, over the editor.
    probe(330., 125., "surface.panel"),
    // Rename…, highlighted, right of its title.
    probe(330., 182., "list.row_selected"),
];

/// Moves the pointer to `at`, so the press there finds what's under it.
fn pointer(at: Point<Pixels>) -> Step {
    dispatch(move || {
        vec![PlatformInput::MouseMove(MouseMoveEvent {
            position: at,
            pressed_button: None,
            modifiers: Modifiers::none(),
        })]
    })
}

/// A right click at `at`: the press opens the menu.
fn right_click(at: Point<Pixels>) -> Step {
    dispatch(move || {
        vec![
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Right,
                position: at,
                modifiers: Modifiers::none(),
                click_count: 1,
                first_mouse: false,
            }),
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Right,
                position: at,
                modifiers: Modifiers::none(),
                click_count: 1,
            }),
        ]
    })
}

/// Dispatches `events` to the window as the platform would, once the
/// step's update of the workbench is over (its listeners may update it).
fn dispatch(events: impl FnOnce() -> Vec<PlatformInput> + 'static) -> Step {
    Box::new(move |_, window, cx| {
        window.defer(cx, move |window, cx| {
            for event in events() {
                window.dispatch_event(event, cx);
            }
        });
    })
}

/// Presses `keys` in the focused element, failing the scene unless that's
/// the project tree, as a right click on its row leaves it.
fn key(keys: &'static str) -> Step {
    Box::new(move |workbench, window, cx| {
        if !workbench.panel().focus_handle(cx).is_focused(window) {
            return fail("the right click didn't focus the project tree".to_owned());
        }
        let keystroke = Keystroke::parse(keys).expect("a keystroke");
        window.defer(cx, move |window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        });
    })
}
