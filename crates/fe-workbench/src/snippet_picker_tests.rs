//! Insert Snippet and Configure User Snippets: the snippets valid for the
//! active editor's language listed and inserted through the snippet
//! session; a language's snippet file created with an example, never
//! overwritten, and opened in a tab.

use std::{fs, path::PathBuf};

use fe_config::user_snippets::{load_snippets, snippets_dir};
use fe_gpui::EditorView;
use gpui::{Entity, TestAppContext};

use crate::snippet_picker::{
    CONFIGURE_SNIPPETS, ConfigureRow, ConfigureSnippetsPicker, INSERT_SNIPPET, InsertSnippetPicker,
    SnippetRow,
};
use crate::tests::lsp::Lsp;
use crate::user_snippets_tests::{open_at, write_snippets};
use crate::{ExtensionSnippet, Picker, set_extension_snippets};

fn insert_picker(s: &mut Lsp) -> Option<Entity<Picker<InsertSnippetPicker>>> {
    (s.workbench).read_with(&s.cx, |w, cx| w.snippet_picker(INSERT_SNIPPET, cx))
}

fn configure_picker(s: &mut Lsp) -> Option<Entity<Picker<ConfigureSnippetsPicker>>> {
    (s.workbench).read_with(&s.cx, |w, cx| w.snippet_picker(CONFIGURE_SNIPPETS, cx))
}

/// Insert Snippet's rows now: each name, prefixes and detail.
fn rows(s: &mut Lsp) -> Vec<(String, String, String)> {
    let picker = insert_picker(s).expect("Insert Snippet shows");
    picker.read_with(&s.cx, |picker, _| {
        (picker.delegate.rows().into_iter())
            .map(
                |SnippetRow {
                     name,
                     prefixes,
                     detail,
                     ..
                 }| (name, prefixes, detail),
            )
            .collect()
    })
}

fn row(name: &str, prefixes: &str, detail: &str) -> (String, String, String) {
    (name.to_owned(), prefixes.to_owned(), detail.to_owned())
}

fn extension(name: &str, prefix: &str, body: &str, language: &str) -> ExtensionSnippet {
    ExtensionSnippet {
        extension_id: format!("acme.{}", name.to_lowercase()),
        extension_name: name.to_owned(),
        prefix: prefix.to_owned(),
        body: body.to_owned(),
        description: None,
        languages: vec![language.to_owned()],
    }
}

const RUST: &str = r##"{
    "Test": { "prefix": ["test", "unit"], "body": "#[test]\nfn ${1:name}() {\n\t$0\n}", "description": "A test" },
    "Main function": { "prefix": "main", "body": "fn main() {\n\t$0\n}" },
}"##;

const GLOBAL: &str = r##"{
    "Todo": { "prefix": "todo", "body": "TODO(${1:who}): $0" },
    "Python main": { "prefix": "pymain", "body": "# $1", "scope": "python" },
}"##;

#[gpui::test]
fn insert_snippet_lists_valid_snippets_and_inserts(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write_snippets(&s, "rust.json", RUST);
    write_snippets(&s, "global.code-snippets", GLOBAL);
    s.cx.update(|_, cx| {
        set_extension_snippets(
            cx,
            vec![
                extension("Rusty", "impl", "impl ${1:Type} {\n\t$0\n}", "rust"),
                extension("Gopher", "func", "func ${1:name}() {\n\t$0\n}", "go"),
            ],
        )
    });
    let before = "fn main() {\n    ";
    let editor = open_at(
        &mut s,
        "src/snip.rs",
        "fn main() {\n    \n}\n",
        before.len(),
    );

    // Rust's, the user's by name then extensions'; not Python's or Go's.
    s.run("workbench.insert_snippet");
    assert_eq!(
        rows(&mut s),
        [
            row("Main function", "main", "User snippet"),
            row("Test", "test, unit", "A test"),
            row("Todo", "todo", "User snippet"),
            row("impl", "impl", "Rusty"),
        ]
    );
    // The query matches a name, a prefix or a description.
    s.cx.simulate_input("unit");
    s.draw();
    assert_eq!(rows(&mut s), [row("Test", "test, unit", "A test")]);

    // Enter inserts it at the caret, indented to its row, in a snippet
    // session: its first placeholder selected, Tab walking on.
    s.cx.simulate_keystrokes("enter");
    s.draw();
    assert!(insert_picker(&mut s).is_none(), "it closes");
    let text = |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    let selection =
        |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().primary_selection().range);
    let in_snippet = |s: &mut Lsp| editor.read_with(&s.cx, |view, _| view.model().in_snippet());
    let expanded = "fn main() {\n    #[test]\n    fn name() {\n        \n    }\n}\n";
    assert_eq!(text(&mut s), expanded);
    assert!(in_snippet(&mut s));
    let name = expanded.find("name").unwrap();
    assert_eq!(selection(&mut s), name..name + "name".len());
    // The editor has the keys again.
    s.cx.simulate_input("works");
    s.cx.simulate_keystrokes("tab");
    s.draw();
    let expanded = "fn main() {\n    #[test]\n    fn works() {\n        \n    }\n}\n";
    assert_eq!(text(&mut s), expanded);
    let body = expanded.find("works() {\n").unwrap() + "works() {\n        ".len();
    assert_eq!(selection(&mut s), body..body);

    // A Go file: Go's, and the global one.
    let go = open_go(&mut s);
    s.run("workbench.insert_snippet");
    assert_eq!(
        rows(&mut s),
        [
            row("Todo", "todo", "User snippet"),
            row("func", "func", "Gopher")
        ]
    );
    // Escape leaves the text as it was.
    s.cx.simulate_keystrokes("escape");
    s.draw();
    assert!(insert_picker(&mut s).is_none());
    assert_eq!(text_of(&mut s, &go), "package main\n");

    // A read-only editor takes none.
    s.cx.update(|_, cx| go.update(cx, |view, cx| view.set_read_only(true, cx)));
    s.run("workbench.insert_snippet");
    assert!(insert_picker(&mut s).is_none());
}

fn text_of(s: &mut Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn open_go(s: &mut Lsp) -> Entity<EditorView> {
    open_at(s, "main.go", "package main\n", 0)
}

/// The path of the active tab's file.
fn active_path(s: &mut Lsp) -> Option<PathBuf> {
    s.workbench.read_with(&s.cx, |w, cx| {
        let pane = w.pane().read(cx);
        pane.active().and_then(|ix| pane.tabs()[ix].path.clone())
    })
}

/// Chooses `language`'s row (`None`: the global file) in Configure User
/// Snippets, opening it, and gives the rows as they were.
fn configure(s: &mut Lsp, language: Option<&str>) -> Vec<ConfigureRow> {
    s.run("workbench.configure_snippets");
    let picker = configure_picker(s).expect("Configure User Snippets shows");
    let rows = picker.read_with(&s.cx, |picker, _| picker.delegate.rows());
    let ix = (rows.iter())
        .position(|row| row.language == language)
        .expect("a row for the language");
    s.cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(ix, window, cx)));
    s.draw();
    assert!(configure_picker(s).is_none(), "it closes");
    rows
}

#[gpui::test]
fn configure_creates_and_opens_the_language_file(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let dir = snippets_dir(s.config.path());
    let rust = dir.join("rust.json");
    assert!(!dir.exists());

    // The global file first, then the languages: none has a file yet.
    let rows = configure(&mut s, Some("rust"));
    assert_eq!(rows[0].language, None);
    assert_eq!(rows[0].label.as_ref(), "New Global Snippets file");
    assert!(rows.iter().all(|row| !row.exists));
    let rust_row = rows
        .iter()
        .find(|row| row.language == Some("rust"))
        .unwrap();
    assert_eq!(rust_row.label.as_ref(), "Rust");

    // Created, with a commented example that loads as no snippets, and
    // opened in a tab.
    let written = fs::read_to_string(&rust).unwrap();
    assert!(written.contains("// Snippets for rust"), "{written}");
    assert!(written.contains("\"prefix\": \"log\""), "{written}");
    let loaded = load_snippets(&dir, &crate::snippet_body_error);
    assert_eq!(loaded, Default::default());
    let opened = active_path(&mut s).expect("a file tab");
    assert_eq!(
        fs::canonicalize(opened).unwrap(),
        fs::canonicalize(&rust).unwrap()
    );

    // An existing file isn't overwritten, and is listed first, as existing.
    let go = dir.join("go.json");
    let mine = r#"{ "Mine": { "prefix": "mine", "body": "mine" } }"#;
    fs::write(&go, mine).unwrap();
    let rows = configure(&mut s, Some("go"));
    let existing: Vec<_> = (rows.iter())
        .filter(|row| row.exists)
        .map(|row| row.language)
        .collect();
    assert_eq!(existing, [Some("go"), Some("rust")]);
    assert_eq!(rows[1].language, Some("go"));
    assert_eq!(fs::read_to_string(&go).unwrap(), mine);
    let opened = active_path(&mut s).expect("a file tab");
    assert_eq!(
        fs::canonicalize(opened).unwrap(),
        fs::canonicalize(&go).unwrap()
    );

    // Rust's, chosen again, is left as the user edited it.
    fs::write(&rust, "{}\n").unwrap();
    configure(&mut s, Some("rust"));
    assert_eq!(fs::read_to_string(&rust).unwrap(), "{}\n");

    // The global file.
    configure(&mut s, None);
    let global = dir.join("global.code-snippets");
    let written = fs::read_to_string(&global).unwrap();
    assert!(written.contains("\"scope\""), "{written}");
    let opened = active_path(&mut s).expect("a file tab");
    assert_eq!(
        fs::canonicalize(opened).unwrap(),
        fs::canonicalize(&global).unwrap()
    );
}
