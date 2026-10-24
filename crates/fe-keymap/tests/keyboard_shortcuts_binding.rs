//! secondary-k secondary-s opens the Keyboard Shortcuts tab,
//! `workbench.open_keyboard_shortcuts`, for each platform's key, as VS
//! Code's cmd-k cmd-s does; the menus' Keyboard Shortcuts items run it and
//! `workbench.open_keyboard_shortcuts_json`, which keeps no key.

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

/// What pressing each of `keys` in turn runs in `stack`, if the last one
/// completes a binding.
fn run(keymap: &Keymap, keys: &[&str], stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = None;
    for key in keys {
        outcome = Some(matcher.push(Keystroke::parse(key).unwrap(), stack));
    }
    match outcome? {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn chord_binding_resolves_per_platform() {
    let stacks = [
        stack(&["workbench"]),
        stack(&["workbench", "editor"]),
        stack(&["workbench", "shortcuts_editor", "text_input"]),
        stack(&["workbench", "settings_editor", "text_input"]),
    ];
    for (os, keys) in [
        (Os::MacOs, ["cmd-k", "cmd-s"]),
        (Os::Linux, ["ctrl-k", "ctrl-s"]),
        (Os::Windows, ["ctrl-k", "ctrl-s"]),
    ] {
        let keymap = keymap(os);
        for stack in &stacks {
            assert_eq!(
                run(&keymap, &keys, stack).as_deref(),
                Some("workbench.open_keyboard_shortcuts"),
                "{os:?} {stack:?}"
            );
        }
        let bound: Vec<String> = (keymap.bindings().iter())
            .filter(|binding| binding.command.as_str() == "workbench.open_keyboard_shortcuts")
            .map(|binding| binding.keys.to_string())
            .collect();
        assert_eq!(bound, [keys.join(" ")], "{os:?}: one key");
        let json = (keymap.bindings().iter())
            .filter(|binding| binding.command.as_str() == "workbench.open_keyboard_shortcuts_json")
            .count();
        assert_eq!(json, 0, "{os:?}: Open Keyboard Shortcuts (JSON) has no key");
    }
}

#[test]
fn menus_list_both_after_settings() {
    for os in [Os::MacOs, Os::Linux] {
        let (bar, diagnostics) = fe_keymap::default_menus_for(None, os);
        assert_eq!(diagnostics, [], "{os:?}");
        let bar = bar.unwrap();
        let menu = (bar.menus.iter())
            .find(|menu| {
                (menu.items.iter())
                    .filter_map(|item| item.command())
                    .any(|item| item.title == "Settings…")
            })
            .expect("a Settings… item");
        let commands: Vec<&str> = (menu.items.iter())
            .filter_map(|item| item.command())
            .map(|item| item.command.as_str())
            .collect();
        let settings = commands
            .iter()
            .position(|command| *command == "workbench.open_settings_ui")
            .unwrap();
        assert_eq!(
            commands[settings + 1..settings + 3],
            [
                "workbench.open_keyboard_shortcuts",
                "workbench.open_keyboard_shortcuts_json"
            ],
            "{os:?}"
        );
    }
}
