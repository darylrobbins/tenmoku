//! Selecting a theme by name and keeping it across reloads of the themes
//! directory.

use std::{
    path::{Path, PathBuf},
    slice::from_ref,
};

use fe_config::{Color, Severity, Theme, ThemeStore};

const PAPER: &str = r##"
name = "Paper"
appearance = "light"

[editor]
background = "#fafafa"
"##;

fn write(dir: &Path, file: &str, source: &str) -> PathBuf {
    let path = dir.join(file);
    std::fs::write(&path, source).unwrap();
    path
}

fn color(text: &str) -> Color {
    text.parse().unwrap()
}

fn dark() -> &'static Theme {
    Theme::fastedit_dark()
}

#[test]
fn selects_and_switches() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "paper.toml", PAPER);

    let mut store = ThemeStore::load(Some(dir.path().to_owned()), "paper");
    assert_eq!(store.theme().name, "Paper");
    assert_eq!(store.theme().editor.background, color("#fafafa"));
    assert_eq!(store.diagnostics(), []);

    let event = store.select("fastedit dark").unwrap();
    assert_eq!(event.previous.name, "Paper");
    assert_eq!(*event.theme, *dark());
    assert_eq!(**store.theme(), *dark());
    assert_eq!(event.diagnostics, []);

    assert!(store.select("fastedit dark").is_none());
    assert!(store.select("FASTEDIT DARK").is_none());
    assert_eq!(**store.theme(), *dark());

    let event = store.select("Paper").unwrap();
    assert_eq!(*event.previous, *dark());
    assert_eq!(event.theme.name, "Paper");
    assert!(store.select("paper").is_none());
}

#[test]
fn unknown_name_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "paper.toml", PAPER);
    let only_nope_warning = |store: &ThemeStore| {
        assert_eq!(**store.theme(), *dark());
        let warnings: Vec<_> = store
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1, "{:?}", store.diagnostics());
        assert!(warnings[0].message.contains("Nope"), "{}", warnings[0]);
        assert!(
            warnings[0].message.contains("fastedit Dark"),
            "{}",
            warnings[0]
        );
        assert_eq!(store.diagnostics().len(), 1);
        (
            warnings[0].file.clone(),
            warnings[0].line,
            warnings[0].column,
        )
    };

    let store = ThemeStore::load(Some(dir.path().to_owned()), "Nope");
    assert_eq!(only_nope_warning(&store), (dir.path().to_owned(), 1, 1));

    let mut store = ThemeStore::load(Some(dir.path().to_owned()), "paper");
    let event = store.select("Nope").unwrap();
    assert_eq!(event.previous.name, "Paper");
    assert_eq!(*event.theme, *dark());
    assert_eq!(event.diagnostics, store.diagnostics());
    only_nope_warning(&store);
    // Still unknown: nothing changes.
    assert!(store.select("Nope").is_none());
    assert!(store.reload(&[dir.path().join("paper.toml")]).is_none());
    only_nope_warning(&store);

    let store = ThemeStore::load(None, "Nope");
    assert_eq!(only_nope_warning(&store), (PathBuf::new(), 1, 1));
    assert_eq!(store.paths(), Vec::<PathBuf>::new());
}

#[test]
fn reload_reads_changed_files() {
    let dir = tempfile::tempdir().unwrap();
    let paper = write(dir.path(), "paper.toml", PAPER);
    write(dir.path(), "notes.txt", "not a theme");
    let mut store = ThemeStore::load(Some(dir.path().to_owned()), "paper");
    assert_eq!(store.paths(), from_ref(&paper));

    write(
        dir.path(),
        "paper.toml",
        &PAPER.replace("#fafafa", "#eeeeee"),
    );
    // Paths the store doesn't read are ignored.
    assert!(store.reload(&[dir.path().join("notes.txt")]).is_none());
    assert!(store.reload(&[]).is_none());
    assert_eq!(store.theme().editor.background, color("#fafafa"));

    let event = store.reload(from_ref(&paper)).unwrap();
    assert_eq!(event.previous.editor.background, color("#fafafa"));
    assert_eq!(event.theme.editor.background, color("#eeeeee"));
    assert_eq!(store.theme().editor.background, color("#eeeeee"));
    // Read again with no change: no event.
    assert!(store.reload(from_ref(&paper)).is_none());

    // A theme added since the last read is found by a selection.
    let ink = write(dir.path(), "ink.toml", "name = \"Ink\"\n");
    assert!(!store.paths().contains(&ink));
    let event = store.select("ink").unwrap();
    assert_eq!(event.theme.name, "Ink");
    assert_eq!(store.paths(), [ink.clone(), paper.clone()]);

    write(
        dir.path(),
        "ink.toml",
        "name = \"Ink\"\n[ui]\naccent = \"#123456\"\n",
    );
    let event = store.reload(&[ink]).unwrap();
    assert_eq!(event.theme.ui.accent, color("#123456"));
}

#[test]
fn broken_selected_file_keeps_last_good_theme() {
    let dir = tempfile::tempdir().unwrap();
    let paper = write(dir.path(), "paper.toml", PAPER);
    let mut store = ThemeStore::load(Some(dir.path().to_owned()), "Paper");
    let good = store.theme().clone();

    write(dir.path(), "paper.toml", "name = \"Paper\"\n[editor\n");
    let event = store.reload(from_ref(&paper)).unwrap();
    assert_eq!(*event.theme, *good);
    assert_eq!(**store.theme(), *good);
    let errors: Vec<_> = store
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", store.diagnostics());
    assert_eq!(errors[0].file, paper);
    assert_eq!(event.diagnostics, store.diagnostics());
    // Selecting it again keeps it too.
    assert!(store.select("Paper").is_none());
    assert_eq!(**store.theme(), *good);

    write(
        dir.path(),
        "paper.toml",
        &PAPER.replace("#fafafa", "#f0f0f0"),
    );
    let event = store.reload(from_ref(&paper)).unwrap();
    assert_eq!(event.theme.editor.background, color("#f0f0f0"));
    assert_eq!(store.theme().name, "Paper");
    assert_eq!(store.diagnostics(), []);

    // Deleted: the theme stays, with a warning.
    std::fs::remove_file(&paper).unwrap();
    let event = store.reload(from_ref(&paper)).unwrap();
    assert_eq!(event.theme.editor.background, color("#f0f0f0"));
    assert_eq!(store.diagnostics().len(), 1);
    assert_eq!(store.diagnostics()[0].severity, Severity::Warning);
    assert!(store.diagnostics()[0].message.contains("Paper"));
    // Still watched, so writing it again (as a save that moves the old file
    // away does) is read.
    assert_eq!(store.paths(), from_ref(&paper));
    write(
        dir.path(),
        "paper.toml",
        &PAPER.replace("#fafafa", "#e0e0e0"),
    );
    let event = store.reload(from_ref(&paper)).unwrap();
    assert_eq!(event.theme.editor.background, color("#e0e0e0"));
    assert_eq!(store.diagnostics(), []);
    std::fs::remove_file(&paper).unwrap();
    store.reload(from_ref(&paper)).unwrap();

    // Choosing another name lets the old one go.
    store.select("fastedit dark").unwrap();
    assert!(store.select("paper").is_some());
    assert_eq!(**store.theme(), *dark());
}
