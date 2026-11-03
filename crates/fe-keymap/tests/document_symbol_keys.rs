//! `secondary-shift-o` runs Go to Symbol in File, as VS Code binds it, and
//! the Outline panel stays reachable: its own key, vim's `g shift-o`, the
//! View menu.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

const SYMBOLS: &str = "nav.document_symbols";
const OUTLINE: &str = "outline.show";

fn each_keymap(check: impl Fn(&Keymap, &str, Os)) {
    for (sources, secondary, os) in [
        ([DEFAULT_MACOS, DEFAULT], "cmd", Os::MacOs),
        ([DEFAULT_LINUX, DEFAULT], "ctrl", Os::Linux),
    ] {
        for source in sources {
            let (layer, diagnostics) =
                KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
            assert_eq!(diagnostics, [], "{os:?}");
            check(&Keymap::from_layers(vec![layer.unwrap()]), secondary, os);
        }
    }
}

fn workbench() -> ContextFrame {
    ContextFrame::new("workbench")
}

/// What typing `keys` runs in `stack`, if anything.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = KeyOutcome::Pending;
    for key in keys.split(' ') {
        outcome = matcher.push(Keystroke::parse(key).unwrap(), stack);
        if outcome != KeyOutcome::Pending {
            break;
        }
    }
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn secondary_shift_o_goes_to_a_symbol() {
    each_keymap(|keymap, secondary, os| {
        let key = format!("{secondary}-shift-o");
        let editor: ContextStack = [workbench(), ContextFrame::new("editor")]
            .into_iter()
            .collect();
        assert_eq!(
            run(keymap, &key, &editor).as_deref(),
            Some(SYMBOLS),
            "{os:?} in an editor"
        );
        let picker: ContextStack = [workbench(), ContextFrame::new("picker")]
            .into_iter()
            .collect();
        assert_eq!(
            run(keymap, &key, &picker).as_deref(),
            Some(SYMBOLS),
            "{os:?} in a picker, as a toggle"
        );
        let terminal: ContextStack = [workbench(), ContextFrame::new("terminal")]
            .into_iter()
            .collect();
        assert_ne!(
            run(keymap, &key, &terminal).as_deref(),
            Some(SYMBOLS),
            "{os:?} the terminal keeps the key"
        );
    });
}

#[test]
fn outline_show_stays_bound() {
    each_keymap(|keymap, secondary, os| {
        let stack: ContextStack = [workbench()].into_iter().collect();
        assert_eq!(
            run(keymap, &format!("{secondary}-alt-o"), &stack).as_deref(),
            Some(OUTLINE),
            "{os:?} its own key"
        );
        let normal: ContextStack = [
            workbench(),
            ContextFrame::new("editor")
                .flag("vim.enabled")
                .set("vim.mode", "normal"),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            run(keymap, "g shift-o", &normal).as_deref(),
            Some(OUTLINE),
            "{os:?} vim's g shift-o"
        );
    });
}

#[test]
fn the_menus_have_both() {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let bar = bar.expect("the default menus load");
    assert!(bar.menu("Go").unwrap().item(SYMBOLS).is_some());
    assert!(bar.menu("View").unwrap().item(OUTLINE).is_some());
}
