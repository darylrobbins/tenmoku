//! The task commands' one key and their menu items.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, ItemKind, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Keystroke, Matcher, MenuBar,
};
use kiln_core::Os;

const COMMAND: &str = "tasks.run_build";

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// What `keys` runs in the contexts `frames`, if anything.
fn run(keymap: &Keymap, keys: &str, frames: &[&str]) -> Option<String> {
    let stack: ContextStack = frames.iter().map(|name| ContextFrame::new(*name)).collect();
    match Matcher::new(keymap.clone()).push(Keystroke::parse(keys).unwrap(), &stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

/// `keys` is `COMMAND`'s alone, on the one binding, in the workbench's context.
fn assert_only_binding(keymap: &Keymap, keys: &str, other: &str) {
    let bound = |keys: &str| KeySequence::parse(keys).unwrap();
    let ours: Vec<_> = (keymap.bindings().iter())
        .filter(|binding| binding.command.as_str() == COMMAND)
        .collect();
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert_eq!(ours[0].keys, bound(keys));
    assert_eq!(
        ours[0].context.as_ref().map(ToString::to_string).as_deref(),
        Some("workbench")
    );
    for key in [keys, other] {
        let holders: Vec<_> = (keymap.bindings().iter())
            .filter(|binding| binding.keys == bound(key) && binding.command.as_str() != COMMAND)
            .map(|binding| binding.command.as_str())
            .collect();
        assert_eq!(holders, Vec::<&str>::new(), "{key}");
    }
    assert_eq!(run(keymap, keys, &["workbench"]).as_deref(), Some(COMMAND));
    assert_eq!(
        run(keymap, keys, &["workbench", "editor"]).as_deref(),
        Some(COMMAND)
    );
}

#[test]
fn cmd_shift_b_runs_the_build_task() {
    let macos = keymap(DEFAULT_MACOS, Os::MacOs);
    assert_only_binding(&macos, "cmd-shift-b", "ctrl-shift-b");
    let linux = keymap(DEFAULT_LINUX, Os::Linux);
    assert_only_binding(&linux, "ctrl-shift-b", "cmd-shift-b");
}

/// The rest are palette-only.
#[test]
fn the_other_task_commands_have_no_key() {
    for keymap in [
        keymap(DEFAULT_MACOS, Os::MacOs),
        keymap(DEFAULT_LINUX, Os::Linux),
    ] {
        let bound = (keymap.bindings().iter())
            .any(|binding| ["tasks.run", "tasks.rerun_last"].contains(&binding.command.as_str()));
        assert!(!bound);
    }
}

fn assert_menu_items(bar: MenuBar, keymap: &Keymap, build_key: &str) {
    // There's no Terminal menu: they're a group of their own in View.
    assert!(bar.menu("Terminal").is_none());
    let view = bar.menu("View").unwrap();
    let run = (view.items.iter())
        .position(|item| item.id() == Some("tasks.run"))
        .unwrap();
    assert_eq!(view.items[run - 1].kind, ItemKind::Separator);
    assert_eq!(view.items[run + 1].id(), Some(COMMAND));
    assert_eq!(view.items[run + 2].kind, ItemKind::Separator);

    let command = |id: &str| view.item(id).and_then(|item| item.command()).unwrap();
    assert_eq!(command("tasks.run").title, "Run Task…");
    assert_eq!(command("tasks.run").shortcut(keymap), None);
    assert_eq!(command(COMMAND).title, "Run Build Task");
    assert_eq!(
        command(COMMAND).shortcut(keymap),
        Some(KeySequence::parse(build_key).unwrap())
    );
}

#[test]
fn run_task_and_run_build_task_are_in_the_menu() {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let keymap_macos = keymap(DEFAULT_MACOS, Os::MacOs);
    assert_menu_items(bar.unwrap(), &keymap_macos, "cmd-shift-b");
    let (bar, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    let keymap_linux = keymap(DEFAULT_LINUX, Os::Linux);
    assert_menu_items(bar.unwrap(), &keymap_linux, "ctrl-shift-b");
}
