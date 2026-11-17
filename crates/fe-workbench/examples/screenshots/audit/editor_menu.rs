//! The editor's right-click menu in `src/main.rs`, opened by a right click
//! in the text as the platform delivers it:
//!
//! - `editor_menu_starting`: on `HashMap` while rust-analyzer hasn't
//!   started (the demo's folder isn't trusted), so the language server's
//!   items show disabled, then Cut, Copy, Paste, Copy Path, Copy Reference
//!   and Toggle Breakpoint and Ask Agent About This Line.
//! - `editor_menu_context`: in a test in a Git repository, the last group
//!   whole: Run Test, Toggle Breakpoint, Annotate with Git Blame and Ask
//!   Agent About This Line, Run Test highlighted from the keyboard.

use std::{fs, path::Path};

use gpui::{
    Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle},
    token_probe::{Probe, probe},
};

pub(crate) const STARTING: Surface = Surface {
    name: "editor_menu_starting",
    steps: |_| vec![idle(), right_click("HashMap::new"), idle(), check(false)],
    probes: STARTING_PROBES,
    ..DEFAULT
};

pub(crate) const CONTEXT: Surface = Surface {
    name: "editor_menu_context",
    git: true,
    setup: |_, parent| add_test(parent),
    steps: |_| {
        vec![
            // The Git status read lands, which the blame item needs.
            idle(),
            idle(),
            right_click("assert!"),
            idle(),
            check(true),
            key("end"),
            key("up"),
            key("up"),
            key("up"),
        ]
    },
    probes: CONTEXT_PROBES,
    ..DEFAULT
};

/// A test appended to `src/main.rs`, which [`CONTEXT`] right-clicks in.
const TEST: &str = "\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn counts() {\n        \
                    assert!(true);\n    }\n}\n";

fn add_test(parent: &Path) {
    let main = parent.join("demo/src/main.rs");
    let text = fs::read_to_string(&main).expect("the demo's main.rs");
    fs::write(&main, format!("{text}{TEST}")).expect("main.rs written");
}

// The menu opens at the pointer, moved left and up to fit the window.
const STARTING_PROBES: &[Probe] = &[
    // The menu, between Go to Definition's title and its key, and between
    // Cut's and Copy's rows.
    probe(870., 246., "surface.panel"),
    probe(870., 432., "surface.panel"),
];

const CONTEXT_PROBES: &[Probe] = &[
    // The menu, right of Cut's title, between its row and Copy's.
    probe(620., 432., "surface.panel"),
    // Run Test, highlighted, right of its title.
    probe(620., 511., "list.row_selected"),
];

/// A right click in the active editor at the start of `text`'s first
/// match, as the platform delivers it: the pointer moves there, then a
/// right press and release.
fn right_click(text: &'static str) -> Step {
    Box::new(move |workbench, window, cx| {
        let Some(editor) = workbench.active_editor(cx).cloned() else {
            return fail("no active editor to right-click in".to_owned());
        };
        let Some(offset) = editor.read(cx).model().text().to_string().find(text) else {
            return fail(format!("no {text:?} to right-click on"));
        };
        let Some(bounds) = editor.update(cx, |view, _| view.caret_bounds_at(offset + 1)) else {
            return fail(format!("{text:?} isn't on screen"));
        };
        let at = bounds.center();
        let events = [
            PlatformInput::MouseMove(MouseMoveEvent {
                position: at,
                pressed_button: None,
                modifiers: Modifiers::none(),
            }),
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
        ];
        // Once this step's update of the workbench is over: the press's
        // listeners update it.
        window.defer(cx, move |window, cx| {
            for event in events {
                window.dispatch_event(event, cx);
            }
        });
    })
}

/// Fails the scene unless the active editor's menu is open, with the
/// context items if `context`, else with the language server's disabled.
fn check(context: bool) -> Step {
    Box::new(move |workbench, _, cx| {
        let Some(editor) = workbench.active_editor(cx) else {
            return fail("no active editor".to_owned());
        };
        let Some(entries) = editor.read(cx).editor_menu_entries() else {
            return fail("the right click opened no menu".to_owned());
        };
        let items: Vec<_> = entries.iter().flatten().collect();
        let titled = |title: &str| items.iter().find(|item| item.title == title);
        if context {
            for title in ["Run Test", "Toggle Breakpoint", "Annotate with Git Blame"] {
                if titled(title).is_none() {
                    return fail(format!("no {title} in the menu: {items:?}"));
                }
            }
        } else if !titled("Go to Definition").is_some_and(|item| !item.enabled) {
            fail(format!("Go to Definition isn't shown disabled: {items:?}"));
        }
    })
}

/// Presses `keys` in the open menu.
fn key(keys: &'static str) -> Step {
    Box::new(move |_, window, cx| {
        let keystroke = Keystroke::parse(keys).expect("a keystroke");
        window.defer(cx, move |window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        });
    })
}
