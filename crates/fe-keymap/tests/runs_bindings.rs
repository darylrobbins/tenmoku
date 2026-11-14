//! `␣r` shows the Runs tool window (`runs.show`): space then `r` in an
//! editor in vim's normal mode with nothing pending, on each platform. The
//! which-key popup lists it after space, as it lists a chord's next keys
//! (`Keymap::continuations`); space before any other key is handed back to
//! vim, and elsewhere space is no chord.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Keystroke, Matcher,
};
use kiln_core::Os;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// An editor's stack, with vim in `mode` if it's on, and something pending
/// if `pending`.
fn editor(mode: Option<&str>, pending: bool) -> ContextStack {
    let mut frame = ContextFrame::new("editor");
    if let Some(mode) = mode {
        frame = frame.flag("vim.enabled").set("vim.mode", mode);
        if mode.starts_with("visual") {
            frame = frame.flag("vim.visual");
        }
    }
    if pending {
        frame = frame
            .flag("vim.pending")
            .set("vim.pending_kind", "operator");
    }
    [ContextFrame::new("workbench"), frame]
        .into_iter()
        .collect()
}

/// What typing `keys`, one keystroke after another, comes to in `stack`.
fn outcome(keymap: &Keymap, keys: &str, stack: &ContextStack) -> KeyOutcome {
    let mut matcher = Matcher::new(keymap.clone());
    let mut outcome = KeyOutcome::Pending;
    for key in keys.split(' ') {
        outcome = matcher.push(Keystroke::parse(key).unwrap(), stack);
        if outcome != KeyOutcome::Pending {
            break;
        }
    }
    outcome
}

/// The command typing `keys` runs in `stack`, if any.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    match outcome(keymap, keys, stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

#[test]
fn space_r_runs_show() {
    let space = Keystroke::parse("space").unwrap();
    let r = Keystroke::parse("r").unwrap();
    for (sources, os) in [
        ([DEFAULT_MACOS, DEFAULT], Os::MacOs),
        ([DEFAULT_LINUX, DEFAULT], Os::Linux),
    ] {
        for source in sources {
            let keymap = keymap(source, os);
            let normal = editor(Some("normal"), false);
            assert_eq!(
                run(&keymap, "space r", &normal).as_deref(),
                Some("runs.show"),
                "{os:?}"
            );

            // Which-key, after space, lists `r` with its command.
            let next = keymap.continuations(&[space], &normal);
            let listed = next.iter().find(|next| next.next == r);
            let listed = listed.expect("which-key lists r after space");
            assert_eq!(
                listed.command.as_ref().map(|id| id.as_str()),
                Some("runs.show"),
                "{os:?}"
            );

            // Space before another key is vim's: both are handed back.
            let l = Keystroke::parse("l").unwrap();
            assert_eq!(
                outcome(&keymap, "space l", &normal),
                KeyOutcome::Unmatched(vec![space, l]),
                "{os:?}"
            );

            // Not with an operator pending, nor in insert or visual mode,
            // nor with vim off: there space is no chord.
            for stack in [
                editor(Some("normal"), true),
                editor(Some("insert"), false),
                editor(Some("visual"), false),
                editor(None, false),
            ] {
                assert_eq!(run(&keymap, "space r", &stack), None, "{os:?} {stack:?}");
                assert!(keymap.continuations(&[space], &stack).is_empty(), "{os:?}");
            }
        }
    }
}

/// The command `keys` run in `stack`, pressed one after another, if any; a
/// chord left pending runs what the chord timer would.
fn run_flushed(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let keys = KeySequence::parse(keys).unwrap();
    let (last, first) = keys.keystrokes().split_last().unwrap();
    for keystroke in first {
        if matcher.push(*keystroke, stack) != KeyOutcome::Pending {
            return None;
        }
    }
    let outcome = match matcher.push(*last, stack) {
        KeyOutcome::Pending => matcher.flush(),
        outcome => outcome,
    };
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

/// With vim on, the dock keys leave the Runs tool window, as they leave
/// Problems (`dock_leave_keys.rs`): its context is `runs_panel`, with the
/// `vim` flag. With vim off they're not the dock's.
#[test]
fn dock_keys_leave_the_runs_panel() {
    let runs = |vim: bool| -> ContextStack {
        let frame = ContextFrame::new("runs_panel");
        let frame = if vim { frame.flag("vim") } else { frame };
        [ContextFrame::new("workbench"), frame]
            .into_iter()
            .collect()
    };
    for (sources, os) in [
        ([DEFAULT_MACOS, DEFAULT], Os::MacOs),
        ([DEFAULT_LINUX, DEFAULT], Os::Linux),
    ] {
        for source in sources {
            let keymap = keymap(source, os);
            for (keys, command) in [
                ("escape", "dock.focus_editor"),
                ("ctrl-w p", "dock.focus_editor"),
                ("ctrl-w w", "dock.focus_editor"),
                ("ctrl-w h", "dock.focus_left"),
                ("ctrl-w ctrl-j", "dock.focus_down"),
                ("ctrl-w k", "dock.focus_up"),
                ("ctrl-w l", "dock.focus_right"),
                ("ctrl-h", "dock.focus_left"),
                ("ctrl-j", "dock.focus_down"),
                ("ctrl-k", "dock.focus_up"),
                ("ctrl-l", "dock.focus_right"),
            ] {
                assert_eq!(
                    run_flushed(&keymap, keys, &runs(true)).as_deref(),
                    Some(command),
                    "{os:?} {keys}"
                );
                let off = run_flushed(&keymap, keys, &runs(false));
                let dock = off
                    .as_deref()
                    .is_some_and(|got| got.starts_with("dock.focus_"));
                assert!(!dock, "{os:?} {keys} with vim off ran {off:?}");
            }
        }
    }
}
