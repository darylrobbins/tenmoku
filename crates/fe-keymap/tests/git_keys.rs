//! The git and diff commands' keys and menu items, none
//! of them a key vim uses in its normal, visual or operator-pending mode.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeyOutcome, KeySequence, Keymap,
    KeymapLayer, Keystroke, Matcher, MenuBar,
};
use fe_vim::{Pending, Step, VimKey, step};
use kiln_core::Os;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// The workbench, then `frames`.
fn stack(frames: Vec<ContextFrame>) -> ContextStack {
    std::iter::once(ContextFrame::new("workbench"))
        .chain(frames)
        .collect()
}

/// An editor, vim off, or with vim on in `mode`, `visual` and `pending`.
fn editor(vim: Option<(&str, bool, bool)>) -> ContextFrame {
    let frame = ContextFrame::new("editor");
    match vim {
        None => frame,
        Some((mode, visual, pending)) => {
            let mut frame = frame.flag("vim.enabled").set("vim.mode", mode);
            if visual {
                frame = frame.flag("vim.visual");
            }
            if pending {
                frame = frame
                    .flag("vim.pending")
                    .set("vim.pending_kind", "operator");
            }
            frame
        }
    }
}

/// The command `keys` runs in `stack`, if any.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    match matcher.push(Keystroke::parse(keys).unwrap(), stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

fn diff_view() -> ContextStack {
    stack(vec![ContextFrame::new("DiffView"), editor(None)])
}

fn plain_editor() -> ContextStack {
    stack(vec![editor(None)])
}

fn commit_box() -> ContextStack {
    stack(vec![ContextFrame::new("git_panel"), editor(None)])
}

/// (keys, command, stack) on `os`.
fn card(os: Os) -> Vec<(&'static str, &'static str, ContextStack)> {
    let mac = os == Os::MacOs;
    let primary = |mac_keys, other| if mac { mac_keys } else { other };
    vec![
        ("ctrl-shift-g", "git.show_changes", plain_editor()),
        ("f7", "diff.next_change", diff_view()),
        ("shift-f7", "diff.previous_change", diff_view()),
        ("alt-f5", "diff.next_change", diff_view()),
        ("shift-alt-f5", "diff.previous_change", diff_view()),
        ("alt-f3", "git.next_change", plain_editor()),
        ("shift-alt-f3", "git.previous_change", plain_editor()),
        (
            primary("cmd-y", "ctrl-alt-y"),
            "git.stage_hunk",
            plain_editor(),
        ),
        (
            primary("cmd-alt-z", "ctrl-alt-z"),
            "git.revert_hunk",
            plain_editor(),
        ),
        (
            primary("cmd-enter", "ctrl-enter"),
            "git.commit_box.commit",
            commit_box(),
        ),
    ]
}

fn platforms() -> [(Os, Keymap, Keymap); 2] {
    [
        (
            Os::MacOs,
            keymap(DEFAULT_MACOS, Os::MacOs),
            keymap(DEFAULT, Os::MacOs),
        ),
        (
            Os::Linux,
            keymap(DEFAULT_LINUX, Os::Linux),
            keymap(DEFAULT, Os::Linux),
        ),
    ]
}

#[test]
fn bound() {
    for (os, generated, table) in platforms() {
        for (keys, command, stack) in card(os) {
            for keymap in [&generated, &table] {
                assert_eq!(
                    run(keymap, keys, &stack).as_deref(),
                    Some(command),
                    "{os:?} {keys}"
                );
            }
        }
    }
    // Each is for its context only.
    for (os, keymap, _) in platforms() {
        assert_eq!(run(&keymap, "f7", &plain_editor()), None, "{os:?}");
        assert_eq!(
            run(&keymap, "alt-f3", &diff_view()).as_deref(),
            Some("git.next_change")
        );
    }
}

/// The vim modes whose keys are vim's: normal, operator-pending and each
/// visual mode, as `VimState::stamp` puts them on the editor's context.
fn vim_modes() -> Vec<ContextFrame> {
    let on = || ContextFrame::new("editor").flag("vim.enabled");
    vec![
        on().set("vim.mode", "normal"),
        on().set("vim.mode", "operator_pending")
            .flag("vim.pending")
            .set("vim.pending_kind", "operator"),
        on().set("vim.mode", "visual").flag("vim.visual"),
        on().set("vim.mode", "visual_line").flag("vim.visual"),
        on().set("vim.mode", "visual_block").flag("vim.visual"),
    ]
}

/// The key fe-vim's reducer reads for `keys`, as fe-gpui's `vim_key` does:
/// none with cmd; `ctrl-[` is `<Esc>`, `ctrl-<letter>` is `<C-letter>`; the
/// named keys are those keys, and a plain character is itself. A function key, or
/// an alt chord without ctrl, types nothing, so none.
fn vim_key(keys: &str) -> Option<VimKey> {
    let mut parts: Vec<&str> = keys.split('-').collect();
    let key = parts.pop().unwrap();
    if parts.contains(&"cmd") {
        return None;
    }
    // Alt doesn't keep a control key from being vim's `<C-x>`.
    if parts.contains(&"alt") && !parts.contains(&"ctrl") {
        return None;
    }
    let named = match key {
        "escape" => Some(VimKey::Esc),
        "enter" => Some(VimKey::Enter),
        "backspace" => Some(VimKey::Backspace),
        "tab" => Some(VimKey::Tab),
        "left" => Some(VimKey::Left),
        "right" => Some(VimKey::Right),
        "up" => Some(VimKey::Up),
        "down" => Some(VimKey::Down),
        _ => None,
    };
    let mut chars = key.chars();
    let letter = chars.next().filter(|_| chars.next().is_none());
    if parts.contains(&"ctrl") {
        return match letter {
            Some('[') => Some(VimKey::Esc),
            Some(c) if c.is_ascii_alphabetic() => Some(VimKey::Ctrl(c.to_ascii_lowercase())),
            _ => named,
        };
    }
    named.or(letter
        .map(|c| match parts.contains(&"shift") {
            true => c.to_ascii_uppercase(),
            false => c,
        })
        .map(VimKey::Char))
}

/// Whether fe-vim's reducer takes `key` in the mode `frame` stands for,
/// alone, after an operator, or in a visual mode.
fn vim_takes(key: VimKey, mode: &ContextFrame) -> bool {
    let visual = mode.flags.contains("vim.visual");
    let start = match visual {
        true => Pending::visual(),
        false => Pending::new(),
    };
    let pending = mode.flags.contains("vim.pending");
    let step = match pending {
        true => match step(start, VimKey::Char('d')) {
            Step::Wait(pending) => step(pending, key),
            other => other,
        },
        false => step(start, key),
    };
    // `Cancel` is any key an operator doesn't continue, vim's or not.
    matches!(step, Step::Wait(_) | Step::Commit(_))
}

/// The context `frames` stand in, for a binding of `command`.
fn in_vim(command: &str, mode: ContextFrame) -> ContextStack {
    match command {
        "git.commit_box.commit" => stack(vec![ContextFrame::new("git_panel"), mode]),
        "diff.next_change" | "diff.previous_change" => {
            stack(vec![ContextFrame::new("DiffView"), mode])
        }
        _ => stack(vec![mode]),
    }
}

#[test]
fn no_vim_keys() {
    for (os, generated, table) in platforms() {
        // Every binding the card adds: those of its commands. Blame
        // annotations' chord is blame_annotation_keys.rs's.
        let added: Vec<_> = table
            .bindings()
            .iter()
            .filter(|b| {
                let command = b.command.as_str();
                command.starts_with("git.") || command.starts_with("diff.")
            })
            .filter(|b| b.command.as_str() != "git.toggle_blame_annotations")
            // compare_saved_key.rs's chord.
            .filter(|b| b.command.as_str() != "diff.compare_saved")
            .map(|b| (b.keys.to_string(), b.command.as_str().to_owned()))
            .collect();
        assert_eq!(added.len(), card(os).len(), "{os:?} {added:?}");
        // The keys vim binds through the keymap are none of them.
        let vim_bound: Vec<_> = table
            .bindings()
            .iter()
            .filter(|b| matches!(b.command.as_str(), "vim.key" | "vim.move"))
            .map(|b| b.keys.to_string())
            .collect();
        for (keys, command) in &added {
            assert!(
                !vim_bound.contains(keys),
                "{os:?} {keys} is vim.key/vim.move's"
            );
            assert!(
                !["ctrl-y", "ctrl-e", "ctrl-d", "ctrl-u"].contains(&keys.as_str()),
                "{os:?} {keys}"
            );
            for keymap in [&generated, &table] {
                for mode in vim_modes() {
                    let name = format!("{os:?} {keys} in {mode:?}");
                    let got = run(keymap, keys, &in_vim(command, mode.clone()));
                    // Never vim's own binding...
                    assert!(
                        !got.as_deref().is_some_and(|c| c.starts_with("vim.")),
                        "{name} ran {got:?}"
                    );
                    // ...and where it applies, the reducer leaves its key alone.
                    if got.is_some() {
                        let key = vim_key(keys);
                        assert!(
                            !key.is_some_and(|key| vim_takes(key, &mode)),
                            "{name}: fe-vim takes {key:?}"
                        );
                    }
                }
            }
        }
        // Vim's scroll and page keys stay unbound by the card.
        for keys in ["ctrl-y", "ctrl-e", "ctrl-d", "ctrl-u"] {
            for mode in vim_modes() {
                let got = run(&generated, keys, &stack(vec![mode]));
                assert!(
                    !got.as_deref()
                        .is_some_and(|c| c.starts_with("git.") || c.starts_with("diff.")),
                    "{os:?} {keys} ran {got:?}"
                );
            }
        }
    }
}

fn shortcut(bar: &MenuBar, keymap: &Keymap, menu: &str, id: &str) -> Option<KeySequence> {
    bar.menu(menu)
        .and_then(|m| m.item(id))
        .and_then(|i| i.command())
        .unwrap_or_else(|| panic!("{menu} > {id}"))
        .shortcut(keymap)
}

#[test]
fn menus() {
    for (os, keymap, _) in platforms() {
        let (bar, diagnostics) = match os {
            Os::MacOs => fe_keymap::default_menus_macos(None),
            _ => fe_keymap::default_menus_linux(None),
        };
        assert_eq!(diagnostics, []);
        let bar = bar.unwrap();
        let key = |keys: &str| Some(KeySequence::parse(keys).unwrap());
        assert_eq!(
            shortcut(&bar, &keymap, "Go", "git.next_change"),
            key("alt-f3"),
            "{os:?}"
        );
        assert_eq!(
            shortcut(&bar, &keymap, "Go", "git.previous_change"),
            key("shift-alt-f3"),
            "{os:?}"
        );
        for (menu, id) in [
            ("View", "git.toggle_line_blame"),
            ("View", "diff.toggle_inline"),
            ("File", "diff.compare_file"),
        ] {
            assert_eq!(shortcut(&bar, &keymap, menu, id), None, "{os:?} {id}");
        }
    }
}
