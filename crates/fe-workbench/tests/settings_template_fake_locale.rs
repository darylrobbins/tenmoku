//! The settings template in a fake locale, `qps-ploc`, whose every message
//! is a marker naming its key (fe-config's generated fixture): what it
//! renders keeps the commented TOML and the active lines byte for byte,
//! and every path that creates the user's `settings.toml` writes it, not
//! the English constant: Open Settings, `write_setting`, and a toggle saved
//! after the locale changed.
//!
//! The catalogue's locale is the process's, so each test takes a lock and
//! sets the locale it needs.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use fe_command::l10n::catalogue;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{USER_SETTINGS_TEMPLATE, user_settings_template};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_workbench::{ConfigDirOverride, Launch, Workbench, save_toggles_to};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

catalogue::resources! {
    "qps-ploc" => ["../../fe-config/src/tests/fixtures/qps-ploc/settings-template.ftl"],
}

/// The fake locale's messages.
const FAKE: &str =
    include_str!("../../fe-config/src/tests/fixtures/qps-ploc/settings-template.ftl");

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other test of this binary running, in
/// `locale` (en-US for `None`), and with no config directory from the
/// environment, so a workbench neither reads the user's settings nor
/// watches them.
fn serially(locale: Option<&str>) -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    for var in [
        "TENMOKU_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "HOME",
        "APPDATA",
        "USERPROFILE",
    ] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    set_locale(locale);
    guard
}

fn set_locale(locale: Option<&str>) {
    let requested: Vec<catalogue::LanguageIdentifier> =
        locale.into_iter().map(|tag| tag.parse().unwrap()).collect();
    let chain = catalogue::set_locales(&requested);
    let first = chain[0].to_string();
    assert_eq!(
        first.to_lowercase(),
        locale.unwrap_or("en-US").to_lowercase()
    );
}

/// The fake locale's keys, in order.
fn fake_keys() -> Vec<&'static str> {
    FAKE.lines()
        .filter_map(|line| line.split_once(" = ").map(|(key, _)| key))
        .filter(|key| key.starts_with("settings-template-"))
        .collect()
}

/// Whether `line` is commented TOML: `# ` then a table header or a
/// `key = value` line.
fn is_commented_toml(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("# ") else {
        return false;
    };
    let shaped = rest.starts_with('[')
        || rest.split_once('=').is_some_and(|(key, _)| {
            !key.trim().is_empty()
                && (key.trim().bytes()).all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        });
    shaped && rest.parse::<toml::Table>().is_ok()
}

fn commented_toml(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| is_commented_toml(line))
        .collect()
}

/// Each commented example, from a commented table header through the
/// commented TOML after it, uncommented and parsed.
fn examples(text: &str) -> Vec<toml::Table> {
    let mut examples = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let commented = is_commented_toml(line);
        if commented && line.starts_with("# [") {
            examples.extend(current.take());
            current = Some(String::new());
        } else if !commented && line != "#" {
            examples.extend(current.take());
            continue;
        }
        if let (Some(example), true) = (current.as_mut(), commented) {
            example.push_str(&line[2..]);
            example.push('\n');
        }
    }
    examples.extend(current);
    examples
        .iter()
        .map(|example| example.parse().unwrap_or_else(|e| panic!("{example}: {e}")))
        .collect()
}

/// `table`'s tables and keys, values aside.
fn shape(table: &toml::Table) -> Vec<String> {
    fn walk(value: &toml::Value, path: &str, out: &mut Vec<String>) {
        match value {
            toml::Value::Table(table) => {
                out.push(format!("[{path}]"));
                for (key, value) in table {
                    walk(value, &format!("{path}.{key}"), out);
                }
            }
            toml::Value::Array(items) if items.iter().all(toml::Value::is_table) => {
                for item in items {
                    out.push(format!("[[{path}]]"));
                    walk(item, path, out);
                }
            }
            _ => out.push(path.to_owned()),
        }
    }
    let mut out = Vec::new();
    walk(&toml::Value::Table(table.clone()), "", &mut out);
    out
}

/// Checks that `text` starts as the fake locale's template does, and that
/// its commented TOML is en-US's, byte for byte and in order, but for
/// `set`, the line a write uncommented.
fn assert_fake_template(text: &str, set: Option<&str>) {
    assert!(
        text.starts_with("# ⟦settings-template-intro⟧ "),
        "{}",
        &text[..200.min(text.len())]
    );
    let mut english = commented_toml(USER_SETTINGS_TEMPLATE);
    english.retain(|line| Some(*line) != set);
    assert_eq!(commented_toml(text), english);
}

#[test]
fn the_template_renders_every_block_and_keeps_its_toml() {
    let _serial = serially(Some("qps-ploc"));
    let fake = user_settings_template();
    assert_ne!(fake, USER_SETTINGS_TEMPLATE);
    assert!(
        std::ptr::eq(fake, user_settings_template()),
        "kept, not rendered again"
    );
    assert_fake_template(fake, None);

    // Every block is its marker, once, and no English prose is left.
    let keys = fake_keys();
    assert!(keys.len() > 90, "{} keys", keys.len());
    for key in &keys {
        let marker = format!("# ⟦{key}⟧ ");
        assert_eq!(fake.matches(&marker).count(), 1, "{key}");
    }
    let english: Vec<&str> = (USER_SETTINGS_TEMPLATE.lines())
        .filter(|line| line.starts_with("# ") && !is_commented_toml(line))
        .collect();
    for line in fake.lines() {
        assert!(!english.contains(&line), "{line} is still English");
        if line.starts_with('#') && !is_commented_toml(line) {
            assert!(line.chars().count() <= 80, "{line} wraps at 80 columns");
        }
    }

    // The commented examples uncomment to the same tables and keys.
    let ours: Vec<Vec<String>> = examples(fake).iter().map(shape).collect();
    let english: Vec<Vec<String>> = examples(USER_SETTINGS_TEMPLATE).iter().map(shape).collect();
    assert_eq!(ours, english);
    let flat: Vec<String> = english.concat();
    for table in [
        "[[.vim.remap]]",
        ".vim.remap.mode",
        ".vim.remap.from",
        ".vim.remap.to",
        "[[.vim.which_key_group]]",
        "[.formatters.typescript]",
        "[.formatters.rust]",
        "[.formatters.python]",
        "[.formatters.go]",
    ] {
        assert!(flat.iter().any(|item| item == table), "{table} in {flat:?}");
    }

    // The active lines parse as before: tables of defaults alone.
    let ours: toml::Table = fake.parse().unwrap();
    let english: toml::Table = USER_SETTINGS_TEMPLATE.parse().unwrap();
    assert_eq!(ours, english);
    let settings: fe_config::Settings = toml::from_str(fake).unwrap();
    let defaults: fe_config::Settings = toml::from_str("").unwrap();
    assert_eq!(format!("{settings:?}"), format!("{defaults:?}"));
}

#[test]
fn write_setting_creates_the_rendered_template() {
    let _serial = serially(Some("qps-ploc"));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tenmoku").join("settings.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fe_config::write_setting(&path, &["status_bar", "visible"], false).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert_fake_template(&text, Some("# visible = true"));
    let comments = |text: &str| -> Vec<String> {
        (text.lines())
            .filter(|line| line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    };
    let mut expected = comments(user_settings_template());
    expected.retain(|line| line != "# visible = true");
    let mut written = comments(&text);
    written.retain(|line| line != "# visible = true");
    assert_eq!(written, expected);
    let table: toml::Table = text.parse().unwrap();
    assert_eq!(table["status_bar"]["visible"].as_bool(), Some(false));
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
}

fn workbench<'a>(
    cx: &'a mut TestAppContext,
    project: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    init(cx);
    let launch = Launch {
        root: Some(project.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.run_until_parked();
    (workbench, cx)
}

/// Runs command `id` in the workbench's context, as the palette does.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

/// Runs until `path` exists, as a write on another thread makes it.
fn wait_for(path: &Path, cx: &mut VisualTestContext) {
    for _ in 0..500 {
        cx.run_until_parked();
        if path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cx.run_until_parked();
}

#[gpui::test]
fn open_settings_creates_the_rendered_template(cx: &mut TestAppContext) {
    let _serial = serially(Some("qps-ploc"));
    let project = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let dir: PathBuf = config.path().join("tenmoku");
    let file = dir.join("settings.toml");
    let (_workbench, cx) = workbench(cx, project.path());
    cx.update(|_, cx| cx.set_global(ConfigDirOverride(Some(dir))));

    dispatch("workbench.open_settings", cx);
    wait_for(&file, cx);
    let text = fs::read_to_string(&file).unwrap();
    assert_eq!(text, user_settings_template());
    assert_fake_template(&text, None);
}

#[gpui::test]
fn a_toggle_after_a_locale_change_creates_that_locales_template(cx: &mut TestAppContext) {
    let _serial = serially(None);
    let project = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let path = config.path().join("settings.toml");
    let (_workbench, cx) = workbench(cx, project.path());
    // The writer starts in en-US, before the file exists.
    cx.update(|_, cx| save_toggles_to(cx, path.clone()));
    assert_eq!(user_settings_template(), USER_SETTINGS_TEMPLATE);

    set_locale(Some("qps-ploc"));
    dispatch("view.toggle_status_bar", cx);
    wait_for(&path, cx);
    let text = fs::read_to_string(&path).unwrap();
    assert_fake_template(&text, Some("# visible = true"));
    let table: toml::Table = text.parse().unwrap();
    assert_eq!(table["status_bar"]["visible"].as_bool(), Some(false));
}
