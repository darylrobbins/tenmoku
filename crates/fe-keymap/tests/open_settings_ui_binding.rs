//! secondary-comma opens the Settings editor, `workbench.open_settings_ui`,
//! for each platform's key, as do the menus' Settings… items;
//! `workbench.open_settings`, the file, keeps no key.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KeyOutcome, Keymap, KeymapLayer, Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

fn stack(frames: &[&str]) -> ContextStack {
    frames.iter().map(|name| ContextFrame::new(*name)).collect()
}

fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(keys).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn comma_binding_runs_the_ui() {
    let stacks = [
        stack(&["workbench"]),
        stack(&["workbench", "editor"]),
        stack(&["workbench", "settings_editor", "text_input"]),
    ];
    for (os, keys) in [(Os::MacOs, "cmd-,"), (Os::Linux, "ctrl-,")] {
        let keymap = keymap(os);
        for stack in &stacks {
            assert_eq!(
                run(&keymap, keys, stack).as_deref(),
                Some("workbench.open_settings_ui"),
                "{os:?} {stack:?}"
            );
        }
        let file = (keymap.bindings().iter())
            .filter(|binding| binding.command.as_str() == "workbench.open_settings")
            .count();
        assert_eq!(file, 0, "{os:?}: Open Settings (JSON) has no key");
    }
}

#[test]
fn settings_menu_items_run_the_ui() {
    for os in [Os::MacOs, Os::Linux] {
        let (bar, diagnostics) = fe_keymap::default_menus_for(None, os);
        assert_eq!(diagnostics, [], "{os:?}");
        let items: Vec<_> = (bar.unwrap().menus.iter())
            .flat_map(|menu| menu.items.iter().filter_map(|item| item.command()).cloned())
            .filter(|item| item.title == "Settings…")
            .collect();
        assert_eq!(items.len(), 1, "{os:?}: one Settings… item");
        assert_eq!(
            items[0].command.as_str(),
            "workbench.open_settings_ui",
            "{os:?}"
        );
    }
}
