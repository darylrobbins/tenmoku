//! Show Call Hierarchy's key, shift-alt-h on every platform, and its Go menu entry.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{DEFAULT, DEFAULT_KEYMAP_PATH, KeySequence, Keymap, KeymapLayer, MenuBar};
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

fn resolves(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
    let keys = KeySequence::parse_for(keys, os).unwrap();
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
fn shift_alt_h_shows_the_call_hierarchy_on_macos_and_linux() {
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for frames in [&["workbench", "editor"][..], &["workbench"]] {
            assert_eq!(
                resolves(&keymap, os, "shift-alt-h", &stack(frames)).as_deref(),
                Some("nav.show_call_hierarchy"),
                "{os:?} {frames:?}"
            );
        }
        // Not in a terminal.
        assert_eq!(
            resolves(
                &keymap,
                os,
                "shift-alt-h",
                &stack(&["workbench", "terminal"])
            ),
            None,
            "{os:?}"
        );
    }
}

#[test]
fn the_go_menu_names_it_after_go_to_implementations() {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let bar: MenuBar = bar.expect("the default menus load");
    let ids: Vec<_> = (bar.menu("Go").unwrap().items.iter())
        .map(|item| item.id().unwrap_or("-").to_owned())
        .collect();
    let at = |command: &str| ids.iter().position(|id| id == command);
    // Peek References sits between them on the Mac (T1150).
    let implementation = at("nav.goto_implementation").expect("Go to Implementations");
    let call_hierarchy = at("nav.show_call_hierarchy").expect("Show Call Hierarchy");
    assert!(
        ids[implementation + 1..call_hierarchy]
            .iter()
            .all(|id| id == "editor.peek_references"),
        "{ids:?}"
    );
}
