//! Insert Line Below / Above and Copy Lines Up keys.

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

fn stack(frame: ContextFrame) -> ContextStack {
    [ContextFrame::new("workbench"), frame]
        .into_iter()
        .collect()
}

fn vim(mode: &str) -> ContextStack {
    stack(
        ContextFrame::new("editor")
            .flag("vim.enabled")
            .set("vim.mode", mode),
    )
}

/// What `keys` runs in `stack`: the last binding of exactly that key whose
/// context holds.
fn resolve(keymap: &Keymap, os: Os, keys: &str, stack: &ContextStack) -> Option<String> {
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

const KEYS: [(&str, &str, &str, &str); 3] = [
    (
        "cmd-enter",
        "ctrl-enter",
        "editor.insert_line_below",
        "below",
    ),
    (
        "cmd-shift-enter",
        "ctrl-shift-enter",
        "editor.insert_line_above",
        "above",
    ),
    (
        "shift-alt-up",
        "ctrl-shift-alt-up",
        "editor.copy_rows_up",
        "copy up",
    ),
];

#[test]
fn bound_in_the_editor_outside_vim_normal_mode() {
    for (os, column) in [(Os::MacOs, 0), (Os::Linux, 1)] {
        let map = keymap(os);
        for (mac, other, command, name) in KEYS {
            let keys = if column == 0 { mac } else { other };
            let editor = stack(ContextFrame::new("editor"));
            assert_eq!(
                resolve(&map, os, keys, &editor).as_deref(),
                Some(command),
                "{name} on {os:?}"
            );
            for mode in ["insert", "replace"] {
                assert_eq!(
                    resolve(&map, os, keys, &vim(mode)).as_deref(),
                    Some(command),
                    "{name} in vim {mode} on {os:?}"
                );
            }
            assert_ne!(
                resolve(&map, os, keys, &vim("normal")).as_deref(),
                Some(command),
                "{name} in vim normal on {os:?}"
            );
        }
    }
}

#[test]
fn cmd_enter_stays_with_proposals_and_the_commit_box() {
    let mac = keymap(Os::MacOs);
    let proposal = stack(ContextFrame::new("editor").flag("proposal"));
    assert_eq!(
        resolve(&mac, Os::MacOs, "cmd-enter", &proposal).as_deref(),
        Some("proposal.accept_all")
    );
    let panel = stack(ContextFrame::new("git_panel"));
    assert_eq!(
        resolve(&mac, Os::MacOs, "cmd-enter", &panel).as_deref(),
        Some("git.commit_box.commit")
    );
    let in_box = [
        ContextFrame::new("workbench"),
        ContextFrame::new("git_panel"),
        ContextFrame::new("editor"),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        resolve(&mac, Os::MacOs, "cmd-enter", &in_box).as_deref(),
        Some("git.commit_box.commit")
    );
}
