//! Reopen Closed Editor's key, `secondary-shift-t`, in the workbench but
//! not in a focused terminal, and its File menu item after Close Tab.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{DEFAULT, DEFAULT_KEYMAP_PATH, KeySequence, Keymap, KeymapLayer, MenuBar};
use kiln_core::Os;

const COMMAND: &str = "workbench.reopen_closed_editor";

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(frames: &[&'static str]) -> ContextStack {
    frames.iter().copied().map(ContextFrame::new).collect()
}

/// What the key runs in `stack`: the last binding of exactly that key
/// whose context holds.
fn key(keymap: &Keymap, os: Os, stack: &ContextStack) -> Option<String> {
    let keys = KeySequence::parse_for("secondary-shift-t", os).unwrap();
    keymap
        .bindings()
        .iter()
        .rev()
        .find(|binding| {
            binding.keys == keys && binding.context.as_ref().is_none_or(|c| c.eval(stack))
        })
        .map(|binding| binding.command.as_str().to_owned())
}

#[test]
fn secondary_shift_t_reopens_in_the_workbench_not_the_terminal() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for frames in [&["workbench"][..], &["workbench", "editor"]] {
            assert_eq!(
                key(&keymap, os, &stack(frames)).as_deref(),
                Some(COMMAND),
                "{os:?} {frames:?}"
            );
        }
        assert_eq!(
            key(&keymap, os, &stack(&["workbench", "terminal"])),
            None,
            "{os:?}"
        );
    }
}

fn file_ids(bar: &MenuBar) -> Vec<String> {
    let file = bar.menu("File").unwrap();
    (file.items.iter())
        .map(|item| item.id().unwrap_or("-").to_owned())
        .collect()
}

#[test]
fn the_file_menu_lists_it_after_close_tab() {
    let (macos, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let (linux, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    for bar in [macos.unwrap(), linux.unwrap()] {
        let ids = file_ids(&bar);
        let close = ids.iter().position(|id| id == "workbench.close_tab");
        assert_eq!(ids[close.unwrap() + 1], COMMAND, "{ids:?}");
        let item = (bar.menu("File"))
            .and_then(|menu| menu.item(COMMAND))
            .and_then(|item| item.command())
            .unwrap();
        assert_eq!(item.title, "Reopen Closed Editor");
    }
}
