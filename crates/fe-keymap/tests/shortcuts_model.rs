//! The Keyboard Shortcuts editor's model: a row per command with its
//! bindings, searched by command or keys, with clashing bindings marked.

use std::path::Path;

use fe_keymap::{
    CommandInfo, DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, KeySequence, Keymap,
    KeymapLayer, ShortcutIssue, ShortcutRow, ShortcutSource, ShortcutsModel,
};
use kiln_core::Os;

fn layer(name: &str, source: &str) -> KeymapLayer {
    let (layer, diagnostics) = KeymapLayer::parse_for(Path::new(name), source, None, Os::Linux);
    assert_eq!(diagnostics, [], "{name}");
    layer.unwrap()
}

fn command(id: &str, title: &str) -> CommandInfo {
    CommandInfo {
        id: id.into(),
        title: title.into(),
        context: None,
    }
}

fn build(layers: &[(&str, &str)], commands: &[CommandInfo]) -> ShortcutsModel {
    let layers: Vec<KeymapLayer> = layers
        .iter()
        .map(|(name, source)| layer(name, source))
        .collect();
    ShortcutsModel::from_layers(&layers, commands, Os::Linux)
}

fn row<'a>(rows: &'a [ShortcutRow], id: &str) -> &'a ShortcutRow {
    rows.iter()
        .find(|row| row.command == id)
        .unwrap_or_else(|| panic!("no row for {id}"))
}

/// Each row's command with its bindings' keys.
fn summary(rows: &[ShortcutRow]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let keys: Vec<String> = row
                .bindings
                .iter()
                .map(|binding| binding.keys.to_string())
                .collect();
            format!("{}: {}", row.command, keys.join(", "))
        })
        .collect()
}

fn keys(text: &str) -> KeySequence {
    KeySequence::parse_for(text, Os::Linux).unwrap()
}

const EDITING: &str = r#"
[[bindings]]
keys = "ctrl-s"
command = "workspace.save"

[[bindings]]
keys = "ctrl-k s"
command = "workspace.save"
context = "editor"

[[bindings]]
keys = "ctrl-k ctrl-0"
command = "editor.fold_all"
context = "editor && !vim"

[[bindings]]
keys = "alt-left"
command = "editor.move"
context = "editor"
args = { motion = "word_left" }
"#;

fn editing_commands() -> Vec<CommandInfo> {
    vec![
        command("workspace.save", "Save"),
        command("editor.fold_all", "Fold All"),
        command("editor.move", "Move Cursor"),
        command("workspace.close", "Close Window"),
        command("editor.unfold_all", "Unfold All"),
    ]
}

#[test]
fn row_per_command_with_all_bindings() {
    let model = build(&[("defaults.toml", EDITING)], &editing_commands());
    // In title order, one row each; save's two key sequences in its one row.
    assert_eq!(
        summary(model.rows()),
        [
            "workspace.close: ",
            "editor.fold_all: ctrl-k ctrl-0",
            "editor.move: alt-left",
            "workspace.save: ctrl-s, ctrl-k s",
            "editor.unfold_all: ",
        ]
    );
    let save = row(model.rows(), "workspace.save");
    assert_eq!(save.title, "Save");
    assert!(save.known);
    let labels: Vec<&str> = save.bindings.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Ctrl+S", "Ctrl+K S"]);
    let whens: Vec<Option<&str>> = save.bindings.iter().map(|b| b.when.as_deref()).collect();
    assert_eq!(whens, [None, Some("editor")]);
    let fold = row(model.rows(), "editor.fold_all");
    assert_eq!(fold.bindings[0].when.as_deref(), Some("editor && !vim"));
    assert_eq!(fold.bindings[0].location.line, 11);

    // Labels are the platform's.
    let mac = ShortcutsModel::new(
        &Keymap::from_layers([layer("defaults.toml", EDITING)]),
        &editing_commands(),
        Os::MacOs,
    );
    let fold = row(mac.rows(), "editor.fold_all");
    assert_eq!(fold.bindings[0].label, "⌃K ⌃0");
}

#[test]
fn unbound_commands_listed() {
    let model = build(&[("defaults.toml", EDITING)], &editing_commands());
    for id in ["workspace.close", "editor.unfold_all"] {
        let row = row(model.rows(), id);
        assert!(row.bindings.is_empty(), "{id}");
        assert!(row.known);
    }
    // A command bound but not passed in still gets a row, titled with its ID.
    let model = build(&[("defaults.toml", EDITING)], &[]);
    assert_eq!(model.rows().len(), 3);
    let fold = row(model.rows(), "editor.fold_all");
    assert_eq!(fold.title, "editor.fold_all");
    assert!(!fold.known);
    // A command's own context is carried over.
    let commands = [CommandInfo {
        context: Some("terminal".into()),
        ..command("terminal.clear", "Clear Terminal")
    }];
    let model = build(&[], &commands);
    assert_eq!(model.rows()[0].context.as_deref(), Some("terminal"));
}

const USER: &str = r#"
[[bindings]]
keys = "ctrl-w"
command = "workspace.close"

[[bindings]]
keys = "ctrl-k ctrl-j"
command = "editor.unfold_all"
"#;

#[test]
fn source_default_vs_user() {
    let model = build(
        &[("defaults.toml", EDITING), ("keymap.toml", USER)],
        &editing_commands(),
    );
    let save = row(model.rows(), "workspace.save");
    assert!(
        save.bindings
            .iter()
            .all(|b| b.source == ShortcutSource::Default)
    );
    let close = row(model.rows(), "workspace.close");
    assert_eq!(close.bindings[0].source, ShortcutSource::User);
    assert_eq!(close.bindings[0].location.file, Path::new("keymap.toml"));
    // Any layer after the first is the user's.
    let model = build(
        &[("defaults.toml", EDITING), ("a.toml", ""), ("b.toml", USER)],
        &editing_commands(),
    );
    let close = row(model.rows(), "workspace.close");
    assert_eq!(close.bindings[0].source, ShortcutSource::User);
}

#[test]
fn args_are_shown() {
    let model = build(&[("defaults.toml", EDITING)], &editing_commands());
    let moves = row(model.rows(), "editor.move");
    assert_eq!(
        moves.bindings[0].args.as_deref(),
        Some(r#"{"motion":"word_left"}"#)
    );
    let save = row(model.rows(), "workspace.save");
    assert_eq!(save.bindings[0].args, None);
}

fn search(model: &ShortcutsModel, query: &str, exact: bool) -> Vec<String> {
    summary(&model.search(query, exact))
}

#[test]
fn search_by_title() {
    let model = build(&[("defaults.toml", EDITING)], &editing_commands());
    assert_eq!(
        search(&model, "fold", false),
        ["editor.fold_all: ctrl-k ctrl-0", "editor.unfold_all: "]
    );
    // Ignoring case, every word.
    assert_eq!(search(&model, "UNFOLD all", false), ["editor.unfold_all: "]);
    assert_eq!(search(&model, "fold window", false), Vec::<String>::new());
    // The when-text matches, and keeps only that binding.
    assert_eq!(
        search(&model, "save editor", false),
        ["workspace.save: ctrl-k s"]
    );
    assert_eq!(
        search(&model, "!vim", false),
        ["editor.fold_all: ctrl-k ctrl-0"]
    );
    // An empty query lists everything.
    assert_eq!(search(&model, "  ", false), summary(model.rows()));
}

#[test]
fn search_by_id() {
    let model = build(&[("defaults.toml", EDITING)], &editing_commands());
    assert_eq!(
        search(&model, "workspace.", false),
        ["workspace.close: ", "workspace.save: ctrl-s, ctrl-k s"]
    );
    // `@command:` is that command alone.
    assert_eq!(
        search(&model, "@command:editor.fold_all", false),
        ["editor.fold_all: ctrl-k ctrl-0"]
    );
    assert_eq!(
        search(&model, "@command:editor.fold", false),
        Vec::<String>::new()
    );
}

#[test]
fn search_by_keystroke_prefix_and_exact() {
    let model = build(&[("defaults.toml", EDITING)], &editing_commands());
    let both = ["editor.fold_all: ctrl-k ctrl-0", "workspace.save: ctrl-k s"];
    // Keys typed plainly match bindings starting with them.
    assert_eq!(search(&model, "ctrl-k", false), both);
    assert_eq!(search(&model, "@keybinding:ctrl-k", false), both);
    // As the labels print them.
    assert_eq!(search(&model, "Ctrl+K", false), both);
    assert_eq!(search(&model, "ctrl+k ctrl+0", false), [both[0]]);
    // Exact: only bindings equal to them.
    assert_eq!(
        search(&model, "@keybinding:ctrl-k", true),
        Vec::<String>::new()
    );
    assert_eq!(
        search(&model, "@keybinding:ctrl-k s", true),
        ["workspace.save: ctrl-k s"]
    );
    // Quoted keys are exact.
    assert_eq!(
        search(&model, "\"ctrl-s\"", false),
        ["workspace.save: ctrl-s"]
    );
    assert_eq!(search(&model, "\"ctrl-k\"", false), Vec::<String>::new());
    // Explicit keys match only by keys: no binding starts with `s`, though
    // `Save` has one.
    assert_eq!(search(&model, "@keybinding:s", false), Vec::<String>::new());
    // Plain words that parse as keys match by title too.
    assert_eq!(search(&model, "alt-left", false), ["editor.move: alt-left"]);
    // Typed keys are exact too when asked, written either way.
    assert_eq!(search(&model, "ctrl-k", true), Vec::<String>::new());
    assert_eq!(search(&model, "Ctrl+K", true), Vec::<String>::new());
    assert_eq!(search(&model, "ctrl-s", true), ["workspace.save: ctrl-s"]);
    assert_eq!(search(&model, "Ctrl+S", true), ["workspace.save: ctrl-s"]);
    assert_eq!(
        search(&model, "ctrl-k s", true),
        ["workspace.save: ctrl-k s"]
    );
    assert_eq!(
        search(&model, "Ctrl+K S", true),
        ["workspace.save: ctrl-k s"]
    );
}

#[test]
fn search_by_displayed_key_label() {
    let source = r#"
[[bindings]]
keys = "escape"
command = "editor.cancel"

[[bindings]]
keys = "ctrl-escape"
command = "editor.dismiss"

[[bindings]]
keys = "cmd-k"
command = "terminal.clear"
"#;
    let commands = [
        command("editor.cancel", "Cancel"),
        command("editor.dismiss", "Dismiss"),
        command("terminal.clear", "Clear"),
    ];
    let model = build(&[("defaults.toml", source)], &commands);
    let labels: Vec<&str> = model
        .rows()
        .iter()
        .map(|row| row.bindings[0].label.as_str())
        .collect();
    assert_eq!(labels, ["Esc", "Super+K", "Ctrl+Esc"]);
    // Each searched by the label it shows.
    assert_eq!(search(&model, "Esc", false), ["editor.cancel: escape"]);
    assert_eq!(search(&model, "esc", true), ["editor.cancel: escape"]);
    assert_eq!(
        search(&model, "@keybinding:Esc", false),
        ["editor.cancel: escape"]
    );
    assert_eq!(search(&model, "\"Esc\"", false), ["editor.cancel: escape"]);
    assert_eq!(
        search(&model, "Ctrl+Esc", false),
        ["editor.dismiss: ctrl-escape"]
    );
    assert_eq!(
        search(&model, "ctrl-esc", false),
        ["editor.dismiss: ctrl-escape"]
    );
    assert_eq!(search(&model, "Super+K", false), ["terminal.clear: cmd-k"]);
}

#[test]
fn search_by_source() {
    let model = build(
        &[("defaults.toml", EDITING), ("keymap.toml", USER)],
        &editing_commands(),
    );
    assert_eq!(
        search(&model, "@source:user", false),
        [
            "workspace.close: ctrl-w",
            "editor.unfold_all: ctrl-k ctrl-j"
        ]
    );
    assert_eq!(
        search(&model, "@source:default save", false),
        ["workspace.save: ctrl-s, ctrl-k s"]
    );
    // Unbound commands have no source.
    let model = build(&[("defaults.toml", EDITING)], &editing_commands());
    assert_eq!(search(&model, "@source:user", false), Vec::<String>::new());
    assert_eq!(
        search(&model, "@source:default", false),
        [
            "editor.fold_all: ctrl-k ctrl-0",
            "editor.move: alt-left",
            "workspace.save: ctrl-s, ctrl-k s",
        ]
    );
    // With keys, before or after them.
    let both = ["editor.fold_all: ctrl-k ctrl-0", "workspace.save: ctrl-k s"];
    assert_eq!(
        search(&model, "@source:default @keybinding:ctrl-k", false),
        both
    );
    assert_eq!(
        search(&model, "@keybinding:ctrl-k @source:default", false),
        both
    );
    assert_eq!(
        search(&model, "\"ctrl-k s\" @source:default", false),
        [both[1]]
    );
    assert_eq!(
        search(&model, "@keybinding:ctrl-k @source:user", false),
        Vec::<String>::new()
    );
    // So with `@command:`.
    assert_eq!(
        search(&model, "@keybinding:ctrl-k @command:workspace.save", false),
        [both[1]]
    );
    assert_eq!(
        search(&model, "@command:workspace.save @keybinding:ctrl-k", false),
        [both[1]]
    );
}

#[test]
fn unparsable_keystroke_falls_back_to_text() {
    let model = build(&[("defaults.toml", EDITING)], &editing_commands());
    // Not keys: matched as words.
    assert_eq!(
        search(&model, "@keybinding:unfold", false),
        ["editor.unfold_all: "]
    );
    assert_eq!(
        search(&model, "\"close window\"", false),
        ["workspace.close: "]
    );
    assert_eq!(search(&model, "\"ctrl-nope\"", false), Vec::<String>::new());
}

/// Each marked binding as `command keys: issue`.
fn conflicts(model: &ShortcutsModel) -> Vec<String> {
    model
        .conflicts()
        .into_iter()
        .map(|(row, binding, issue)| format!("{} {}: {issue:?}", row.command, binding.keys))
        .collect()
}

#[test]
fn identical_keys_same_context_conflict() {
    let source = r#"
[[bindings]]
keys = "ctrl-d"
command = "editor.delete_line"

[[bindings]]
keys = "ctrl-d"
command = "editor.duplicate"
context = "editor"
"#;
    // Both hold in `[editor]`, where the deeper context wins.
    let model = build(&[("defaults.toml", source)], &[]);
    assert_eq!(
        conflicts(&model),
        [
            r#"editor.delete_line ctrl-d: Conflict { command: "editor.duplicate", wins: false }"#,
            r#"editor.duplicate ctrl-d: Conflict { command: "editor.delete_line", wins: true }"#,
        ]
    );
    // The same keys and context: the later replaces the earlier everywhere.
    let user = r#"
[[bindings]]
keys = "ctrl-d"
command = "editor.select_next"
context = "editor"
"#;
    let model = build(&[("defaults.toml", source), ("keymap.toml", user)], &[]);
    let duplicate = row(model.rows(), "editor.duplicate");
    assert_eq!(
        duplicate.bindings[0].issues,
        [ShortcutIssue::Hidden {
            by: Some("editor.select_next".into())
        }]
    );
    let next = row(model.rows(), "editor.select_next");
    assert!(next.bindings[0].issues.contains(&ShortcutIssue::Conflict {
        command: "editor.duplicate".into(),
        wins: true
    }));
    // Rebinding the same keys to the same command is no conflict.
    let same = r#"
[[bindings]]
keys = "ctrl-d"
command = "editor.duplicate"
context = "editor"
"#;
    let model = build(&[("defaults.toml", source), ("keymap.toml", same)], &[]);
    let duplicate = row(model.rows(), "editor.duplicate");
    assert_eq!(
        duplicate.bindings[0].issues,
        [ShortcutIssue::Hidden {
            by: Some("editor.duplicate".into())
        }]
    );
    assert!(
        !duplicate.bindings[1]
            .issues
            .iter()
            .any(|issue| matches!(issue, ShortcutIssue::Conflict { command, .. } if command == "editor.duplicate"))
    );
}

#[test]
fn different_contexts_do_not_conflict() {
    let source = r#"
[[bindings]]
keys = "ctrl-t"
command = "terminal.new"
context = "terminal"

[[bindings]]
keys = "ctrl-t"
command = "editor.transpose"
context = "editor"
"#;
    let model = build(&[("defaults.toml", source)], &[]);
    assert_eq!(conflicts(&model), Vec::<String>::new());
}

#[test]
fn unbind_hides_default() {
    let user = r#"
[[bindings]]
keys = "ctrl-k s"
context = "editor"
command = ""
"#;
    let model = build(
        &[("defaults.toml", EDITING), ("keymap.toml", user)],
        &editing_commands(),
    );
    let save = row(model.rows(), "workspace.save");
    // Still listed, in the order read, from the defaults, but hidden.
    assert_eq!(save.bindings.len(), 2);
    assert_eq!(save.bindings[1].keys, keys("ctrl-k s"));
    assert_eq!(save.bindings[1].source, ShortcutSource::Default);
    assert_eq!(
        save.bindings[1].issues,
        [ShortcutIssue::Hidden { by: None }]
    );
    assert_eq!(save.bindings[0].issues, []);
    assert_eq!(
        conflicts(&model),
        ["workspace.save ctrl-k s: Hidden { by: None }"]
    );
    // An unbind only hides what's before it.
    let rebound = r#"
[[bindings]]
keys = "ctrl-k s"
context = "editor"
command = ""

[[bindings]]
keys = "ctrl-k s"
context = "editor"
command = "workspace.save_all"
"#;
    let model = build(
        &[("defaults.toml", EDITING), ("keymap.toml", rebound)],
        &editing_commands(),
    );
    assert_eq!(
        conflicts(&model),
        ["workspace.save ctrl-k s: Hidden { by: None }"]
    );
    // A keymap alone no longer has the removed binding.
    let keymap = Keymap::from_layers([layer("defaults.toml", EDITING), layer("keymap.toml", user)]);
    let model = ShortcutsModel::new(&keymap, &editing_commands(), Os::Linux);
    assert_eq!(
        summary(&model.search("@command:workspace.save", false)),
        ["workspace.save: ctrl-s"]
    );
}

#[test]
fn prefix_shadowing() {
    let source = r#"
[[bindings]]
keys = "ctrl-k"
command = "editor.cut_to_end"
context = "editor"

[[bindings]]
keys = "ctrl-k ctrl-0"
command = "editor.fold_all"
context = "editor"
"#;
    let model = build(&[("defaults.toml", source)], &[]);
    let cut = row(model.rows(), "editor.cut_to_end");
    assert_eq!(
        cut.bindings[0].issues,
        [ShortcutIssue::Shadowed {
            by: "editor.fold_all".into(),
            keys: keys("ctrl-k ctrl-0"),
        }]
    );
    // Issues the host finds in more contexts replace the report's.
    let model = model.with_issues(&[]);
    assert_eq!(conflicts(&model), Vec::<String>::new());
}

#[test]
fn every_default_binding_is_listed() {
    for (source, os) in [(DEFAULT_LINUX, Os::Linux), (DEFAULT_MACOS, Os::MacOs)] {
        let (layer, diagnostics) =
            KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), source, None, os);
        assert_eq!(diagnostics, [], "{os:?}");
        let layer = layer.unwrap();
        let keymap = Keymap::from_layers([&layer]);
        assert!(keymap.bindings().len() > 100, "{os:?}");
        let commands = [command("workspace.save", "Save")];
        for model in [
            ShortcutsModel::new(&keymap, &commands, os),
            ShortcutsModel::from_layers(std::slice::from_ref(&layer), &commands, os),
        ] {
            let listed: Vec<_> = model
                .rows()
                .iter()
                .flat_map(|row| {
                    row.bindings
                        .iter()
                        .map(move |binding| (row.command.clone(), binding))
                })
                .collect();
            assert_eq!(listed.len(), keymap.bindings().len(), "{os:?}");
            for binding in keymap.bindings() {
                let rows = listed
                    .iter()
                    .filter(|(command, listed)| {
                        command == binding.command.as_str()
                            && listed.keys == binding.keys
                            && listed.location == binding.location
                    })
                    .count();
                assert_eq!(rows, 1, "{os:?}: {} at {}", binding.keys, binding.location);
            }
            // One row per command, in title order.
            let ids: Vec<&str> = model
                .rows()
                .iter()
                .map(|row| row.command.as_str())
                .collect();
            let mut unique = ids.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), ids.len(), "{os:?}");
            let titles: Vec<String> = model
                .rows()
                .iter()
                .map(|row| row.title.to_lowercase())
                .collect();
            assert!(titles.is_sorted(), "{os:?}");
        }
    }
}
