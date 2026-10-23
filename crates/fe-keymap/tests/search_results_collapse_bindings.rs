//! `left` and `right` collapse and expand the search results' file groups,
//! with vim on or off, and resolve to neither command anywhere else.

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

fn stack(frames: &[ContextFrame]) -> ContextStack {
    frames.iter().cloned().collect()
}

fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(keys).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn left_and_right_resolve_in_search_results() {
    let results = |vim: bool| {
        let frame = ContextFrame::new("search_results");
        let frame = if vim { frame.flag("vim") } else { frame };
        stack(&[
            ContextFrame::new("workbench"),
            ContextFrame::new("search_panel"),
            frame,
        ])
    };
    let elsewhere = [
        stack(&[ContextFrame::new("workbench"), ContextFrame::new("editor")]),
        stack(&[
            ContextFrame::new("workbench"),
            ContextFrame::new("search_panel"),
            ContextFrame::new("text_input"),
        ]),
        stack(&[ContextFrame::new("workbench")]),
    ];
    let commands = [
        ("left", "search.collapse_or_parent"),
        ("right", "search.expand_or_first_child"),
    ];
    for os in [Os::MacOs, Os::Linux] {
        let keymap = keymap(os);
        for (keys, command) in commands {
            for vim in [false, true] {
                assert_eq!(
                    run(&keymap, keys, &results(vim)).as_deref(),
                    Some(command),
                    "{os:?} {keys} vim {vim}"
                );
            }
            for stack in &elsewhere {
                assert_ne!(
                    run(&keymap, keys, stack).as_deref(),
                    Some(command),
                    "{os:?} {keys} in {stack:?}"
                );
            }
        }
        // Only left and right, not vim's h and l.
        let bound: Vec<_> = (keymap.bindings().iter())
            .filter(|binding| commands.iter().any(|(_, c)| *c == binding.command.as_str()))
            .map(|binding| binding.keys.to_string())
            .collect();
        assert_eq!(bound, ["left", "right"], "{os:?}");
    }
}
