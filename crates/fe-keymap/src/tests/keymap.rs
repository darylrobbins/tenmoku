use std::path::Path;

use fe_command::CommandId;
use fe_config::Severity;

use super::{keymap, keys, keystroke, registry, stack, write};
use crate::{KeyOutcome, KeymapIssue, KeymapStore, Location, Matcher};

/// What pressing `keys` alone runs in `frames`, or `None`.
fn runs(matcher: &mut Matcher, keys: &str, frames: &[&str]) -> Option<String> {
    let context = stack(frames);
    let mut outcome = KeyOutcome::Unmatched(Vec::new());
    for key in keys.split_whitespace() {
        outcome = matcher.push(keystroke(key), &context);
    }
    match outcome {
        KeyOutcome::Dispatch(invocation) => Some(invocation.command.to_string()),
        _ => {
            matcher.flush();
            None
        }
    }
}

fn id(command: &str) -> CommandId {
    CommandId::parse(command).unwrap()
}

#[test]
fn layers_and_unbinding() {
    let defaults = "\
[[bindings]]
keys = \"cmd-shift-s\"
command = \"workspace.save_as\"

[[bindings]]
keys = \"alt-z\"
command = \"editor.toggle_soft_wrap\"
context = \"editor\"

[[bindings]]
keys = \"alt-z\"
command = \"terminal.zoom\"
context = \"terminal\"
";
    // The same keys and contexts, written differently.
    let user = "\
[[bindings]]
keys = \"shift-cmd-s\"
command = \"workspace.save_all\"

[[bindings]]
keys = \"Alt-z\"
context = \"editor\"
command = \"\"
";
    let keymap = keymap(defaults, user);
    let mut matcher = Matcher::new(keymap.clone());

    // The user's binding replaces the default's.
    let save = runs(&mut matcher, "cmd-shift-s", &["editor"]);
    assert_eq!(save.as_deref(), Some("workspace.save_all"));
    assert!(keymap.bindings_for(&id("workspace.save_as")).is_empty());
    assert_eq!(keymap.bindings_for(&id("workspace.save_all")).len(), 1);

    // The unbinding removes alt-z in the editor, but not in the terminal.
    assert_eq!(runs(&mut matcher, "alt-z", &["editor"]), None);
    assert!(
        keymap
            .bindings_for(&id("editor.toggle_soft_wrap"))
            .is_empty()
    );
    let zoom = runs(&mut matcher, "alt-z", &["terminal"]);
    assert_eq!(zoom.as_deref(), Some("terminal.zoom"));

    // Each binding keeps its file and line.
    let at = |binding: &crate::Binding| (binding.location.file.clone(), binding.location.line);
    let located: Vec<_> = keymap
        .bindings()
        .iter()
        .map(|binding| (binding.command.to_string(), at(binding)))
        .collect();
    assert_eq!(
        located,
        [
            ("workspace.save_as".into(), ("default.toml".into(), 1)),
            ("terminal.zoom".into(), ("default.toml".into(), 10)),
            ("workspace.save_all".into(), ("user.toml".into(), 1)),
        ]
    );
    let user_binding = &keymap.bindings()[2];
    assert_eq!(user_binding.layer, 1);
    assert_eq!(user_binding.keys, keys("cmd-shift-s"));
}

#[test]
fn entry_errors_keep_last_good() {
    let dir = tempfile::tempdir().unwrap();
    let good = "\
[[bindings]]
keys = \"cmd-k cmd-0\"
command = \"editor.fold_all\"
context = \"editor\"

[[bindings]]
keys = \"alt-left\"
command = \"editor.move\"
args = { motion = \"word_left\", extend = true }
context = \"editor\"
";
    let path = write(dir.path(), "keymap.toml", good);
    let mut store = KeymapStore::load("", Some(path.clone()), Some(Box::new(registry())));
    assert!(store.diagnostics().is_empty(), "{:?}", store.diagnostics());
    let loaded = store.keymap().clone();
    assert_eq!(loaded.bindings().len(), 2);
    assert_eq!(loaded.bindings()[1].args["motion"], "word_left");

    // Each error is at its token, and the last good layer stays.
    let fold = "[[bindings]]\nkeys = \"cmd-s\"\ncommand = \"editor.save\"\n\n[[bindings]]\n";
    for (entry, key, line, column) in [
        (
            "keys = \"cmd-k cmd-bogus\"\ncommand = \"editor.fold_all\"",
            "bindings[1].keys",
            6,
            19,
        ),
        (
            "keys = \"cmd-j\"\ncommand = \"editor.fold_all\"\ncontext = \"editor &&\"",
            "bindings[1].context",
            8,
            21,
        ),
        (
            "keys = \"cmd-j\"\ncommand = \"Editor.fold_all\"",
            "bindings[1].command",
            7,
            11,
        ),
        (
            "keys = \"cmd-j\"\ncommand = \"fold_all\"",
            "bindings[1].command",
            7,
            11,
        ),
        (
            // Args the registry rejects: of the wrong type, or for a
            // command that takes none.
            "keys = \"cmd-j\"\ncommand = \"editor.move\"\nargs = { motion = 3 }",
            "bindings[1].args",
            8,
            8,
        ),
        (
            "keys = \"cmd-j\"\ncommand = \"editor.wide_move\"\nargs = { motion = 3 }",
            "bindings[1].args",
            8,
            8,
        ),
        (
            "keys = \"cmd-j\"\ncommand = \"editor.save\"\nargs = { now = true }",
            "bindings[1].args",
            8,
            8,
        ),
    ] {
        let text = format!("{fold}{entry}\n");
        write(dir.path(), "keymap.toml", &text);
        let event = store
            .reload(std::slice::from_ref(&path))
            .expect("new diagnostics");
        let errors: Vec<_> = event.diagnostics.iter().filter(|d| d.is_error()).collect();
        assert_eq!(errors.len(), 1, "{text}\n{errors:?}");
        let error = errors[0];
        assert_eq!(error.file, path);
        assert_eq!(error.key.as_deref(), Some(key), "{text}\n{error}");
        assert_eq!(
            (error.line, error.column),
            (line, column),
            "{text}\n{error}"
        );
        assert_eq!(event.keymap, loaded, "the last good layer stays: {text}");
        assert_eq!(*store.keymap(), loaded);
    }

    // An unknown command is a warning, and only its binding is dropped.
    let text = format!("{fold}keys = \"cmd-j\"\ncommand = \"editor.unknown\"\n");
    write(dir.path(), "keymap.toml", &text);
    let event = store.reload(std::slice::from_ref(&path)).unwrap();
    let [warning] = event.diagnostics.as_slice() else {
        panic!("{:?}", event.diagnostics);
    };
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(warning.key.as_deref(), Some("bindings[1].command"));
    assert_eq!((warning.line, warning.column), (7, 11));
    let commands: Vec<_> = event
        .keymap
        .bindings()
        .iter()
        .map(|binding| binding.command.to_string())
        .collect();
    assert_eq!(commands, ["editor.save"]);
}

#[test]
fn report_finds_overrides_shadows_and_prefix_delays() {
    let defaults = "\
[[bindings]]
keys = \"cmd-s\"
command = \"workspace.save\"

[[bindings]]
keys = \"cmd-k\"
command = \"workspace.open_recent\"

[[bindings]]
keys = \"cmd-k cmd-0\"
command = \"editor.fold_all\"
context = \"editor\"

[[bindings]]
keys = \"cmd-d\"
command = \"editor.select_next\"
context = \"editor\"

[[bindings]]
keys = \"cmd-d\"
command = \"terminal.split\"
context = \"terminal\"
";
    let user = "\
[[bindings]]
keys = \"cmd-d\"
command = \"workspace.duplicate\"

[[bindings]]
keys = \"cmd-s\"
command = \"workspace.save_all\"
";
    let keymap = keymap(defaults, user);
    let at = |file: &str, line| Location {
        file: file.into(),
        line,
        column: 1,
    };
    let issues: Vec<_> = keymap
        .report()
        .into_iter()
        .map(|issue| match issue {
            KeymapIssue::Overridden { winner, loser } => {
                ("overridden", winner.location, loser.location)
            }
            // The terminal binding doesn't hold in [editor].
            KeymapIssue::Shadowed {
                winner,
                loser,
                context,
            } => {
                assert_eq!(context, stack(&["editor"]));
                ("shadowed", winner.location, loser.location)
            }
            KeymapIssue::PrefixDelay { prefix, longer, .. } => {
                ("prefix delay", prefix.location, longer.location)
            }
        })
        .collect();
    assert_eq!(
        issues,
        [
            ("overridden", at("user.toml", 5), at("default.toml", 1)),
            ("prefix delay", at("default.toml", 5), at("default.toml", 9)),
            ("shadowed", at("default.toml", 14), at("user.toml", 1)),
        ]
    );
    // Each names both locations when printed.
    for issue in keymap.report() {
        let text = issue.to_string();
        assert_eq!(text.matches(".toml:").count(), 2, "{text}");
    }

    // Distinct keys, or contexts that never hold together, report nothing.
    let clean = "\
[[bindings]]
keys = \"cmd-s\"
command = \"workspace.save\"

[[bindings]]
keys = \"cmd-k cmd-0\"
command = \"editor.fold_all\"
context = \"editor\"

[[bindings]]
keys = \"cmd-d\"
command = \"editor.select_next\"
context = \"editor\"

[[bindings]]
keys = \"cmd-d\"
command = \"terminal.split\"
context = \"terminal\"
";
    assert_eq!(keymap_issues(clean), []);

    // A prefix that's itself shadowed in a context isn't delayed there.
    let shadowed_prefix = "\
[[bindings]]
keys = \"cmd-k\"
command = \"workspace.open_recent\"

[[bindings]]
keys = \"cmd-k\"
command = \"editor.kill\"
context = \"editor\"

[[bindings]]
keys = \"cmd-k cmd-0\"
command = \"editor.fold_all\"
context = \"editor\"
";
    let kinds: Vec<_> = keymap_issues(shadowed_prefix)
        .into_iter()
        .map(|issue| match issue {
            KeymapIssue::Overridden { .. } => panic!("{issue}"),
            KeymapIssue::Shadowed { winner, .. } => ("shadowed", winner.command.to_string()),
            KeymapIssue::PrefixDelay { prefix, .. } => ("prefix delay", prefix.command.to_string()),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("shadowed", "editor.kill".to_string()),
            ("prefix delay", "editor.kill".to_string()),
        ]
    );
}

fn keymap_issues(source: &str) -> Vec<KeymapIssue> {
    super::keymap(source, "").report()
}

#[test]
fn a_missing_user_file_is_no_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("keymap.toml");
    let defaults = "[[bindings]]\nkeys = \"cmd-s\"\ncommand = \"workspace.save\"\n";
    let (keymap, diagnostics) = crate::load_keymap(defaults, Some(&missing));
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(keymap.bindings().len(), 1);
    assert_eq!(
        keymap.bindings()[0].location.file,
        Path::new(crate::DEFAULT_KEYMAP_PATH)
    );
}
