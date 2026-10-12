//! How long a saved `settings.toml`, `keymap.toml` or user theme takes to
//! apply, against M5's "TOML edits apply < 200 ms": from the rename of the
//! new file over the old one to the change being visible in the `App`,
//! through kiln's config service on a real [`kiln::platform`] watcher.
//!
//! Timing tests, so they're `#[ignore]`d. PERF.md's "Config reload (M5)"
//! has the numbers and the command:
//!
//! `cargo test --release -p fastedit --test config_reload_latency -- --ignored --nocapture --test-threads=1`

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_config::{Settings, SettingsStore};
use fe_gpui::fe_keymap;
use gpui::{App, TestAppContext};
use kiln::{
    Config, GlobalSettings,
    command::CommandId,
    platform::keys,
    prelude::*,
    ui::{Color, ThemeMode, hsla},
};

/// Saves of each file.
const SAVES: usize = 20;
/// The longest a save may take before the test gives up on it.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The user's three files in a temporary directory, with the config
/// service started on them as `main.rs`'s `config()` does.
struct Files {
    _dir: tempfile::TempDir,
    settings: PathBuf,
    keymap: PathBuf,
    theme: PathBuf,
}

fn font_size(save: usize) -> f64 {
    10.0 + save as f64
}

fn settings_text(save: usize) -> String {
    format!("[editor]\nfont_size = {}\n", font_size(save))
}

/// The key the `save`th keymap binds to `editor.select_all`.
fn key(save: usize) -> String {
    format!("ctrl-alt-{}", (b'a' + save as u8) as char)
}

fn keymap_text(save: usize) -> String {
    format!(
        "[[bindings]]\nkeys = \"{}\"\ncommand = \"editor.select_all\"\ncontext = \"editor\"\n",
        key(save)
    )
}

fn accent(save: usize) -> u32 {
    0x200000 + 0x010101 * save as u32
}

fn theme_text(save: usize) -> String {
    format!(
        "kiln_theme = 1\nappearance = \"dark\"\n[accent]\nbase = \"#{:06x}\"\n",
        accent(save)
    )
}

impl Files {
    /// Writes the first version of each file and starts the service.
    fn start(cx: &mut TestAppContext) -> Self {
        // The watcher's thread wakes the service's task.
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        // Canonical, as the watcher names files by the paths it's given.
        let root = dir.path().canonicalize().unwrap();
        let files = Self {
            settings: root.join("settings.toml"),
            keymap: root.join("keymap.toml"),
            theme: root.join("theme.toml"),
            _dir: dir,
        };
        fs::write(&files.settings, settings_text(0)).unwrap();
        fs::write(&files.keymap, keymap_text(0)).unwrap();
        fs::write(&files.theme, theme_text(0)).unwrap();
        let config = Config::new("org.fastedit.latency", "fastedit latency")
            .commands(|cx| {
                fe_gpui::init_with_keymap(cx, fe_keymap::Keymap::default());
                fe_workbench::init(cx);
            })
            .catalog(fe_workbench::command_catalog())
            .keymap(fe_keymap::DEFAULT_KEYMAP_PATH, fe_keymap::DEFAULT)
            // `main.rs`'s `log_keymap_report`: part of each keymap reload.
            .on_keymap_reload(|cx| {
                for issue in fe_gpui::keymap_report(cx) {
                    log::info!("keymap: {issue}");
                }
            })
            .menus(fe_keymap::menu::DEFAULT_MENUS_PATH, fe_keymap::MENUS)
            .menu_shortcut_context(fe_keymap::MENU_SHORTCUT_CONTEXT)
            .settings_store(SettingsStore::load(Some(files.settings.clone()), None).into_inner())
            .theme_mode(ThemeMode::Dark)
            .theme_extensions(fe_config::theme::EXTENSIONS)
            .user_theme(&files.theme)
            .user_keymap(&files.keymap);
        let diagnostics = cx.update(|cx| kiln::init(cx, config));
        assert_eq!(diagnostics, []);
        cx.run_until_parked();
        assert!(applied(Kind::Settings, 0, cx));
        assert!(applied(Kind::Keymap, 0, cx));
        assert!(applied(Kind::Theme, 0, cx));
        files
    }

    fn path(&self, kind: Kind) -> &Path {
        match kind {
            Kind::Settings => &self.settings,
            Kind::Keymap => &self.keymap,
            Kind::Theme => &self.theme,
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Settings,
    Keymap,
    Theme,
}

/// Whether the `save`th version of the file is the one in effect.
fn applied(kind: Kind, save: usize, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| match kind {
        Kind::Settings => settings_font_size(cx) == font_size(save),
        Kind::Keymap => {
            let id = CommandId::try_from("editor.select_all").unwrap();
            let want = key(save);
            keys::current_keymap(cx)
                .bindings_for(&id)
                .iter()
                .any(|binding| binding.keys.to_string() == want)
        }
        Kind::Theme => cx.theme().colors.accent.base == hsla(Color::rgb(accent(save))),
    })
}

fn settings_font_size(cx: &App) -> f64 {
    cx.global::<GlobalSettings<Settings>>()
        .settings()
        .editor
        .font_size
}

/// Saves `SAVES` new versions of `kind`'s file as editors do (a new file
/// renamed over the old one) and times each from the rename to the change
/// being visible. Prints p50 and max.
fn measure(kind: Kind, name: &str, cx: &mut TestAppContext) {
    let files = Files::start(cx);
    let path = files.path(kind);
    let temp = path.with_extension("toml.tmp");
    let mut times = Vec::new();
    for save in 1..=SAVES {
        let text = match kind {
            Kind::Settings => settings_text(save),
            Kind::Keymap => keymap_text(save),
            Kind::Theme => theme_text(save),
        };
        fs::write(&temp, text).unwrap();
        let start = Instant::now();
        fs::rename(&temp, path).unwrap();
        while !applied(kind, save, cx) {
            assert!(
                start.elapsed() < TIMEOUT,
                "{name} save {save} not applied in {TIMEOUT:?}"
            );
            std::thread::sleep(Duration::from_millis(1));
            cx.run_until_parked();
        }
        times.push(start.elapsed());
        // Let the watcher's events for this save settle, so the next
        // starts from quiet.
        std::thread::sleep(Duration::from_millis(100));
        cx.run_until_parked();
    }
    times.sort();
    let ms = |time: Duration| time.as_secs_f64() * 1000.0;
    println!(
        "config reload {name}: {SAVES} saves, p50 {:.1} ms, max {:.1} ms, min {:.1} ms",
        ms(times[SAVES / 2]),
        ms(times[SAVES - 1]),
        ms(times[0]),
    );
}

#[gpui::test]
#[ignore = "timing"]
fn settings_toml_reload_latency(cx: &mut TestAppContext) {
    measure(Kind::Settings, "settings.toml", cx);
}

#[gpui::test]
#[ignore = "timing"]
fn keymap_toml_reload_latency(cx: &mut TestAppContext) {
    measure(Kind::Keymap, "keymap.toml", cx);
}

#[gpui::test]
#[ignore = "timing"]
fn user_theme_reload_latency(cx: &mut TestAppContext) {
    measure(Kind::Theme, "theme.toml", cx);
}
