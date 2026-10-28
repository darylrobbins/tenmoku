//! Close Saved Editors and Close Editors in Group: cmd-k u and cmd-k w
//! (ctrl-k elsewhere), in the workbench. cmd-k cmd-w stays Close Pane, and
//! Close All Editors has no key.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{DEFAULT, DEFAULT_KEYMAP_PATH, KeySequence, Keymap, KeymapLayer};
use kiln_core::Os;

fn keymap(os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// What `keys` runs in the workbench: the last binding of exactly that
/// sequence.
fn runs(keymap: &Keymap, os: Os, keys: &str) -> Option<String> {
    let stack: ContextStack = [ContextFrame::new("workbench")].into_iter().collect();
    let keys = KeySequence::parse_for(keys, os).unwrap();
    keymap
        .bindings()
        .iter()
        .rev()
        .find(|binding| {
            binding.keys == keys && binding.context.as_ref().is_none_or(|c| c.eval(&stack))
        })
        .map(|binding| binding.command.as_str().to_owned())
}

#[test]
fn the_keys_close_saved_and_group_editors() {
    for (os, chord, close) in [
        (Os::MacOs, "cmd-k", "cmd-k cmd-w"),
        (Os::Linux, "ctrl-k", "ctrl-k ctrl-w"),
    ] {
        let keymap = keymap(os);
        let key = |last: &str| runs(&keymap, os, &format!("{chord} {last}"));
        assert_eq!(key("u").as_deref(), Some("workbench.close_saved"), "{os:?}");
        assert_eq!(
            key("w").as_deref(),
            Some("workbench.close_group_editors"),
            "{os:?}"
        );
        assert_eq!(runs(&keymap, os, close).as_deref(), Some("pane.close"));
        assert!(
            keymap
                .bindings()
                .iter()
                .all(|binding| binding.command.as_str() != "workbench.close_all_editors")
        );
    }
}
