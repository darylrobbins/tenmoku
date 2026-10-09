//! Colour themes: the bundled fastedit Dark, user theme files and their
//! diagnostics.

use std::path::Path;

use fe_config::{Appearance, Color, Diagnostic, Severity, Theme, load_themes, themes_dir_from};

fn color(text: &str) -> Color {
    text.parse().unwrap()
}

fn write(dir: &Path, file: &str, source: &str) -> std::path::PathBuf {
    let path = dir.join(file);
    std::fs::write(&path, source).unwrap();
    path
}

/// Loads `source` as the only user theme, `user.toml`, and returns the
/// theme and the diagnostics.
fn load_one(source: &str) -> (Option<Theme>, Vec<Diagnostic>) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "user.toml", source);
    let (set, diagnostics) = load_themes(Some(dir.path()));
    (set.get("user").cloned(), diagnostics)
}

/// Copied from `UiTheme::default_dark` (crates/fe-ui/src/theme.rs).
const UI: [(&str, u32); 20] = [
    ("background", 0x16181dff),
    ("panel_background", 0x121418ff),
    ("border", 0x23262dff),
    ("text", 0x8b8f98ff),
    ("muted_text", 0x4b5160ff),
    ("error_text", 0xe06c75ff),
    ("row_hover", 0xffffff0a),
    ("row_selected", 0x4f7cc833),
    ("row_selected_inactive", 0xffffff0f),
    ("row_focus_border", 0x4f7cc8aa),
    ("icon", 0x8b8f98ff),
    ("folder_icon", 0x7f9fd8ff),
    ("chevron", 0x6c717cff),
    ("indent_guide", 0xffffff14),
    ("tab_active", 0x16181dff),
    ("tab_active_text", 0xc8ccd4ff),
    ("tab_inactive", 0x121418ff),
    ("dirty_dot", 0xd19a66ff),
    ("drop_target", 0x4f7cc8ff),
    ("accent", 0x4f7cc8ff),
];

/// Copied from `EditorTheme::from_settings` (crates/fe-gpui/src/lib.rs).
const EDITOR: [(&str, u32); 7] = [
    ("background", 0x16181dff),
    ("foreground", 0xc8ccd4ff),
    ("gutter_foreground", 0x4b5160ff),
    ("gutter_active_foreground", 0xa0a7b4ff),
    ("active_line", 0xffffff08),
    ("selection", 0x4f7cc855),
    ("cursor", 0x7aa2f7ff),
];

/// Copied from `EditorTheme::from_settings` (crates/fe-gpui/src/lib.rs).
const SYNTAX: [(&str, u32); 25] = [
    ("attribute", 0xd19a66ff),
    ("comment", 0x5c6370ff),
    ("constant", 0xd19a66ff),
    ("constructor", 0xe5c07bff),
    ("escape", 0x56b6c2ff),
    ("function", 0x61afefff),
    ("function.macro", 0x56b6c2ff),
    ("keyword", 0xc678ddff),
    ("label", 0xe06c75ff),
    ("markup.heading", 0xe06c75ff),
    ("markup.link", 0x61afefff),
    ("markup.link.url", 0x56b6c2ff),
    ("markup.raw", 0x98c379ff),
    ("module", 0xe5c07bff),
    ("number", 0xd19a66ff),
    ("operator", 0x56b6c2ff),
    ("property", 0xe06c75ff),
    ("punctuation", 0x7f848eff),
    ("punctuation.special", 0x56b6c2ff),
    ("string", 0x98c379ff),
    ("string.special", 0x56b6c2ff),
    ("string.special.key", 0xe06c75ff),
    ("tag", 0xe06c75ff),
    ("type", 0xe5c07bff),
    ("variable.builtin", 0xe06c75ff),
];

#[test]
fn bundled_dark_matches_the_hard_coded_colours() {
    let (theme, diagnostics) = Theme::parse(
        Path::new("fastedit-dark.toml"),
        fe_config::theme::FASTEDIT_DARK_SOURCE,
    );
    assert_eq!(diagnostics, []);
    let theme = theme.unwrap();
    assert_eq!(&theme, Theme::fastedit_dark());
    assert_eq!(theme.name, "fastedit Dark");
    assert_eq!(theme.appearance, Appearance::Dark);

    let ui: Vec<_> = fe_config::UiColors::KEYS
        .iter()
        .map(|key| (*key, theme.ui.get(key).unwrap().to_rgba_u32()))
        .collect();
    assert_eq!(ui, UI);
    let editor: Vec<_> = fe_config::EditorColors::KEYS
        .iter()
        .map(|key| (*key, theme.editor.get(key).unwrap().to_rgba_u32()))
        .collect();
    assert_eq!(editor, EDITOR);
    let syntax: Vec<_> = theme
        .syntax
        .iter()
        .map(|(name, color)| (name.as_str(), color.to_rgba_u32()))
        .collect();
    assert_eq!(syntax, SYNTAX);

    // The default set is the bundled theme alone.
    let (set, diagnostics) = load_themes(None);
    assert_eq!(diagnostics, []);
    assert_eq!(set.names().collect::<Vec<_>>(), ["fastedit Dark"]);
    assert_eq!(set, fe_config::ThemeSet::default());
}

#[test]
fn partial_theme_falls_back() {
    let (theme, diagnostics) = load_one(
        r##"
name = "user"
[editor]
background = "#000000"
[syntax]
"keyword" = "#ff0000"
"##,
    );
    assert_eq!(diagnostics, []);
    let theme = theme.unwrap();
    let dark = Theme::fastedit_dark();
    let mut expected = dark.clone();
    expected.name = "user".into();
    expected.editor.background = color("#000");
    // The file's own captures come after the fallbacks, replacements too.
    expected.syntax.retain(|(name, _)| name != "keyword");
    expected
        .syntax
        .push(("keyword".into(), "#f00".parse().unwrap()));
    assert_eq!(theme, expected);
    assert_eq!(theme.ui, dark.ui);
    assert_eq!(theme.appearance, Appearance::Dark);

    // New captures come after fastedit Dark's, in file order, and a nested
    // table is a group of dotted names. With no `name`, it's the file stem.
    let (theme, diagnostics) = load_one(
        r##"
appearance = "light"
[syntax]
zz = "#111"
function.builtin = "#222"
aa = "#333"
"##,
    );
    assert_eq!(diagnostics, []);
    let theme = theme.unwrap();
    assert_eq!(theme.name, "user");
    assert_eq!(theme.appearance, Appearance::Light);
    let added: Vec<_> = theme.syntax[SYNTAX.len()..]
        .iter()
        .map(|(name, color)| (name.as_str(), color.to_string()))
        .collect();
    assert_eq!(
        added,
        [
            ("zz", "#111111".to_owned()),
            ("function.builtin", "#222222".to_owned()),
            ("aa", "#333333".to_owned()),
        ]
    );

    // Replaced bundled captures and new ones are in file order, after the
    // fallbacks for the bundled captures the file doesn't mention.
    let (theme, diagnostics) = load_one(
        r##"
[syntax]
keyword = "#111"
new_capture = "#222"
comment = "#333"
"##,
    );
    assert_eq!(diagnostics, []);
    let theme = theme.unwrap();
    let names: Vec<_> = theme.syntax.iter().map(|(name, _)| name.as_str()).collect();
    let fallbacks: Vec<_> = SYNTAX
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !["keyword", "comment"].contains(name))
        .collect();
    assert_eq!(names[..fallbacks.len()], fallbacks);
    assert_eq!(
        names[fallbacks.len()..],
        ["keyword", "new_capture", "comment"]
    );
    assert_eq!(theme.syntax_color("comment"), Some(color("#333")));
}

#[test]
fn bad_values_are_diagnosed() {
    let dir = tempfile::tempdir().unwrap();
    let file = write(
        dir.path(),
        "bad.toml",
        r##"name = "Bad"
colour = "#fff"
[ui]
border = "#12345"
tab_active = "#abc"
[editor]
cursor =   "red"
nope = "#fff"
[syntax]
"keyword" = "#010203"
"Bad Name" = "#fff"
"##,
    );
    write(dir.path(), "broken.toml", "name = \"Broken\"\n[ui\n");
    let (set, diagnostics) = load_themes(Some(dir.path()));
    let found: Vec<_> = diagnostics
        .iter()
        .map(|d| {
            (
                d.file.file_name().unwrap().to_str().unwrap(),
                d.severity,
                d.key.as_deref(),
                d.line,
                d.column,
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("bad.toml", Severity::Warning, Some("colour"), 2, 1),
            ("bad.toml", Severity::Error, Some("ui.border"), 4, 10),
            ("bad.toml", Severity::Error, Some("editor.cursor"), 7, 12),
            ("bad.toml", Severity::Warning, Some("editor.nope"), 8, 1),
            (
                "bad.toml",
                Severity::Warning,
                Some("syntax.Bad Name"),
                11,
                1
            ),
            ("broken.toml", Severity::Error, None, 2, 4),
        ],
        "{diagnostics:#?}"
    );
    assert_eq!(diagnostics[0].file, file);

    // The malformed keys fall back; the rest of the file applies.
    let dark = Theme::fastedit_dark();
    let bad = set.get("Bad").unwrap();
    assert_eq!(bad.ui.border, dark.ui.border);
    assert_eq!(bad.ui.tab_active, color("#aabbcc"));
    assert_eq!(bad.editor.cursor, dark.editor.cursor);
    assert_eq!(bad.syntax_color("keyword"), Some(color("#010203")));
    assert_eq!(bad.syntax.len(), SYNTAX.len());

    // The file with a syntax error is left out.
    assert!(set.get("Broken").is_none());
    assert_eq!(set.names().collect::<Vec<_>>(), ["fastedit Dark", "Bad"]);

    // Wrong types are errors too, and a syntax key's falls back.
    let (theme, diagnostics) = load_one(
        r##"
name = "user"
appearance = "dim"
ui = 3
[syntax]
"keyword" = 7
"##,
    );
    let keys: Vec<_> = diagnostics
        .iter()
        .map(|d| (d.severity, d.key.as_deref().unwrap(), d.message.as_str()))
        .collect();
    assert_eq!(
        keys,
        [
            (
                Severity::Error,
                "appearance",
                "expected \"dark\" or \"light\""
            ),
            (Severity::Error, "ui", "expected a table, found integer"),
            (
                Severity::Error,
                "syntax.keyword",
                "expected a colour string, found integer"
            ),
        ]
    );
    let theme = theme.unwrap();
    assert_eq!(theme, {
        let mut dark = dark.clone();
        dark.name = "user".into();
        dark
    });
}

#[test]
fn themes_dir_lists_user_themes() {
    let config = tempfile::tempdir().unwrap();
    let env = |name: &str| (name == "FASTEDIT_CONFIG_DIR").then(|| config.path().into());
    let dir = themes_dir_from(env).unwrap();
    assert_eq!(dir, config.path().join("themes"));
    std::fs::create_dir(&dir).unwrap();

    write(&dir, "b.toml", "name = \"Beta\"\n[ui]\ntext = \"#AbC\"\n");
    write(
        &dir,
        "a.toml",
        "name = \"Alpha\"\n[ui]\ntext = \"#A1B2C3\"\n",
    );
    write(
        &dir,
        "dark.toml",
        "name = \"FASTEDIT dark\"\n[ui]\ntext = \"#a1b2c3d4\"\n",
    );
    write(&dir, "notes.txt", "name = \"Not a theme\"\n");
    std::fs::create_dir(dir.join("sub.toml")).unwrap();

    let (set, diagnostics) = load_themes(Some(&dir));
    assert_eq!(diagnostics, []);
    // The user's fastedit Dark replaces the bundled one in place.
    assert_eq!(
        set.names().collect::<Vec<_>>(),
        ["FASTEDIT dark", "Alpha", "Beta"]
    );
    let text = |name: &str| set.get(name).unwrap().ui.text;
    assert_eq!(
        text("alpha"),
        Color {
            r: 0xa1,
            g: 0xb2,
            b: 0xc3,
            a: 0xff
        }
    );
    assert_eq!(
        text("BETA"),
        Color {
            r: 0xaa,
            g: 0xbb,
            b: 0xcc,
            a: 0xff
        }
    );
    assert_eq!(
        text("fastedit Dark"),
        Color {
            r: 0xa1,
            g: 0xb2,
            b: 0xc3,
            a: 0xd4
        }
    );
    assert!(set.get("Not a theme").is_none());

    // Two user themes with one name: the later file wins, with a warning.
    write(&dir, "c.toml", "\nname = \"alpha\"\n");
    let (set, diagnostics) = load_themes(Some(&dir));
    assert_eq!(set.names().count(), 3);
    assert_eq!(
        set.get("Alpha").unwrap().ui.text,
        Theme::fastedit_dark().ui.text
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    let warning = &diagnostics[0];
    assert_eq!(warning.severity, Severity::Warning);
    assert_eq!(warning.file, dir.join("c.toml"));
    assert_eq!((warning.line, warning.column), (2, 8));

    for bad in [
        "#12345", "red", "#ab", "#abcd", "#abcdefg", "abc", "#ggg", " #abc", "#+bc",
    ] {
        assert!(bad.parse::<Color>().is_err(), "{bad:?} parsed");
    }
}
