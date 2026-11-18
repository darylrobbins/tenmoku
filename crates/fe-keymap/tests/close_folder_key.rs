//! File > Close Folder's key: `secondary-k f` (cmd-k f on macOS, ctrl-k f
//! elsewhere) in the workbench, with Format Selection's `secondary-k
//! secondary-f` still its own in an editor, and its File menu item between
//! Close Pane and Close Window.

use std::path::Path;

use fe_command::{CommandId, ContextFrame, ContextStack, Value};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, KeySequence, Keymap, KeymapLayer, Matcher, MenuBar,
    shortcuts,
};
use kiln_core::Os;

const COMMAND: &str = "workbench.close_folder";

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(frames: &[&'static str]) -> ContextStack {
    frames.iter().copied().map(ContextFrame::new).collect()
}

/// What `keys` runs in `stack`, through the matcher as the app feeds it.
fn run(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = KeyOutcome::Unmatched(Vec::new());
    for keystroke in KeySequence::parse_for(keys, os).unwrap().keystrokes() {
        outcome = matcher.push(*keystroke, stack);
    }
    if outcome == KeyOutcome::Pending {
        outcome = matcher.flush();
    }
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn secondary_k_f_closes_the_folder() {
    for (os, keys, format) in [
        (Os::MacOs, "cmd-k f", "cmd-k cmd-f"),
        (Os::Linux, "ctrl-k f", "ctrl-k ctrl-f"),
    ] {
        let keymap = keymap(os);
        for frames in [
            &["workbench"][..],
            &["workbench", "editor"],
            &["workbench", "project_panel"],
        ] {
            assert_eq!(
                run(&keymap, os, keys, &stack(frames)).as_deref(),
                Some(COMMAND),
                "{os:?} {frames:?}"
            );
        }
        assert_eq!(
            run(&keymap, os, format, &stack(&["workbench", "editor"])).as_deref(),
            Some("lsp.format_selection"),
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
fn the_file_menu_lists_it_between_close_pane_and_close_window() {
    let (macos, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let (linux, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    for (os, bar, keys) in [
        (Os::MacOs, macos.unwrap(), "cmd-k f"),
        (Os::Linux, linux.unwrap(), "ctrl-k f"),
    ] {
        let ids = file_ids(&bar);
        let pane = ids.iter().position(|id| id == "pane.close").unwrap();
        assert_eq!(
            ids[pane + 1..pane + 3],
            [COMMAND, "window.close"],
            "{os:?} {ids:?}"
        );
        let item = (bar.menu("File"))
            .and_then(|menu| menu.item(COMMAND))
            .and_then(|item| item.command())
            .unwrap();
        assert_eq!(item.title, "Close Folder", "{os:?}");
        // Its key, as the menu resolves it: a chord, which a menu
        // accelerator can't show, as Close Pane's.
        let keymap = keymap(os);
        let id = CommandId::parse(COMMAND).unwrap();
        let workbench = stack(&["workbench"]);
        assert_eq!(
            shortcuts(&keymap, &id, &Value::Null, &workbench),
            [KeySequence::parse_for(keys, os).unwrap()],
            "{os:?}"
        );
        assert_eq!(item.shortcut(&keymap), None, "{os:?}");
    }
}
