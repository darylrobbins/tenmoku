// SPDX-License-Identifier: Apache-2.0

//! An app's own tables in the user's theme file (`Config::theme_extensions`):
//! kept as written for the app to read, and reloaded with the rest of the
//! file. The batches are sent as the watcher would send them.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::TestAppContext;
use kiln::{
    Config,
    core::Diagnostic,
    platform::config::{self, BatchSender, batch_channel},
    prelude::*,
    ui::ThemeMode,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-theme-extensions-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Sets the app up with `config`, returning its diagnostics and a sender
/// for its files' changes.
fn init(config: Config, cx: &mut TestAppContext) -> (Vec<Diagnostic>, BatchSender) {
    let (sender, batches) = batch_channel();
    let diagnostics = cx.update(|cx| kiln::init(cx, config.watch_with(batches)));
    cx.run_until_parked();
    (diagnostics, sender)
}

fn save(path: &Path, text: &str, sender: &BatchSender, cx: &mut TestAppContext) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.to_owned()]);
    cx.run_until_parked();
}

/// The active theme's `name` table's `key`, as written.
fn read(name: &str, key: &str, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| {
        let table = cx.theme().data().extension(name)?;
        Some(table.get(key)?.as_str()?.to_owned())
    })
}

fn theme(editor_background: &str) -> String {
    format!(
        "kiln_theme = 1\nappearance = \"dark\"\n[accent]\nbase = \"#ff0000\"\n\
         [editor]\nbackground = \"{editor_background}\"\n[syntax]\nkeyword = \"#c678dd\"\n"
    )
}

#[gpui::test]
fn the_apps_tables_come_from_the_users_theme_and_hot_reload(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.path("theme.toml");
    fs::write(&path, theme("#101010")).unwrap();
    let config = Config::new("com.example.Notes", "Notes")
        .theme_mode(ThemeMode::Dark)
        .user_theme(&path)
        .theme_extensions(&["editor", "syntax"]);
    let (diagnostics, sender) = init(config, cx);
    assert_eq!(diagnostics, []);
    assert_eq!(read("editor", "background", cx).as_deref(), Some("#101010"));
    assert_eq!(read("syntax", "keyword", cx).as_deref(), Some("#c678dd"));

    // A change to an app's table alone is a theme change.
    save(&path, &theme("#202020"), &sender, cx);
    assert_eq!(read("editor", "background", cx).as_deref(), Some("#202020"));
    assert_eq!(cx.update(|cx| config::diagnostics(cx).to_vec()), []);

    // kiln's own light theme has no app tables: the app uses its defaults.
    cx.update(|cx| cx.set_theme_mode(ThemeMode::Light));
    assert_eq!(read("editor", "background", cx), None);
    cx.update(|cx| cx.set_theme_mode(ThemeMode::Dark));
    assert_eq!(read("editor", "background", cx).as_deref(), Some("#202020"));

    // Nor has kiln's dark, back once the file is gone.
    fs::remove_file(&path).unwrap();
    sender.send(vec![path.clone()]);
    cx.run_until_parked();
    assert_eq!(read("editor", "background", cx), None);
}

#[gpui::test]
fn a_table_the_app_didnt_name_is_an_unknown_key(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.path("theme.toml");
    fs::write(&path, theme("#101010")).unwrap();
    let config = Config::new("com.example.Notes", "Notes")
        .theme_mode(ThemeMode::Dark)
        .user_theme(&path)
        .theme_extensions(&["editor"]);
    let (diagnostics, _sender) = init(config, cx);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0].message.contains("unknown"),
        "{diagnostics:?}"
    );
    assert_eq!(read("editor", "background", cx).as_deref(), Some("#101010"));
    assert_eq!(read("syntax", "keyword", cx), None);
}

#[test]
#[should_panic(expected = "`surface` is a theme key of kiln's own")]
fn kilns_own_tables_cant_be_an_apps() {
    let _ = Config::new("com.example.Notes", "Notes").theme_extensions(&["editor", "surface"]);
}
