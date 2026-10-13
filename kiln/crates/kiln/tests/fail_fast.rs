// SPDX-License-Identifier: LicenseRef-Proprietary

//! `Config::fail_fast`: `init` panics on any diagnostic in the keymap and
//! menus the app ships, listing each, and never on the user's files; off,
//! as by default, it returns them.

use std::{
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::TestAppContext;
use kiln::{
    Config,
    command::CommandId,
    core::Severity,
    platform::{
        config::{BatchSender, batch_channel},
        keys,
    },
};

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

const KEYMAP: &str = r#"
[[bindings]]
keys = "secondary-n"
command = "notes.new_note"
"#;

const MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "New Note"
command = "notes.new_note"
"#;

fn config(keymap: &str, menus: &str) -> Config {
    Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .keymap("keymap.toml", keymap.to_owned())
        .menus("menus.toml", menus.to_owned())
}

/// What `init` panicked with, with `config`.
fn panic_message(config: Config, cx: &mut TestAppContext) -> String {
    let panicked = catch_unwind(AssertUnwindSafe(|| {
        cx.update(|cx| kiln::init(cx, config));
    }));
    let payload = panicked.expect_err("init panics");
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => (*payload.downcast::<&str>().unwrap()).to_owned(),
    }
}

/// A directory under the system's temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-fail-fast-{}-{}",
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
        fs::remove_dir_all(&self.0).ok();
    }
}

/// Writes `text` to `path` and sends a batch naming it.
fn save(path: &Path, text: &str, sender: &BatchSender, cx: &mut TestAppContext) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.to_owned()]);
    cx.run_until_parked();
}

/// The keys bound to `notes.new_note`, in the keymap in use.
fn new_note_keys(cx: &mut TestAppContext) -> Vec<String> {
    let id = CommandId::parse("notes.new_note").unwrap();
    cx.update(|cx| {
        keys::current_keymap(cx)
            .bindings_for(&id)
            .iter()
            .map(|binding| binding.keys.to_string())
            .collect()
    })
}

#[gpui::test]
fn an_error_in_the_apps_keymap_panics(cx: &mut TestAppContext) {
    let keymap =
        format!("{KEYMAP}\n[[bindings]]\nkeys = \"secondary-nope-x\"\ncommand = \"app.quit\"\n");
    let message = panic_message(config(&keymap, MENUS).fail_fast(true), cx);
    assert!(message.contains("1 problem"), "{message}");
    assert!(message.contains("keymap.toml:7:"), "{message}");
    assert!(message.contains("error"), "{message}");
}

#[gpui::test]
fn a_warning_in_the_apps_keymap_and_menus_panics_listing_each(cx: &mut TestAppContext) {
    let keymap =
        format!("{KEYMAP}\n[[bindings]]\nkeys = \"secondary-m\"\ncommand = \"notes.missing\"\n");
    let menus = format!("{MENUS}\n[[menus.items]]\ntitle = \"Gone\"\ncommand = \"notes.gone\"\n");
    let message = panic_message(config(&keymap, &menus).fail_fast(true), cx);
    assert!(message.contains("2 problem"), "{message}");
    let lines: Vec<&str> = message.lines().skip(1).collect();
    assert_eq!(lines.len(), 2, "{message}");
    assert!(lines[0].contains("keymap.toml:"), "{message}");
    assert!(lines[0].contains("warning"), "{message}");
    assert!(lines[0].contains("notes.missing"), "{message}");
    assert!(lines[1].contains("menus.toml:"), "{message}");
    assert!(lines[1].contains("notes.gone"), "{message}");
}

#[gpui::test]
fn the_bundled_defaults_pass_fail_fast(cx: &mut TestAppContext) {
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .keymap("keymap.toml", KEYMAP)
        .default_keys(true)
        .default_menus(true)
        .fail_fast(true);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
}

#[gpui::test]
fn the_users_keymap_never_panics_and_keeps_its_last_good_layer(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.path("keymap.toml");
    fs::write(&user, "[[bindings]\n").unwrap();
    let (sender, batches) = batch_channel();
    let config = config(KEYMAP, MENUS)
        .fail_fast(true)
        .user_keymap(&user)
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].file, user);
    assert_eq!(diagnostics[0].severity, Severity::Error);
    cx.run_until_parked();
    let secondary_n = new_note_keys(cx);
    assert_eq!(secondary_n.len(), 1, "the app's alone");

    let good = "[[bindings]]\nkeys = \"secondary-shift-n\"\ncommand = \"notes.new_note\"\n";
    save(&user, good, &sender, cx);
    assert_eq!(new_note_keys(cx).len(), 2);
    save(&user, "[[bindings]\n", &sender, cx);
    assert_eq!(new_note_keys(cx).len(), 2, "the last good layer");
    let unknown = "[[bindings]]\nkeys = \"secondary-m\"\ncommand = \"notes.missing\"\n";
    save(&user, unknown, &sender, cx);
    assert_eq!(new_note_keys(cx), secondary_n, "a warning loads the rest");
}

#[gpui::test]
fn without_fail_fast_init_returns_the_diagnostics(cx: &mut TestAppContext) {
    let keymap =
        format!("{KEYMAP}\n[[bindings]]\nkeys = \"secondary-m\"\ncommand = \"notes.missing\"\n");
    let menus = format!("{MENUS}\n[[menus.items]]\ntitle = \"Gone\"\ncommand = \"notes.gone\"\n");
    let diagnostics = cx.update(|cx| kiln::init(cx, config(&keymap, &menus)));
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics.iter().all(|d| d.severity == Severity::Warning));
    assert_eq!(new_note_keys(cx).len(), 1, "the rest load");
}
