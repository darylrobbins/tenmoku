//! The spelling commands' one key and their menu items.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, ItemKind, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Keystroke, Matcher, MenuBar,
};
use kiln_core::Os;

const SUGGEST: &str = "spell.suggest";

/// The items of the Edit menu's spelling group, in order.
const ITEMS: &[(&str, &str)] = &[
    (SUGGEST, "Spelling: Suggest Corrections…"),
    ("spell.next", "Spelling: Next Misspelling"),
    ("spell.previous", "Spelling: Previous Misspelling"),
    ("spell.add_word", "Spelling: Add Word to Dictionary"),
    ("spell.ignore_word", "Spelling: Ignore Word"),
    ("spell.toggle", "Spelling: Toggle Spell Check"),
];

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

/// `keys` is `spell.suggest`'s alone, in the editor's context, and only `keys`
/// is bound to any spelling command. Neither platform's key holds another
/// command, so nothing is shadowed or in the way on moving between them.
fn assert_only_binding(keymap: &Keymap, keys: &str, other: &str) {
    let bound = |keys: &str| KeySequence::parse(keys).unwrap();
    let ours: Vec<_> = (keymap.bindings().iter())
        .filter(|binding| binding.command.as_str().starts_with("spell."))
        .collect();
    assert_eq!(ours.len(), 1, "{ours:?}");
    assert_eq!(ours[0].command.as_str(), SUGGEST);
    assert_eq!(ours[0].keys, bound(keys));
    assert_eq!(
        ours[0].context.as_ref().map(ToString::to_string).as_deref(),
        Some("editor")
    );
    for key in [keys, other] {
        let holders: Vec<_> = (keymap.bindings().iter())
            .filter(|binding| binding.keys == bound(key) && binding.command.as_str() != SUGGEST)
            .map(|binding| binding.command.as_str())
            .collect();
        assert_eq!(holders, Vec::<&str>::new(), "{key}");
    }
    assert_eq!(
        run(keymap, keys, &["workbench", "editor"]).as_deref(),
        Some(SUGGEST)
    );
    assert_eq!(run(keymap, keys, &["workbench"]), None);
}

fn assert_menu_items(bar: MenuBar, keymap: &Keymap, suggest_key: &str) {
    let edit = bar.menu("Edit").unwrap();
    let first = (edit.items.iter())
        .position(|item| item.id() == Some(SUGGEST))
        .unwrap();
    assert_eq!(edit.items[first - 1].kind, ItemKind::Separator);
    assert_eq!(edit.items[first + ITEMS.len()].kind, ItemKind::Separator);
    for (at, (id, title)) in ITEMS.iter().enumerate() {
        let item = edit.items[first + at].command().unwrap();
        assert_eq!(item.id, *id);
        assert_eq!(item.title, *title);
        let key = (*id == SUGGEST).then(|| KeySequence::parse(suggest_key).unwrap());
        assert_eq!(item.shortcut(keymap), key, "{id}");
    }
}

#[test]
fn quick_fix_key_and_menu() {
    let macos = keymap(DEFAULT_MACOS, Os::MacOs);
    assert_only_binding(&macos, "alt-cmd-.", "ctrl-alt-.");
    let linux = keymap(DEFAULT_LINUX, Os::Linux);
    assert_only_binding(&linux, "ctrl-alt-.", "alt-cmd-.");

    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    assert_menu_items(bar.unwrap(), &macos, "alt-cmd-.");
    let (bar, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    assert_menu_items(bar.unwrap(), &linux, "ctrl-alt-.");
}
