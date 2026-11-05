// SPDX-License-Identifier: Apache-2.0

//! The config service in headless GPUI, fed batches of changed files as
//! the watcher would send them, and once from a real watcher.

use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use gpui::{
    AppContext as _, Context, IntoElement, Render, Subscription, TestAppContext, Window, div,
};
use kiln_core::{KeymapLayer, KeymapStore, MenuBar, Os, SettingsStore, Severity};
use kiln_platform::{
    command_actions::Command as _,
    commands,
    config::{self, BatchSender, ConfigDiagnostics, ConfigService, GlobalSettings, batch_channel},
    keys,
    menus::{self, KEY_CONTEXT, MenuCommand},
};
use serde::{Deserialize, Serialize};

mod pad {
    kiln_platform::commands!(
        pad,
        [
            /// Save
            Save,
        ]
    );
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct PadSettings {
    font_size: f64,
    title: String,
}

impl Default for PadSettings {
    fn default() -> Self {
        Self {
            font_size: 13.0,
            title: "Pad".into(),
        }
    }
}

/// A directory under the system's temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        // Canonical, as the watcher names files by the paths it's given
        // and tests compare them.
        Self(dir.canonicalize().unwrap())
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// Starts a service with `settings` from the user file at `path`, fed by
/// the sender returned.
fn start_settings(path: &Path, cx: &mut TestAppContext) -> BatchSender {
    let (sender, batches) = batch_channel();
    cx.update(|cx| {
        let store = SettingsStore::<PadSettings>::load(Some(path.to_owned()), None);
        ConfigService::new()
            .settings(store)
            .start_with(batches, cx)
            .detach();
    });
    sender
}

fn settings(cx: &mut TestAppContext) -> PadSettings {
    cx.update(|cx| PadSettings::clone(&config::settings::<PadSettings>(cx)))
}

/// Writes `text` to `path` and sends a batch naming it.
fn save(path: &Path, text: &str, sender: &BatchSender, cx: &mut TestAppContext) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.to_owned()]);
    cx.run_until_parked();
}

#[gpui::test]
fn settings_reload_and_keep_the_last_good_on_invalid_toml(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.file("settings.toml", "font_size = 14\n");
    let sender = start_settings(&path, cx);
    cx.run_until_parked();
    assert_eq!(settings(cx).font_size, 14.0);
    assert_eq!(cx.update(|cx| config::diagnostics(cx).to_vec()), []);

    save(&path, "font_size = 16\n", &sender, cx);
    assert_eq!(settings(cx).font_size, 16.0);
    let previous = cx.update(|cx| {
        cx.global::<GlobalSettings<PadSettings>>()
            .previous()
            .clone()
    });
    assert_eq!(previous.font_size, 14.0);

    save(&path, "font_size = \n", &sender, cx);
    assert_eq!(settings(cx).font_size, 16.0, "the last good is kept");
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    let [error] = &diagnostics[..] else {
        panic!("one diagnostic: {diagnostics:?}");
    };
    assert_eq!(error.severity, Severity::Error);
    assert_eq!(error.file, path);
    let own = cx.update(|cx| {
        cx.global::<GlobalSettings<PadSettings>>()
            .diagnostics()
            .to_vec()
    });
    assert_eq!(own, diagnostics);

    save(&path, "font_size = 18\n", &sender, cx);
    assert_eq!(settings(cx).font_size, 18.0);
    assert_eq!(cx.update(|cx| config::diagnostics(cx).to_vec()), []);
}

#[gpui::test]
fn a_batch_of_other_files_changes_nothing(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.file("settings.toml", "title = \"One\"\n");
    let other = dir.file("other.toml", "");
    let (sender, batches) = batch_channel();
    let calls = Rc::new(Cell::new(0));
    let notified = Rc::new(Cell::new(0));
    let _subscriptions: Vec<Subscription> = cx.update(|cx| {
        let store = SettingsStore::<PadSettings>::load(Some(path.clone()), None);
        let theme = dir.file("theme.toml", "");
        let seen = calls.clone();
        ConfigService::new()
            .settings(store)
            .on_change(vec![theme], move |_, _| seen.set(seen.get() + 1))
            .start_with(batches, cx)
            .detach();
        let settings = notified.clone();
        let diagnostics = notified.clone();
        vec![
            cx.observe_global::<GlobalSettings<PadSettings>>(move |_| {
                settings.set(settings.get() + 1)
            }),
            cx.observe_global::<ConfigDiagnostics>(move |_| diagnostics.set(diagnostics.get() + 1)),
        ]
    });
    cx.run_until_parked();

    // The settings file changed, but the batch doesn't name it.
    fs::write(&path, "title = \"Two\"\n").unwrap();
    sender.send(vec![other.clone()]);
    cx.run_until_parked();
    assert_eq!(settings(cx).title, "One");
    assert_eq!(calls.get(), 0);
    assert_eq!(notified.get(), 0);

    // A batch naming it, unchanged since, changes nothing either.
    fs::write(&path, "title = \"One\"\n").unwrap();
    sender.send(vec![path.clone(), other]);
    cx.run_until_parked();
    assert_eq!(notified.get(), 0);
}

#[gpui::test]
fn on_change_hears_only_batches_naming_its_files(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let mine = dir.file("theme.toml", "");
    let other = dir.file("other.toml", "");
    let (sender, batches) = batch_channel();
    let heard = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let heard = heard.clone();
        ConfigService::new()
            .on_change(vec![mine.clone()], move |batch, _| {
                heard.borrow_mut().push(batch.to_vec())
            })
            .start_with(batches, cx)
            .detach();
    });
    sender.send(vec![other.clone()]);
    sender.send(vec![other.clone(), mine.clone()]);
    cx.run_until_parked();
    assert_eq!(*heard.borrow(), [vec![other, mine]]);
}

/// A view that follows the settings as an app's would: it observes them,
/// and reads them as it renders.
struct Pad {
    seen: Vec<f64>,
    rendered: f64,
    _settings: Subscription,
}

impl Pad {
    fn new(cx: &mut Context<Self>) -> Self {
        let settings = cx.observe_global::<GlobalSettings<PadSettings>>(|pad, cx| {
            let font_size = config::settings::<PadSettings>(cx).font_size;
            pad.seen.push(font_size);
        });
        Self {
            seen: Vec::new(),
            rendered: 0.0,
            _settings: settings,
        }
    }
}

impl Render for Pad {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rendered = config::settings::<PadSettings>(cx).font_size;
        div()
    }
}

#[gpui::test]
fn every_window_follows_a_reload(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.file("settings.toml", "font_size = 14\n");
    let sender = start_settings(&path, cx);
    let windows: Vec<_> = (0..2)
        .map(|_| cx.update(|cx| cx.open_window(Default::default(), |_, cx| cx.new(Pad::new))))
        .map(Result::unwrap)
        .collect();
    cx.run_until_parked();

    save(&path, "font_size = 20\n", &sender, cx);
    for window in &windows {
        let (seen, rendered) = window
            .read_with(cx, |pad, _| (pad.seen.clone(), pad.rendered))
            .unwrap();
        assert_eq!(seen, [20.0]);
        assert_eq!(rendered, 20.0, "every window redraws");
    }
}

const KEYS: &str = r#"
[[bindings]]
keys = "secondary-s"
command = "pad.save"
"#;

const MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "Save"
command = "pad.save"
"#;

/// `keys` with this platform's `secondary`, in GPUI's spelling.
fn gpui_keys(keys: &str) -> String {
    let secondary = if Os::current() == Os::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    gpui::Keystroke::parse(&keys.replace("secondary", secondary))
        .unwrap()
        .unparse()
}

/// The menus' GPUI key bindings for File > Save.
fn save_bindings(cx: &mut TestAppContext) -> Vec<String> {
    let action = MenuCommand {
        menu: "File".into(),
        item: "pad.save".into(),
    };
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings_for_action(&action)
            .inspect(|binding| {
                assert_eq!(binding.predicate().unwrap().to_string(), KEY_CONTEXT);
            })
            .map(|binding| binding.keystrokes()[0].inner().unparse())
            .collect()
    })
}

#[gpui::test]
fn a_keymap_reload_swaps_the_keymap_and_refreshes_the_menus(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let user = dir.file("keymap.toml", "");
    let (sender, batches) = batch_channel();
    cx.update(|cx| {
        commands::init(cx, "Pad");
        pad::register(cx);
        let catalog = commands::catalog(cx);
        let (layer, diagnostics) =
            KeymapLayer::parse(Path::new("keymap.toml"), KEYS, Some(&*catalog));
        assert_eq!(diagnostics, []);
        let store = KeymapStore::with_default(
            layer,
            diagnostics,
            Some(user.clone()),
            Some(Box::new(catalog)),
        );
        ConfigService::new()
            .keymap(store)
            .start_with(batches, cx)
            .detach();
        let (bar, diagnostics) = MenuBar::parse(Path::new("menus.toml"), MENUS, None);
        assert_eq!(diagnostics, []);
        menus::install(cx, bar.unwrap());
    });
    cx.run_until_parked();
    assert_eq!(save_bindings(cx), [gpui_keys("secondary-s")]);

    let rebind = r#"
        [[bindings]]
        keys = "alt-secondary-s"
        command = "pad.save"

        [[bindings]]
        keys = "secondary-s"
        command = ""
    "#;
    save(&user, rebind, &sender, cx);
    assert_eq!(save_bindings(cx), [gpui_keys("alt-secondary-s")]);
    // Key input matches against the new keymap: one binding, the user's.
    let keymap = cx.update(|cx| keys::current_keymap(cx));
    let bound = keymap.bindings_for(&pad::Save::id());
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].location.file, user);

    // A binding to a command the app doesn't have is a warning, kept for
    // the app to show; the rest of the file applies.
    save(
        &user,
        "[[bindings]]\nkeys = \"secondary-j\"\ncommand = \"pad.nothing\"\n",
        &sender,
        cx,
    );
    assert_eq!(save_bindings(cx), [gpui_keys("secondary-s")]);
    let diagnostics = cx.update(|cx| config::diagnostics(cx).to_vec());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
}

#[gpui::test]
fn start_creates_the_user_files_directory(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.0.join("config").join("settings.toml");
    cx.update(|cx| {
        let store = SettingsStore::<PadSettings>::load(Some(path.clone()), None);
        // The watcher's thread isn't the test's, so its batches would wake
        // the task from elsewhere: the task isn't run here.
        drop(ConfigService::new().settings(store).start(cx).unwrap());
    });
    assert!(path.parent().unwrap().is_dir());
    assert_eq!(settings(cx), PadSettings::default());
}

/// An upper bound only, sized for a loaded CI host: FSEvents may take a while.
const TIMEOUT: Duration = Duration::from_secs(60);

/// The one test on a real watcher.
#[gpui::test]
fn the_watcher_reloads_settings_saved_on_disk(cx: &mut TestAppContext) {
    // The watcher's thread wakes the service's task.
    cx.executor().allow_parking();
    let dir = TempDir::new();
    let path = dir.file("settings.toml", "font_size = 14\n");
    let task = cx.update(|cx| {
        let store = SettingsStore::<PadSettings>::load(Some(path.clone()), None);
        ConfigService::new().settings(store).start(cx).unwrap()
    });
    // Saved as editors do: a new file renamed over the old one.
    let temp = dir.0.join("settings.toml.tmp");
    fs::write(&temp, "font_size = 22\n").unwrap();
    fs::rename(&temp, &path).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while settings(cx).font_size != 22.0 {
        assert!(Instant::now() < deadline, "no reload in {TIMEOUT:?}");
        std::thread::sleep(Duration::from_millis(20));
        cx.run_until_parked();
    }
    drop(task);
}
