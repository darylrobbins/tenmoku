//! The day-one features' keys and menu items: find and replace, replace in
//! files, the tree's file operations, Problems, Open Recent and the
//! notifications. None takes a key vim reads in an editor in its normal,
//! visual, operator-pending or insert mode, but nvim's `]d` and `[d`; the
//! tree and the lists have vim's keys with the `vim` flag.

use std::path::Path;

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{
    Binding, DEFAULT, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, Key, KeyOutcome,
    KeySequence, Keymap, KeymapLayer, Keystroke, Matcher, MenuBar,
};
use fe_vim::{Pending, Step, VimKey, step};
use kiln_core::Os;

fn keymap(source: &str, os: Os) -> Keymap {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
    assert_eq!(diagnostics, [], "{os:?}");
    Keymap::from_layers(vec![layer.unwrap()])
}

/// Each platform's generated keymap, then the one table as on it.
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

/// The workbench, then `frames`.
fn stack(frames: Vec<ContextFrame>) -> ContextStack {
    std::iter::once(ContextFrame::new("workbench"))
        .chain(frames)
        .collect()
}

fn frame(name: &str, vim: bool) -> ContextFrame {
    let frame = ContextFrame::new(name);
    match vim {
        true => frame.flag("vim"),
        false => frame,
    }
}

/// An editor with vim off.
fn plain_editor() -> ContextStack {
    stack(vec![ContextFrame::new("editor")])
}

/// An editor with vim on in `mode`.
fn vim_editor(mode: &str) -> ContextStack {
    stack(vec![vim_mode(mode)])
}

/// The editor's frame with vim on in `mode`, as `VimState::stamp` puts it:
/// `vim.visual` in the visual modes, `vim.pending` while an operator waits.
fn vim_mode(mode: &str) -> ContextFrame {
    let frame = ContextFrame::new("editor")
        .flag("vim.enabled")
        .set("vim.mode", mode);
    match mode {
        "visual" | "visual_line" | "visual_block" => frame.flag("vim.visual"),
        "operator_pending" => frame
            .flag("vim.pending")
            .set("vim.pending_kind", "operator"),
        _ => frame,
    }
}

fn find_input() -> ContextStack {
    stack(vec![
        ContextFrame::new("find_bar"),
        ContextFrame::new("text_input"),
    ])
}

fn replace_input() -> ContextStack {
    stack(vec![
        ContextFrame::new("find_bar"),
        ContextFrame::new("replace_input"),
        ContextFrame::new("text_input"),
    ])
}

fn search_input() -> ContextStack {
    stack(vec![
        ContextFrame::new("search_panel"),
        ContextFrame::new("text_input"),
    ])
}

fn search_results(vim: bool) -> ContextStack {
    stack(vec![
        ContextFrame::new("search_panel"),
        frame("search_results", vim),
    ])
}

fn tree(vim: bool) -> ContextStack {
    stack(vec![frame("project_panel", vim)])
}

fn problems(vim: bool) -> ContextStack {
    stack(vec![frame("problems_panel", vim)])
}

/// The command `keys` run in `stack`, pressed one after another, if any.
fn run(keymap: &Keymap, keys: &str, stack: &ContextStack) -> Option<String> {
    let mut matcher = Matcher::new(keymap.clone());
    let keys = KeySequence::parse(keys).unwrap();
    let (last, first) = keys.keystrokes().split_last().unwrap();
    for keystroke in first {
        if matcher.push(*keystroke, stack) != KeyOutcome::Pending {
            return None;
        }
    }
    match matcher.push(*last, stack) {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.as_str().to_owned()),
        _ => None,
    }
}

/// The tree's keys with the `vim` flag: (keys, command).
const TREE_VIM: &[(&str, &str)] = &[
    ("j", "project_panel.select_next"),
    ("k", "project_panel.select_prev"),
    ("h", "project_panel.collapse_or_parent"),
    ("l", "project_panel.open"),
    ("g g", "project_panel.select_first"),
    ("shift-g", "project_panel.select_last"),
    ("a", "project_panel.new_file"),
    ("%", "project_panel.new_file"),
    ("r", "project_panel.rename"),
    ("shift-r", "project_panel.rename"),
    ("d", "project_panel.delete"),
    ("shift-d", "project_panel.delete"),
    ("shift-y", "project_panel.copy_relative_path"),
    ("g y", "project_panel.copy_path"),
];

/// Every binding the card adds on `os`: (keys, command, a stack it runs in).
fn card(os: Os) -> Vec<(&'static str, &'static str, ContextStack)> {
    let mac = os == Os::MacOs;
    let keys = |mac_keys, other| if mac { mac_keys } else { other };
    let mut card = vec![
        (
            keys("cmd-alt-f", "ctrl-alt-f"),
            "find.show_replace",
            plain_editor(),
        ),
        // cmd-shift-1 as GPUI reports it on macOS: shift cleared, the key
        // what it types.
        (keys("cmd-!", "ctrl-!"), "find.replace_next", find_input()),
        (
            keys("cmd-alt-enter", "ctrl-alt-enter"),
            "find.replace_all",
            find_input(),
        ),
        ("enter", "find.replace_next", replace_input()),
        (
            keys("cmd-shift-h", "ctrl-shift-h"),
            "search.show_replace",
            plain_editor(),
        ),
        (
            keys("cmd-alt-enter", "ctrl-alt-enter"),
            "search.replace_all",
            search_input(),
        ),
        ("up", "search.select_previous", search_results(false)),
        ("down", "search.select_next", search_results(false)),
        ("enter", "search.open_selected", search_results(false)),
        ("k", "search.select_previous", search_results(true)),
        ("j", "search.select_next", search_results(true)),
        ("f2", "project_panel.rename", tree(false)),
        (
            keys("cmd-backspace", "ctrl-backspace"),
            "project_panel.delete",
            tree(false),
        ),
        (
            keys("cmd-shift-m", "ctrl-shift-m"),
            "problems.show",
            plain_editor(),
        ),
        ("f8", "diagnostics.next", plain_editor()),
        ("shift-f8", "diagnostics.previous", plain_editor()),
        ("] d", "diagnostics.next", vim_editor("normal")),
        ("[ d", "diagnostics.previous", vim_editor("normal")),
        ("up", "problems.select_previous", problems(false)),
        ("down", "problems.select_next", problems(false)),
        ("enter", "problems.open", problems(false)),
        ("k", "problems.select_previous", problems(true)),
        ("j", "problems.select_next", problems(true)),
        ("ctrl-r", "workbench.open_recent", plain_editor()),
        (
            keys("cmd-k escape", "ctrl-k escape"),
            "toast.dismiss_all",
            plain_editor(),
        ),
    ];
    card.extend(
        TREE_VIM
            .iter()
            .map(|&(keys, command)| (keys, command, tree(true))),
    );
    card
}

/// Those of the card's commands' bindings that were there before it: the
/// tree's arrows and `enter`.
fn before_card(keys: &str, command: &str) -> bool {
    command.starts_with("project_panel.")
        && ["up", "down", "left", "right", "enter"].contains(&keys)
}

#[test]
fn resolves() {
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
        // The workbench's keys outside an editor too, with vim on or off.
        let mac = os == Os::MacOs;
        let outside = [
            (
                if mac { "cmd-alt-f" } else { "ctrl-alt-f" },
                "find.show_replace",
            ),
            (
                if mac { "cmd-shift-h" } else { "ctrl-shift-h" },
                "search.show_replace",
            ),
            (
                if mac { "cmd-shift-m" } else { "ctrl-shift-m" },
                "problems.show",
            ),
            ("f8", "diagnostics.next"),
            ("ctrl-r", "workbench.open_recent"),
            (
                if mac { "cmd-k escape" } else { "ctrl-k escape" },
                "toast.dismiss_all",
            ),
        ];
        for (keys, command) in outside {
            for stack in [tree(false), tree(true), problems(true), stack(vec![])] {
                assert_eq!(
                    run(&generated, keys, &stack).as_deref(),
                    Some(command),
                    "{os:?} {keys} in {stack:?}"
                );
            }
        }
        // Replace from the pane's find bar too.
        let replace = if mac { "cmd-alt-f" } else { "ctrl-alt-f" };
        assert_eq!(
            run(&generated, replace, &find_input()).as_deref(),
            Some("find.show_replace")
        );
        // `enter` in the find input still finds the next match, and in the
        // tree and the lists still opens.
        for (stack, command) in [
            (find_input(), "find.next"),
            (tree(true), "project_panel.open"),
            (search_results(true), "search.open_selected"),
        ] {
            assert_eq!(run(&generated, "enter", &stack).as_deref(), Some(command));
        }
        // The terminal keeps ctrl-r for the shell.
        let terminal = stack(vec![ContextFrame::new("terminal")]);
        assert_eq!(run(&generated, "ctrl-r", &terminal), None, "{os:?}");
    }
}

/// The modes whose keys are vim's, as the editor's frame shows them.
const VIM_MODES: [&str; 6] = [
    "normal",
    "operator_pending",
    "visual",
    "visual_line",
    "visual_block",
    "insert",
];

/// The key the editor hands vim for `keystroke`, as fe-gpui's `vim_key`
/// reads it: none with cmd; `ctrl-[` is `<Esc>`, `ctrl-<letter>` is
/// `<C-letter>` (shift and alt aside); the named keys are those keys, and a
/// typed character is itself. A function key, or an alt chord without
/// ctrl, types nothing, so none.
fn vim_key(keystroke: &Keystroke) -> Option<VimKey> {
    let held = &keystroke.modifiers;
    if held.cmd || (held.alt && !held.ctrl) {
        return None;
    }
    let named = match keystroke.key {
        Key::Escape => Some(VimKey::Esc),
        Key::Enter => Some(VimKey::Enter),
        Key::Backspace => Some(VimKey::Backspace),
        Key::Tab => Some(VimKey::Tab),
        Key::Left => Some(VimKey::Left),
        Key::Right => Some(VimKey::Right),
        Key::Up => Some(VimKey::Up),
        Key::Down => Some(VimKey::Down),
        _ => None,
    };
    match keystroke.key {
        Key::Char('[') if held.ctrl => Some(VimKey::Esc),
        Key::Char(c) if held.ctrl && c.is_ascii_alphabetic() => Some(VimKey::Ctrl(c)),
        Key::Char(c) if !held.ctrl => Some(VimKey::Char(match held.shift {
            true => c.to_ascii_uppercase(),
            false => c,
        })),
        Key::Space if !held.ctrl => Some(VimKey::Char(' ')),
        _ => named,
    }
}

/// Whether vim reads `key` in `mode`: fe-vim's reducer, alone, after an
/// operator or in a visual mode; in insert mode, any key that types or that
/// nvim's insert mode reads (its `<C-..>` keys, `<Esc>`, `<CR>`, ...).
fn vim_takes(key: VimKey, mode: &str) -> bool {
    let start = match mode {
        "insert" => return true,
        "visual" | "visual_line" | "visual_block" => Pending::visual(),
        _ => Pending::new(),
    };
    let step = match mode {
        "operator_pending" => match step(start, VimKey::Char('d')) {
            Step::Wait(pending) => step(pending, key),
            other => other,
        },
        _ => step(start, key),
    };
    // `Cancel` is any key an operator doesn't continue, vim's or not.
    matches!(step, Step::Wait(_) | Step::Commit(_))
}

/// The card's commands' bindings in `keymap`.
fn added(keymap: &Keymap, os: Os) -> Vec<&Binding> {
    let commands: Vec<_> = card(os)
        .into_iter()
        .map(|(_, command, _)| command)
        .collect();
    keymap
        .bindings()
        .iter()
        .filter(|b| commands.contains(&b.command.as_str()))
        .filter(|b| !before_card(&b.keys.to_string(), b.command.as_str()))
        .collect()
}

#[test]
fn vim_safe() {
    for (os, generated, table) in platforms() {
        for keymap in [&generated, &table] {
            let added = added(keymap, os);
            // The card lists every binding of its commands.
            let card: Vec<_> = card(os)
                .into_iter()
                .map(|(keys, command, _)| (KeySequence::parse(keys).unwrap(), command))
                .collect();
            let found: Vec<_> = added
                .iter()
                .map(|b| (b.keys.clone(), b.command.as_str()))
                .collect();
            assert_eq!(found.len(), card.len(), "{os:?} {found:?}");
            assert!(card.iter().all(|entry| found.contains(entry)), "{os:?}");

            for binding in added {
                let keys = binding.keys.to_string();
                let nvim_d = keys == "] d" || keys == "[ d";
                let first = &binding.keys.keystrokes()[0];
                for mode in VIM_MODES {
                    // In an editor of its own and inside each panel's frame,
                    // where an editor could be.
                    let stack = vim_editor(mode);
                    let name = format!("{os:?} {keys} ({}) in {mode}", binding.command);
                    if binding.depth(&stack).is_none() {
                        continue;
                    }
                    if nvim_d {
                        assert_eq!(mode, "normal", "{name}");
                        continue;
                    }
                    let key = vim_key(first);
                    assert!(
                        !key.is_some_and(|key| vim_takes(key, mode)),
                        "{name}: vim reads {key:?}"
                    );
                }
                // `]d` and `[d` wait for nothing else.
                if nvim_d {
                    let pending = stack(vec![
                        vim_mode("normal")
                            .flag("vim.pending")
                            .set("vim.pending_kind", "count"),
                    ]);
                    assert_eq!(binding.depth(&pending), None, "{os:?} {keys}");
                }
            }
        }

        // ctrl-r: Open Recent with vim off or outside the editor, vim's in
        // an editor with vim on, in every mode.
        for keymap in [&generated, &table] {
            for stack in [plain_editor(), tree(true), problems(true), stack(vec![])] {
                assert_eq!(
                    run(keymap, "ctrl-r", &stack).as_deref(),
                    Some("workbench.open_recent"),
                    "{os:?} {stack:?}"
                );
            }
            for mode in VIM_MODES.into_iter().chain(["replace"]) {
                assert_eq!(
                    run(keymap, "ctrl-r", &vim_editor(mode)),
                    None,
                    "{os:?} {mode}"
                );
            }
        }
        // The vim modes' own keys stay theirs: `]` and `[` but before `d`.
        for mode in VIM_MODES {
            for keys in ["] ]", "[ [", "] )", "[ (", "] }"] {
                assert_eq!(
                    run(&generated, keys, &vim_editor(mode)),
                    None,
                    "{mode} {keys}"
                );
            }
        }
    }
}

#[test]
fn tree_vim() {
    for (os, generated, table) in platforms() {
        for keymap in [&generated, &table] {
            for &(keys, command) in TREE_VIM {
                assert_eq!(
                    run(keymap, keys, &tree(true)).as_deref(),
                    Some(command),
                    "{os:?} {keys}"
                );
                // Without the flag no letter key runs a tree command, nor `%`.
                let got = run(keymap, keys, &tree(false));
                assert!(
                    !got.as_deref()
                        .is_some_and(|c| c.starts_with("project_panel.")),
                    "{os:?} {keys} ran {got:?} with vim off"
                );
            }
            // Nor do the lists' `j` and `k`.
            for stack in [search_results(false), problems(false)] {
                for keys in ["j", "k"] {
                    assert_eq!(run(keymap, keys, &stack), None, "{os:?} {keys}");
                }
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
        let mac = os == Os::MacOs;
        let key = |mac_keys: &str, other: &str| {
            Some(KeySequence::parse(if mac { mac_keys } else { other }).unwrap())
        };
        // macOS shows no key for Open Recent: see `native_safe`.
        let open_recent = (!mac).then(|| KeySequence::parse("ctrl-r").unwrap());
        for (menu, id, title, keys) in [
            ("File", "workbench.open_recent", "Open Recent…", open_recent),
            (
                "Edit",
                "find.show_replace",
                "Replace",
                key("cmd-alt-f", "ctrl-alt-f"),
            ),
            (
                "Edit",
                "search.show_replace",
                "Replace in Files",
                key("cmd-shift-h", "ctrl-shift-h"),
            ),
            (
                "View",
                "problems.show",
                "Problems",
                key("cmd-shift-m", "ctrl-shift-m"),
            ),
            ("Go", "diagnostics.next", "Next Problem", key("f8", "f8")),
            (
                "Go",
                "diagnostics.previous",
                "Previous Problem",
                key("shift-f8", "shift-f8"),
            ),
        ] {
            let item = bar.menu(menu).and_then(|m| m.item(id));
            let item = item.unwrap_or_else(|| panic!("{os:?} {menu} > {id}"));
            assert_eq!(item.command().unwrap().title, title);
            assert_eq!(shortcut(&bar, &keymap, menu, id), keys, "{os:?} {id}");
        }
        // Each after its neighbour.
        let ids = |menu: &str| -> Vec<String> {
            let menu = bar.menu(menu).unwrap();
            let ids = menu
                .items
                .iter()
                .map(|item| item.id().unwrap_or("-").to_owned());
            ids.collect()
        };
        let after = |menu: &str, first: &str, second: &str| {
            let ids = ids(menu);
            let at = |id: &str| ids.iter().position(|i| i == id).unwrap();
            assert_eq!(at(first) + 1, at(second), "{os:?} {menu}: {ids:?}");
        };
        after("File", "workbench.open_file", "workbench.open_recent");
        after("Edit", "find.show", "find.show_replace");
        after("Edit", "search.show", "search.show_replace");
        after("Go", "diagnostics.next", "diagnostics.previous");
    }
}

/// On macOS a menu item's shortcut is an AppKit key equivalent, which runs
/// the item when the app leaves the key unhandled, as a vim editor in insert
/// or replace mode leaves a key the keymap gives it, such as ctrl-r. So
/// each shortcut the card's items show there runs their command through the
/// keymap first, in an editor whatever vim's mode.
#[test]
fn native_safe() {
    let keymap = keymap(DEFAULT_MACOS, Os::MacOs);
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let bar = bar.unwrap();
    let items = [
        ("File", "workbench.open_recent"),
        ("Edit", "find.show_replace"),
        ("Edit", "search.show_replace"),
        ("View", "problems.show"),
        ("Go", "diagnostics.next"),
        ("Go", "diagnostics.previous"),
    ];
    let mut shown = 0;
    for (menu, id) in items {
        let Some(keys) = shortcut(&bar, &keymap, menu, id) else {
            continue;
        };
        shown += 1;
        let keys = keys.to_string();
        for mode in VIM_MODES.into_iter().chain(["replace"]) {
            assert_eq!(
                run(&keymap, &keys, &vim_editor(mode)).as_deref(),
                Some(id),
                "{menu} > {id}: {keys} in {mode}"
            );
        }
    }
    assert_eq!(shown, items.len() - 1, "all but Open Recent");
    assert_eq!(
        shortcut(&bar, &keymap, "File", "workbench.open_recent"),
        None
    );
}
