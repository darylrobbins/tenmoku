//! Editing the user's keymap.toml for the Keyboard Shortcuts editor: add,
//! remove, change and reset bindings as text, keeping comments and layout,
//! and writing the file atomically.

use std::{fs, path::Path};

use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DefaultKeymap, KEYMAP_HEADER, KeySequence, Keymap, KeymapEdit,
    KeymapEditError, KeymapLayer, edit_keymap, edit_keymap_on, write_keymap_hooked,
    write_keymap_on,
};
use kiln_core::Os;

const DEFAULTS: &str = r#"
[[bindings]]
keys = "ctrl-s"
command = "editor.save"
context = "editor"

[[bindings]]
keys = "ctrl-k ctrl-0"
command = "editor.fold_all"
context = "editor"

[[bindings]]
keys = "ctrl-shift-s"
command = "editor.save"
context = "editor"

[[bindings]]
keys = "ctrl-left"
command = "editor.move"
context = "editor"
args = { motion = "word_left" }
"#;

/// The defaults' text, for an edit to read on any platform.
fn sources() -> [DefaultKeymap<'static>; 1] {
    [(Path::new(DEFAULT_KEYMAP_PATH), DEFAULTS)]
}

/// The defaults as on Linux.
fn defaults() -> Vec<KeymapLayer> {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new(DEFAULT_KEYMAP_PATH), DEFAULTS, None, Os::Linux);
    assert_eq!(diagnostics, []);
    vec![layer.unwrap()]
}

fn edit(source: &str, edit: KeymapEdit) -> Result<String, KeymapEditError> {
    edit_keymap_on(&sources(), source, &edit, Os::Linux)
}

fn add(keys: &str, context: Option<&str>, command: &str) -> KeymapEdit {
    KeymapEdit::Add {
        keys: keys.into(),
        context: context.map(Into::into),
        command: command.into(),
        args: None,
    }
}

fn remove(keys: &str, context: Option<&str>) -> KeymapEdit {
    KeymapEdit::Remove {
        keys: keys.into(),
        context: context.map(Into::into),
    }
}

fn change(old_keys: &str, context: Option<&str>, new_keys: &str) -> KeymapEdit {
    KeymapEdit::Change {
        old_keys: old_keys.into(),
        context: context.map(Into::into),
        new_keys: new_keys.into(),
    }
}

fn reset(command: &str) -> KeymapEdit {
    KeymapEdit::ResetCommand {
        command: command.into(),
    }
}

fn user_layer(source: &str) -> KeymapLayer {
    let (layer, diagnostics) =
        KeymapLayer::parse_for(Path::new("keymap.toml"), source, None, Os::Linux);
    assert!(
        layer.is_some(),
        "doesn't load: {diagnostics:?}\n---\n{source}"
    );
    layer.unwrap()
}

/// The effective bindings with `source` over the defaults: each winning
/// binding's keys, context and command, with args, in the order read.
fn effective(source: &str) -> Vec<String> {
    let mut layers = defaults();
    layers.push(user_layer(source));
    let keymap = Keymap::from_layers(&layers);
    let bindings = keymap.bindings();
    bindings
        .iter()
        .enumerate()
        .filter(|(ix, binding)| {
            !bindings[ix + 1..]
                .iter()
                .any(|later| later.keys == binding.keys && later.context == binding.context)
        })
        .map(|(_, binding)| {
            let context = binding
                .context
                .as_ref()
                .map_or("*".into(), ToString::to_string);
            let args = if binding.args.is_null() {
                String::new()
            } else {
                format!(" {}", binding.args)
            };
            format!("{} [{context}] {}{args}", binding.keys, binding.command)
        })
        .collect()
}

fn defaults_effective() -> Vec<String> {
    effective("")
}

const USER: &str = r#"# My keymap.

# Fold with F1.
[[bindings]]
keys = "f1"
command = "editor.fold_all"
context = "editor"

# Quit everywhere.
# (Two lines of comment.)
[[bindings]]
keys = "ctrl-q"   # the usual
command = "app.quit"

# Trailing words.
"#;

#[test]
fn add_appends_entry() {
    let edited = edit(USER, add("ctrl-alt-s", Some("editor"), "editor.save")).unwrap();
    assert_eq!(
        edited,
        format!(
            "{USER}\n[[bindings]]\nkeys = \"ctrl-alt-s\"\ncommand = \"editor.save\"\ncontext = \"editor\"\n"
        )
    );

    // Everywhere, with args, in the table's order; into an empty file.
    let args: toml::Value = toml::from_str::<toml::Table>("motion = \"left\"\nextend = true")
        .unwrap()
        .into();
    let edited = edit(
        "",
        KeymapEdit::Add {
            keys: "alt-h".into(),
            context: None,
            command: "editor.move".into(),
            args: Some(args),
        },
    )
    .unwrap();
    assert_eq!(
        edited,
        "[[bindings]]\nkeys = \"alt-h\"\ncommand = \"editor.move\"\nargs = { extend = true, motion = \"left\" }\n"
    );

    // A file with no final newline, and one whose last entry is followed
    // by a blank line, each get one blank line before the new entry.
    let bare = "[[bindings]]\nkeys = \"f1\"\ncommand = \"app.quit\"";
    let edited = edit(bare, add("f2", None, "app.quit")).unwrap();
    assert_eq!(
        edited,
        format!("{bare}\n\n[[bindings]]\nkeys = \"f2\"\ncommand = \"app.quit\"\n")
    );
    // A file with \r\n line endings gets them in the new entry too.
    let crlf = "# Mine.\r\n\r\n[[bindings]]\r\nkeys = \"f1\"\r\ncommand = \"app.quit\"\r\n";
    let edited = edit(crlf, add("f2", None, "app.quit")).unwrap();
    assert_eq!(
        edited,
        format!("{crlf}\r\n[[bindings]]\r\nkeys = \"f2\"\r\ncommand = \"app.quit\"\r\n")
    );
    let removed = edit(&edited, remove("f1", None)).unwrap();
    assert_eq!(
        removed,
        "# Mine.\r\n\r\n[[bindings]]\r\nkeys = \"f2\"\r\ncommand = \"app.quit\"\r\n"
    );
    let spaced = "[[bindings]]\nkeys = \"f1\"\ncommand = \"app.quit\"\n\n";
    let edited = edit(spaced, add("f2", None, "app.quit")).unwrap();
    assert_eq!(
        edited,
        format!("{spaced}[[bindings]]\nkeys = \"f2\"\ncommand = \"app.quit\"\n")
    );
}

#[test]
fn remove_user_entry_deletes_with_its_comment() {
    let edited = edit(USER, remove("f1", Some("editor"))).unwrap();
    assert_eq!(
        edited,
        r#"# My keymap.

# Quit everywhere.
# (Two lines of comment.)
[[bindings]]
keys = "ctrl-q"   # the usual
command = "app.quit"

# Trailing words.
"#
    );

    let edited = edit(USER, remove("ctrl-q", None)).unwrap();
    assert_eq!(
        edited,
        r#"# My keymap.

# Fold with F1.
[[bindings]]
keys = "f1"
command = "editor.fold_all"
context = "editor"

# Trailing words.
"#
    );

    // The first entry, its comment starting the file: the next one starts
    // it.
    let first = "# F1.\n[[bindings]]\nkeys = \"f1\"\ncommand = \"app.quit\"\n\n\
                 [[bindings]]\nkeys = \"f2\"\ncommand = \"app.quit\"\n";
    let edited = edit(first, remove("f1", None)).unwrap();
    assert_eq!(
        edited,
        "[[bindings]]\nkeys = \"f2\"\ncommand = \"app.quit\"\n"
    );

    // Every user entry for the keys goes; a default under them is then
    // unbound too.
    let user = r#"[[bindings]]
keys = "ctrl-s"
command = "app.quit"
context = "editor"

[[bindings]]
keys = "ctrl-s"
command = "editor.fold_all"
context = "editor"
"#;
    let edited = edit(user, remove("ctrl-s", Some("editor"))).unwrap();
    assert_eq!(
        edited,
        "[[bindings]]\nkeys = \"ctrl-s\"\ncommand = \"\"\ncontext = \"editor\"\n"
    );
}

#[test]
fn remove_default_appends_unbind() {
    let edited = edit(USER, remove("ctrl-k ctrl-0", Some("editor"))).unwrap();
    assert_eq!(
        edited,
        format!(
            "{USER}\n[[bindings]]\nkeys = \"ctrl-k ctrl-0\"\ncommand = \"\"\ncontext = \"editor\"\n"
        )
    );

    // Keys that aren't bound, or bound in another context, are an error.
    for (keys, context) in [("f9", Some("editor")), ("ctrl-s", None)] {
        assert!(
            matches!(
                edit(USER, remove(keys, context)),
                Err(KeymapEditError::NotBound { .. })
            ),
            "{keys}"
        );
    }
    // As is a default already unbound.
    let unbound = edit("", remove("ctrl-s", Some("editor"))).unwrap();
    assert!(matches!(
        edit(&unbound, remove("ctrl-s", Some("editor"))),
        Err(KeymapEditError::NotBound { .. })
    ));
}

#[test]
fn change_user_entry() {
    let edited = edit(USER, change("ctrl-q", None, "ctrl-shift-q")).unwrap();
    assert_eq!(edited, USER.replace("\"ctrl-q\"", "\"ctrl-shift-q\""));

    // Keys by platform: only this platform's change.
    let by_platform = r#"[[bindings]]
keys = { macos = "cmd-q", linux = "ctrl-q" }
command = "app.quit"
"#;
    let edited = edit(by_platform, change("ctrl-q", None, "alt-f4")).unwrap();
    assert_eq!(edited, by_platform.replace("\"ctrl-q\"", "\"alt-f4\""));
    let by_default = r#"[[bindings]]
keys = { macos = "cmd-q", default = "ctrl-q" }
command = "app.quit"
"#;
    let edited = edit(by_default, change("ctrl-q", None, "alt-f4")).unwrap();
    assert_eq!(
        edited,
        by_default.replace("\"ctrl-q\" }", "\"ctrl-q\", linux = \"alt-f4\" }")
    );
    // Changed again, the platform's own keys are rewritten in place.
    let again = edit(&edited, change("alt-f4", None, "ctrl-w")).unwrap();
    assert_eq!(again, edited.replace("\"alt-f4\"", "\"ctrl-w\""));

    // Keys by platform as dotted keys, or a table of their own: the other
    // platforms' keys and their comments stay.
    let dotted = r#"[[bindings]]
keys.macos = "cmd-q"  # the Mac's
keys.default = "ctrl-q"
command = "app.quit"
"#;
    let edited = edit(dotted, change("ctrl-q", None, "alt-f4")).unwrap();
    assert_eq!(
        edited,
        dotted.replace(
            "keys.default = \"ctrl-q\"\n",
            "keys.default = \"ctrl-q\"\nkeys.linux = \"alt-f4\"\n"
        )
    );
    let table = r#"[[bindings]]
command = "app.quit"

[bindings.keys]
# The Mac's.
macos = "cmd-q"
linux = "ctrl-q"

# Next.
[[bindings]]
keys = "f1"
command = "app.quit"
"#;
    let edited = edit(table, change("ctrl-q", None, "alt-f4")).unwrap();
    assert_eq!(edited, table.replace("\"ctrl-q\"", "\"alt-f4\""));
    let table_default = table.replace("linux = ", "default = ");
    let edited = edit(&table_default, change("ctrl-q", None, "alt-f4")).unwrap();
    assert_eq!(
        edited,
        table_default.replace(
            "default = \"ctrl-q\"\n",
            "default = \"ctrl-q\"\nlinux = \"alt-f4\"\n"
        )
    );
    // Deleting such an entry takes its keys table with it.
    let removed = edit(table, remove("ctrl-q", None)).unwrap();
    assert_eq!(
        removed,
        "# Next.\n[[bindings]]\nkeys = \"f1\"\ncommand = \"app.quit\"\n"
    );
}

#[test]
fn remove_user_entry_deletes_its_tables_apart_from_it() {
    // `[bindings.args]` after another table is still the last binding's.
    let user = r#"[[bindings]]
keys = "f1"
command = "editor.move"
args = { motion = "up" }

[[bindings]]
keys = "f2"
command = "editor.move"

[metadata]
note = "kept"

# Its args.
[bindings.args]
motion = "left"
"#;
    let kept = r#"[[bindings]]
keys = "f1"
command = "editor.move"
args = { motion = "up" }

[metadata]
note = "kept"
"#;
    let removed = edit(user, remove("f2", None)).unwrap();
    assert_eq!(removed, kept);
    let layer = user_layer(&removed);
    assert_eq!(layer.entries.len(), 1);
    assert_eq!(layer.entries[0].args, serde_json::json!({ "motion": "up" }));
    let removed = edit(user, reset("editor.move")).unwrap();
    assert_eq!(removed, "[metadata]\nnote = \"kept\"\n");

    // The only binding, its args apart from it.
    let only = r#"[[bindings]]
keys = "f2"
command = "editor.move"

[metadata]
note = "kept"

[bindings.args]
motion = "left"
"#;
    let removed = edit(only, remove("f2", None)).unwrap();
    assert_eq!(removed, "[metadata]\nnote = \"kept\"\n");
    // Its keys apart from it are changed where they are.
    let keys_apart = only.replace(
        "[bindings.args]\nmotion = \"left\"\n",
        "[bindings.keys]\nmacos = \"cmd-2\"\ndefault = \"f2\"\n",
    );
    let keys_apart = keys_apart.replace("keys = \"f2\"\n", "");
    let changed = edit(&keys_apart, change("f2", None, "f3")).unwrap();
    assert_eq!(
        changed,
        keys_apart.replace("default = \"f2\"\n", "default = \"f2\"\nlinux = \"f3\"\n")
    );
}

#[test]
fn change_default_unbinds_and_adds() {
    let edited = edit(USER, change("ctrl-s", Some("editor"), "ctrl-alt-s")).unwrap();
    assert_eq!(
        edited,
        format!(
            "{USER}\n[[bindings]]\nkeys = \"ctrl-s\"\ncommand = \"\"\ncontext = \"editor\"\n\n\
             [[bindings]]\nkeys = \"ctrl-alt-s\"\ncommand = \"editor.save\"\ncontext = \"editor\"\n"
        )
    );

    // The default's args go with it.
    let edited = edit("", change("ctrl-left", Some("editor"), "alt-b")).unwrap();
    assert_eq!(
        edited,
        "[[bindings]]\nkeys = \"ctrl-left\"\ncommand = \"\"\ncontext = \"editor\"\n\n\
         [[bindings]]\nkeys = \"alt-b\"\ncommand = \"editor.move\"\ncontext = \"editor\"\n\
         args = { motion = \"word_left\" }\n"
    );

    assert!(matches!(
        edit(USER, change("f9", None, "f10")),
        Err(KeymapEditError::NotBound { .. })
    ));
}

#[test]
fn reset_command() {
    let user = r#"# Mine.

# Save with F2.
[[bindings]]
keys = "f2"
command = "editor.save"

# Quit with F3.
[[bindings]]
keys = "f3"
command = "app.quit"

# Not ctrl-s.
[[bindings]]
keys = "ctrl-s"
command = ""
context = "editor"

# Not ctrl-k ctrl-0 either.
[[bindings]]
keys = "ctrl-k ctrl-0"
command = ""
context = "editor"

# Save on the Mac too, there only.
[[bindings]]
keys = "cmd-s"
command = "editor.save"
platform = "macos"
"#;
    let edited = edit(user, reset("editor.save")).unwrap();
    assert_eq!(
        edited,
        r#"# Mine.

# Quit with F3.
[[bindings]]
keys = "f3"
command = "app.quit"

# Not ctrl-k ctrl-0 either.
[[bindings]]
keys = "ctrl-k ctrl-0"
command = ""
context = "editor"
"#
    );

    // An unbind of one of its defaults on another platform only goes too.
    let mac_only = r#"[[bindings]]
keys = "f3"
command = "app.quit"

[[bindings]]
keys = "ctrl-s"
command = ""
context = "editor"
platform = "macos"

[[bindings]]
keys = "ctrl-k ctrl-0"
command = ""
context = "editor"
platform = "macos"
"#;
    let edited = edit(mac_only, reset("editor.save")).unwrap();
    assert_eq!(
        edited,
        r#"[[bindings]]
keys = "f3"
command = "app.quit"

[[bindings]]
keys = "ctrl-k ctrl-0"
command = ""
context = "editor"
platform = "macos"
"#
    );

    // A command with nothing of the user's is left as it is.
    assert_eq!(edit(USER, reset("editor.save")).unwrap(), USER);
}

#[test]
fn comments_and_other_entries_survive_byte_for_byte() {
    // Odd spacing and quoting, inline comments, blank lines, a dotted args
    // key and an entry for another platform: none of it is touched.
    let user = "# Top.\n\n\
                [[bindings]]   # header comment\n\
                keys    =   'f1'\n\
                command = \"editor.fold_all\"  # why\n\
                context = \"editor\"\n\
                \n\
                \n\
                # Kept apart from the next.\n\
                \n\
                # Says ctrl-q.\n\
                [[bindings]]\n\
                keys = \"ctrl-q\"\n\
                command = \"app.quit\"\n\
                \n\
                [[bindings]]\n\
                keys = \"cmd-x\"\n\
                command = \"editor.move\"\n\
                args.motion = \"left\"\n\
                platform = \"macos\"\n\
                # The end.\n";
    let added = edit(user, add("f5", None, "app.quit")).unwrap();
    assert!(added.starts_with(user), "{added}");

    let removed = edit(user, remove("ctrl-q", None)).unwrap();
    assert_eq!(
        removed,
        user.replace(
            "# Says ctrl-q.\n[[bindings]]\nkeys = \"ctrl-q\"\ncommand = \"app.quit\"\n\n",
            ""
        )
    );

    let changed = edit(user, change("f1", Some("editor"), "f6")).unwrap();
    assert_eq!(changed, user.replace("'f1'", "\"f6\""));

    // The blank line above it is kept, and the two below it go with it.
    let folds_reset = edit(user, reset("editor.fold_all")).unwrap();
    assert_eq!(
        folds_reset,
        user.replace(
            "[[bindings]]   # header comment\nkeys    =   'f1'\n\
             command = \"editor.fold_all\"  # why\ncontext = \"editor\"\n\n\n",
            ""
        )
    );

    // Unbinding a default after it all leaves every byte above in place.
    let unbound = edit(user, remove("ctrl-s", Some("editor"))).unwrap();
    assert!(unbound.starts_with(user), "{unbound}");

    // A byte order mark, mixed line endings and no final newline: kept
    // wherever the edit isn't.
    let odd = "\u{feff}# Mixed.\r\n\r\n[[bindings]]\nkeys = \"f1\"\r\ncommand = \"app.quit\"\n\n\
               [[bindings]]\r\nkeys = \"f2\"\ncommand = \"app.quit\"";
    let changed = edit(odd, change("f1", None, "f3")).unwrap();
    assert_eq!(changed, odd.replace("\"f1\"", "\"f3\""));
    let removed = edit(odd, remove("f1", None)).unwrap();
    assert_eq!(
        removed,
        "\u{feff}# Mixed.\r\n\r\n[[bindings]]\r\nkeys = \"f2\"\ncommand = \"app.quit\""
    );
    let reset_ = edit(odd, reset("editor.save")).unwrap();
    assert_eq!(reset_, odd);
    let added = edit(odd, add("f4", None, "app.quit")).unwrap();
    assert_eq!(
        added,
        format!("{odd}\r\n\r\n[[bindings]]\r\nkeys = \"f4\"\r\ncommand = \"app.quit\"\r\n")
    );
    // The first entry, the mark before it: the mark stays.
    let bom = "\u{feff}[[bindings]]\nkeys = \"f1\"\ncommand = \"app.quit\"\n\n\
               [[bindings]]\nkeys = \"f2\"\ncommand = \"app.quit\"\n";
    let removed = edit(bom, remove("f1", None)).unwrap();
    assert_eq!(
        removed,
        "\u{feff}[[bindings]]\nkeys = \"f2\"\ncommand = \"app.quit\"\n"
    );
}

#[test]
fn invalid_keys_rejected() {
    for keys in ["", "ctrl-", "hyper-x", "ctrl-k  bogus-key"] {
        for edit_ in [
            add(keys, None, "app.quit"),
            remove(keys, None),
            change(keys, None, "f1"),
            change("ctrl-q", None, keys),
        ] {
            assert!(
                matches!(edit(USER, edit_.clone()), Err(KeymapEditError::Keys { .. })),
                "{edit_:?}"
            );
        }
    }
    // A click binds only alone with a modifier: such an entry would be
    // dropped, so it isn't written.
    assert!(matches!(
        edit(USER, add("ctrl-k click", None, "app.quit")),
        Err(KeymapEditError::Invalid(_))
    ));
    assert!(edit(USER, add("cmd-click", None, "app.quit")).is_ok());
}

#[test]
fn invalid_context_rejected() {
    for context in ["", "editor &&", "(editor", "a == "] {
        for edit_ in [
            add("f1", Some(context), "app.quit"),
            remove("f1", Some(context)),
            change("f1", Some(context), "f2"),
        ] {
            assert!(
                matches!(
                    edit(USER, edit_.clone()),
                    Err(KeymapEditError::Context { .. })
                ),
                "{edit_:?}"
            );
        }
    }
    for command in ["", "quit", "App.quit", "app..quit"] {
        assert!(
            matches!(
                edit(USER, add("f1", None, command)),
                Err(KeymapEditError::Command(_))
            ),
            "{command}"
        );
        assert!(
            matches!(edit(USER, reset(command)), Err(KeymapEditError::Command(_))),
            "{command}"
        );
    }
    let not_a_table = KeymapEdit::Add {
        keys: "f1".into(),
        context: None,
        command: "app.quit".into(),
        args: Some(toml::Value::Integer(3)),
    };
    assert!(matches!(
        edit(USER, not_a_table),
        Err(KeymapEditError::Args)
    ));
}

#[test]
fn result_always_loads() {
    let edits = [
        add("f1", None, "app.quit"),
        add("ctrl-k ctrl-1", Some("editor && !vim"), "editor.fold_all"),
        remove("ctrl-s", Some("editor")),
        change("f1", None, "f2"),
        change("ctrl-shift-s", Some("editor"), "ctrl-alt-shift-s"),
        remove("ctrl-k ctrl-1", Some("editor && !vim")),
        reset("editor.save"),
        reset("app.quit"),
    ];
    let mut source = USER.to_owned();
    for edit_ in edits {
        source = edit(&source, edit_.clone()).unwrap_or_else(|e| panic!("{edit_:?}: {e}"));
        user_layer(&source);
    }

    // A file that doesn't load isn't made to load by an edit: it's an
    // error, and the file is left to fix by hand.
    let broken = "[[bindings]]\nkeys = \"f1\"\n";
    assert!(matches!(
        edit(broken, add("f2", None, "app.quit")),
        Err(KeymapEditError::Invalid(_))
    ));
    // Even an edit with nothing to change.
    assert!(matches!(
        edit(broken, reset("editor.save")),
        Err(KeymapEditError::Invalid(_))
    ));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keymap.toml");
    fs::write(&path, broken).unwrap();
    assert!(matches!(
        write_keymap_on(&path, &sources(), &reset("editor.save"), Os::Linux),
        Err(KeymapEditError::Invalid(_))
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    assert!(matches!(
        edit("[[bindings", add("f2", None, "app.quit")),
        Err(KeymapEditError::NotToml(_))
    ));
    assert!(matches!(
        edit("bindings = []", add("f2", None, "app.quit")),
        Err(KeymapEditError::Layout)
    ));

    // The built-in keymap, as on this platform: unbinding and rebinding
    // a default loads over it.
    let save = KeySequence::parse("secondary-s").unwrap().to_string();
    let edited = edit_keymap("", &remove(&save, Some("editor"))).unwrap();
    let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), &edited, None);
    assert!(layer.is_some(), "{diagnostics:?}");
    let edited = edit_keymap(&edited, &reset("editor.save")).unwrap();
    assert_eq!(edited, "");
}

#[test]
fn effective_keymap_after_each_edit() {
    let base = defaults_effective();
    assert_eq!(
        base,
        [
            "ctrl-s [editor] editor.save",
            "ctrl-k ctrl-0 [editor] editor.fold_all",
            "ctrl-shift-s [editor] editor.save",
            r#"ctrl-left [editor] editor.move {"motion":"word_left"}"#,
        ]
    );
    let with = |extra: &[&str]| {
        let mut expected = base.clone();
        expected.extend(extra.iter().map(ToString::to_string));
        expected
    };
    let without = |gone: &str, extra: &[&str]| {
        let mut expected: Vec<String> = base.iter().filter(|b| *b != gone).cloned().collect();
        expected.extend(extra.iter().map(ToString::to_string));
        expected
    };

    let added = edit("", add("f1", None, "app.quit")).unwrap();
    assert_eq!(effective(&added), with(&["f1 [*] app.quit"]));

    let overriding = edit(&added, add("ctrl-s", Some("editor"), "app.quit")).unwrap();
    assert_eq!(
        effective(&overriding),
        without(
            "ctrl-s [editor] editor.save",
            &["f1 [*] app.quit", "ctrl-s [editor] app.quit"]
        )
    );

    // Removing the user's ctrl-s unbinds the default under it too.
    let removed = edit(&overriding, remove("ctrl-s", Some("editor"))).unwrap();
    assert_eq!(
        effective(&removed),
        without("ctrl-s [editor] editor.save", &["f1 [*] app.quit"])
    );

    let removed_default = edit(&added, remove("ctrl-k ctrl-0", Some("editor"))).unwrap();
    assert_eq!(
        effective(&removed_default),
        without(
            "ctrl-k ctrl-0 [editor] editor.fold_all",
            &["f1 [*] app.quit"]
        )
    );

    let changed = edit(&added, change("f1", None, "f2")).unwrap();
    assert_eq!(effective(&changed), with(&["f2 [*] app.quit"]));

    let changed_default = edit(&added, change("ctrl-left", Some("editor"), "alt-b")).unwrap();
    assert_eq!(
        effective(&changed_default),
        without(
            r#"ctrl-left [editor] editor.move {"motion":"word_left"}"#,
            &[
                "f1 [*] app.quit",
                r#"alt-b [editor] editor.move {"motion":"word_left"}"#
            ]
        )
    );

    // Resetting brings back the defaults and keeps the rest.
    let customised = edit(&removed, change("ctrl-shift-s", Some("editor"), "f4")).unwrap();
    let customised = edit(&customised, add("f5", None, "editor.save")).unwrap();
    let reset_ = edit(&customised, reset("editor.save")).unwrap();
    assert_eq!(effective(&reset_), with(&["f1 [*] app.quit"]));
}

#[test]
fn write_creates_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config").join("keymap.toml");
    let written =
        write_keymap_on(&path, &sources(), &add("f1", None, "app.quit"), Os::Linux).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        format!("{KEYMAP_HEADER}\n[[bindings]]\nkeys = \"f1\"\ncommand = \"app.quit\"\n")
    );
    assert_eq!(written.source, text);
    assert!(
        written
            .keymap
            .bindings()
            .iter()
            .any(|binding| binding.keys.to_string() == "f1" && binding.layer == 1)
    );

    // The header is only for a new file: removing the entry keeps it, and
    // the header stays as it is.
    write_keymap_on(&path, &sources(), &remove("f1", None), Os::Linux).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), KEYMAP_HEADER);

    // An edit that changes nothing, or fails, creates no file.
    let other = dir.path().join("other.toml");
    write_keymap_on(&other, &sources(), &reset("app.quit"), Os::Linux).unwrap();
    assert!(write_keymap_on(&other, &sources(), &remove("f1", None), Os::Linux).is_err());
    assert!(!other.exists());
}

#[test]
fn write_reports_the_rebuilt_keymaps_issues() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keymap.toml");
    let written = write_keymap_on(
        &path,
        &sources(),
        &add("ctrl-s", Some("editor"), "app.quit"),
        Os::Linux,
    )
    .unwrap();
    assert!(
        written
            .issues
            .iter()
            .any(|issue| issue.to_string().contains("overrides")),
        "{:?}",
        written.issues
    );
}

#[cfg(unix)]
#[test]
fn write_is_atomic_and_keeps_mode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keymap.toml");
    fs::write(&path, USER).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    // A second name for the old file: written in place, it would change
    // too; replaced by a rename, it keeps the old text.
    let old = dir.path().join("old.toml");
    fs::hard_link(&path, &old).unwrap();

    write_keymap_on(&path, &sources(), &add("f5", None, "app.quit"), Os::Linux).unwrap();
    assert_eq!(fs::read_to_string(&old).unwrap(), USER);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with(USER) && text.contains("keys = \"f5\""));
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
    // No temporary file is left behind.
    let mut names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["keymap.toml", "old.toml"]);

    // Through a symlink, to its target; the link stays a link.
    let link = dir.path().join("link.toml");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    write_keymap_on(&link, &sources(), &remove("f5", None), Os::Linux).unwrap();
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), USER);

    // A link pointed elsewhere while the edit is made: the edit goes to
    // its new target, the old one is left as it was.
    let other = dir.path().join("other.toml");
    fs::write(&other, "").unwrap();
    let mut calls = 0;
    write_keymap_hooked(
        &link,
        &sources(),
        &add("f6", None, "app.quit"),
        Os::Linux,
        || {
            calls += 1;
            if calls == 1 {
                fs::remove_file(&link).unwrap();
                std::os::unix::fs::symlink(&other, &link).unwrap();
            }
        },
    )
    .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), USER);
    assert_eq!(
        fs::read_to_string(&other).unwrap(),
        "[[bindings]]\nkeys = \"f6\"\ncommand = \"app.quit\"\n"
    );

    // New permissions while the edit is made are kept.
    let mut calls = 0;
    write_keymap_hooked(
        &path,
        &sources(),
        &add("f7", None, "app.quit"),
        Os::Linux,
        || {
            calls += 1;
            if calls == 1 {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            }
        },
    )
    .unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(fs::read_to_string(&path).unwrap().contains("keys = \"f7\""));
}

#[test]
fn concurrent_change_is_not_lost() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keymap.toml");
    fs::write(&path, USER).unwrap();
    let theirs = format!("{USER}\n[[bindings]]\nkeys = \"f7\"\ncommand = \"app.quit\"\n");
    let mut calls = 0;
    let written = write_keymap_hooked(
        &path,
        &sources(),
        &add("f8", None, "app.quit"),
        Os::Linux,
        || {
            calls += 1;
            if calls == 1 {
                // Saved by hand while the edit was being made.
                fs::write(&path, &theirs).unwrap();
            }
        },
    )
    .unwrap();
    assert_eq!(calls, 2);
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        format!("{theirs}\n[[bindings]]\nkeys = \"f8\"\ncommand = \"app.quit\"\n")
    );
    assert_eq!(written.source, text);

    // An edit with nothing to do is made again if the file changes: here
    // a reset that becomes needed.
    fs::write(&path, USER).unwrap();
    let saved = format!("{USER}\n[[bindings]]\nkeys = \"f2\"\ncommand = \"editor.save\"\n");
    let mut calls = 0;
    let written = write_keymap_hooked(&path, &sources(), &reset("editor.save"), Os::Linux, || {
        calls += 1;
        if calls == 1 {
            fs::write(&path, &saved).unwrap();
        }
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(fs::read_to_string(&path).unwrap(), USER);
    assert_eq!(written.source, USER);

    // A file that keeps changing is an error, and the last change stands.
    let mut n = 0;
    let result = write_keymap_hooked(
        &path,
        &sources(),
        &add("f9", None, "app.quit"),
        Os::Linux,
        || {
            n += 1;
            fs::write(&path, format!("# changed {n}\n")).unwrap();
        },
    );
    assert!(matches!(result, Err(KeymapEditError::Io(_))));
    assert_eq!(fs::read_to_string(&path).unwrap(), "# changed 2\n");
}
