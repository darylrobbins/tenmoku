//! ⌥↵ (`alt-enter`) applies the selected problem's suggested fix
//! (`problems.apply_fix`) in the Problems panel's key context, with vim on
//! or off, on each platform; in an editor it runs nothing of the panel's.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, Keymap, KeymapLayer,
    Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// The workbench's stack with `inner` in it, its frame with the `vim`
/// flag if `vim`.
fn stack(inner: &str, vim: bool) -> ContextStack {
    let mut frame = ContextFrame::new(inner);
    if vim {
        frame = frame.flag("vim");
    }
    [ContextFrame::new("workbench"), frame]
        .into_iter()
        .collect()
}

/// The command `key` runs in `stack`, if any.
fn run(keymap: &Keymap, key: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(key).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn alt_enter_applies_the_fix_in_the_problems_panel() {
    for (source, os) in [
        (DEFAULT, Os::MacOs),
        (DEFAULT_MACOS, Os::MacOs),
        (DEFAULT, Os::Linux),
        (DEFAULT_LINUX, Os::Linux),
    ] {
        let keymap = keymap(source, os);
        for vim in [false, true] {
            assert_eq!(
                run(&keymap, "alt-enter", &stack("problems_panel", vim)).as_deref(),
                Some("problems.apply_fix"),
                "{os:?}, vim {vim}"
            );
        }
        // Enter still opens the problem.
        assert_eq!(
            run(&keymap, "enter", &stack("problems_panel", false)).as_deref(),
            Some("problems.open"),
            "{os:?}"
        );
        // Outside the panel, it isn't the panel's.
        let editor = run(&keymap, "alt-enter", &stack("editor", false));
        assert_ne!(editor.as_deref(), Some("problems.apply_fix"), "{os:?}");
    }
}
