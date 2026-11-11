//! The Settings page in B3 (`docs/design/design-system.md`, "Settings";
//! T569): the serif title, the You / This project switch, a search that
//! takes keys, the section index's marks on changed sections, the Common
//! section first with its theme cards, and every row's label, key, one
//! help line and 26 px control, with an accent dot and Reset when changed.

use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use fe_config::{SettingKind, SettingsModel};
use gpui::{
    Bounds, DevicePixels, Entity, Focusable as _, Modifiers, NoopTextSystem, Pixels,
    PlatformTextSystem, TestAppContext, TestDispatcher, VisualTestContext, px, size,
};

use crate::{
    Launch, Workbench,
    open::ConfigDirOverride,
    settings_editor::{COMMON, CONTROL_HEIGHT, Row, Scope, SettingsEditor, ThemeCard},
};

/// Where a test run in its own process finds its directories.
const OWN_PROCESS_DIRS: &str = "FE_SETTINGS_B3_TEST_DIRS";

/// A project folder and a config directory under one temporary base.
struct Dirs {
    _temp: Option<tempfile::TempDir>,
    root: PathBuf,
    config: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(temp.path()).unwrap();
        Self {
            _temp: Some(temp),
            ..Self::at(&base)
        }
    }

    /// The directories under `base`, made if they aren't there.
    fn at(base: &Path) -> Self {
        let (root, config) = (base.join("project"), base.join("config"));
        fs::create_dir_all(root.join(".tenmoku")).unwrap();
        fs::create_dir_all(&config).unwrap();
        Self {
            _temp: None,
            root,
            config,
        }
    }

    fn user(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    fn workspace(&self) -> PathBuf {
        fe_config::project_settings_path(&self.root)
    }
}

/// Runs the test `name` of this module again, in a process of its own
/// whose `TENMOKU_CONFIG_DIR` is a temporary config directory, so it can
/// write the user's settings, and checks that it passed: `None` here, once
/// it has. There, it's `Some` with the directories it runs in.
fn in_own_process(name: &str) -> Option<Dirs> {
    if let Some(base) = std::env::var_os(OWN_PROCESS_DIRS) {
        return Some(Dirs::at(Path::new(&base)));
    }
    let dirs = Dirs::new();
    let base = dirs.config.parent().unwrap().to_owned();
    let module = module_path!().split_once("::").unwrap().1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([&format!("{module}::{name}"), "--exact", "--test-threads=1"])
        .env("TENMOKU_CONFIG_DIR", &dirs.config)
        .env(OWN_PROCESS_DIRS, &base)
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("1 passed"), "it ran: {stdout}");
    None
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn bounds(selector: &str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let found = cx.debug_bounds(selector.to_owned().leak());
    found.unwrap_or_else(|| panic!("{selector} is drawn"))
}

fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

fn click(selector: &str, cx: &mut VisualTestContext) {
    let at = bounds(selector, cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// A workbench on `dirs`' root, 1200 by 900, whose user settings are
/// `dirs`' config directory's, with the Settings tab open.
fn open_settings<'a>(
    cx: &'a mut TestAppContext,
    dirs: &Dirs,
) -> (
    Entity<Workbench>,
    Entity<SettingsEditor>,
    &'a mut VisualTestContext,
) {
    let launch = Launch {
        root: Some(dirs.root.clone()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(dirs.config.clone())));
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    cx.simulate_resize(size(px(1200.), px(900.)));
    draw(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.open_settings_ui(window, cx)
    });
    draw(cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        tab.item().downcast::<SettingsEditor>().unwrap()
    });
    assert!(editor.read_with(cx, |editor, _| editor.model().is_some()));
    (workbench, editor, cx)
}

/// Shows what searching for `text` finds.
fn search(editor: &Entity<SettingsEditor>, text: &str, cx: &mut VisualTestContext) {
    let search = editor.read_with(cx, |editor, _| editor.search_input().clone());
    search.update(cx, |search, cx| search.clear(cx));
    cx.update(|window, cx| window.focus(&search.focus_handle(cx), cx));
    if !text.is_empty() {
        cx.simulate_input(text);
    }
    draw(cx);
}

/// The rows as text: `# Group` for a heading, the dotted key for a setting.
fn row_texts(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |editor, _| {
        let model = editor.model().unwrap();
        (editor.rows().iter())
            .map(|row| match row {
                Row::Group(title) => format!("# {title}"),
                Row::Setting(ix) => model.entries()[*ix].key_path(),
            })
            .collect()
    })
}

/// Reads the settings files again and draws what they hold.
fn reload(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.reload(cx));
    draw(cx);
}

/// What the text system laid out: each line's text, its size and its
/// font's family.
type Laid = Arc<Mutex<Vec<(String, Pixels, String)>>>;

/// The test platform's text system, which lays text out but paints no
/// glyph, keeping in `laid` each line it lays out with its size and
/// family: it numbers the fonts asked for itself, as the test platform's
/// gives every font the same ID.
fn recording_text_system(laid: Laid) -> Arc<dyn PlatformTextSystem> {
    struct Recorder {
        inner: NoopTextSystem,
        fonts: Mutex<Vec<gpui::Font>>,
        laid: Laid,
    }

    impl PlatformTextSystem for Recorder {
        fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
            self.inner.add_fonts(fonts)
        }
        fn all_font_names(&self) -> Vec<String> {
            self.inner.all_font_names()
        }
        fn font_id(&self, descriptor: &gpui::Font) -> gpui::Result<gpui::FontId> {
            let mut fonts = self.fonts.lock().unwrap();
            let ix = match fonts.iter().position(|font| font == descriptor) {
                Some(ix) => ix,
                None => {
                    fonts.push(descriptor.clone());
                    fonts.len() - 1
                }
            };
            Ok(gpui::FontId(ix + 1))
        }
        fn font_metrics(&self, font_id: gpui::FontId) -> gpui::FontMetrics {
            self.inner.font_metrics(font_id)
        }
        fn typographic_bounds(
            &self,
            font_id: gpui::FontId,
            glyph_id: gpui::GlyphId,
        ) -> gpui::Result<Bounds<f32>> {
            self.inner.typographic_bounds(font_id, glyph_id)
        }
        fn advance(
            &self,
            font_id: gpui::FontId,
            glyph_id: gpui::GlyphId,
        ) -> gpui::Result<gpui::Size<f32>> {
            self.inner.advance(font_id, glyph_id)
        }
        fn glyph_for_char(&self, font_id: gpui::FontId, ch: char) -> Option<gpui::GlyphId> {
            self.inner.glyph_for_char(font_id, ch)
        }
        fn glyph_raster_bounds(
            &self,
            params: &gpui::RenderGlyphParams,
        ) -> gpui::Result<Bounds<DevicePixels>> {
            self.inner.glyph_raster_bounds(params)
        }
        fn rasterize_glyph(
            &self,
            params: &gpui::RenderGlyphParams,
            bounds: Bounds<DevicePixels>,
        ) -> gpui::Result<(gpui::Size<DevicePixels>, Vec<u8>)> {
            self.inner.rasterize_glyph(params, bounds)
        }
        fn layout_line(
            &self,
            text: &str,
            font_size: Pixels,
            runs: &[gpui::FontRun],
        ) -> gpui::LineLayout {
            let family = (runs.first())
                .and_then(|run| {
                    let fonts = self.fonts.lock().unwrap();
                    fonts.get(run.font_id.0.checked_sub(1)?).cloned()
                })
                .map(|font| font.family.to_string())
                .unwrap_or_default();
            (self.laid.lock().unwrap()).push((text.to_owned(), font_size, family));
            self.inner.layout_line(text, font_size, runs)
        }
        fn recommended_rendering_mode(
            &self,
            font_id: gpui::FontId,
            font_size: Pixels,
        ) -> gpui::TextRenderingMode {
            self.inner.recommended_rendering_mode(font_id, font_size)
        }
    }

    Arc::new(Recorder {
        inner: NoopTextSystem::new(),
        fonts: Mutex::default(),
        laid,
    })
}

/// The page title says Settings in Instrument Serif at 30 px, on a 36 px
/// line. Built by hand rather than by `#[gpui::test]`, to give the window
/// a text system that keeps each line it lays out with its font.
#[test]
fn the_title_is_instrument_serif_30() {
    let laid = Laid::default();
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some("the_title_is_instrument_serif_30"),
        recording_text_system(laid.clone()),
    );
    {
        let dirs = Dirs::new();
        let (_workbench, _editor, cx) = open_settings(&mut cx, &dirs);
        let title = bounds("settings-title", cx);
        assert_eq!(title.size.height, px(36.), "a 36 px line: {title:?}");
        let laid = laid.lock().unwrap();
        let titles: Vec<_> = (laid.iter())
            .filter(|(text, _, _)| text == "Settings")
            .collect();
        let serif = (titles.iter()).find(|(_, size, _)| *size == px(30.));
        let serif = serif.unwrap_or_else(|| panic!("Settings at 30 px: {titles:?}"));
        assert_eq!(serif.2, fe_fonts::TITLE, "{titles:?}");
        assert_eq!(fe_fonts::TITLE, "Instrument Serif");
    }
    cx.run_until_parked();
    cx.update(|cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
}

/// The scope switch is a segmented control reading You / This project, 26
/// px tall; This project shows the project's file's values, You the
/// user's, and without a folder only You shows.
#[gpui::test]
fn the_scope_switch_reads_you_and_this_project(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, editor, cx) = open_settings(cx, &dirs);
    let scope = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.scope());
    assert!(drawn("segment:You", cx));
    assert!(drawn("segment:This project", cx));
    assert_eq!(bounds("settings-scope", cx).size.height, CONTROL_HEIGHT);
    let (you, project) = (
        bounds("segment:You", cx),
        bounds("segment:This project", cx),
    );
    assert!(
        you.right() <= project.left(),
        "You first: {you:?} {project:?}"
    );
    assert_eq!(scope(cx), Scope::User);
    click("segment:This project", cx);
    assert_eq!(scope(cx), Scope::Workspace);
    click("segment:You", cx);
    assert_eq!(scope(cx), Scope::User);
    drop((workbench, editor));
}

/// The search takes keys: `editor.font` finds the editor's font family
/// and size rows, by their keys, and draws them; while searching, the
/// Common section doesn't show.
#[gpui::test]
fn searching_a_key_finds_its_row(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    search(&editor, "editor.font", cx);
    let rows = row_texts(&editor, cx);
    let settings: Vec<&String> = rows.iter().filter(|row| !row.starts_with("# ")).collect();
    assert!(
        settings.iter().all(|key| key.starts_with("editor.font")),
        "{rows:?}"
    );
    for key in ["editor.font_family", "editor.font_size"] {
        assert!(rows.contains(&key.to_owned()), "{key} in {rows:?}");
        assert!(drawn(&format!("settings-row-{key}"), cx), "{key} drawn");
    }
    assert!(!rows.contains(&"# Common".to_owned()), "{rows:?}");
    assert!(!drawn("settings-theme-cards", cx));
    // A whole key finds its one row.
    search(&editor, "workbench.rail", cx);
    assert_eq!(row_texts(&editor, cx), ["# Workbench", "workbench.rail"]);
    assert!(drawn("settings-row-workbench.rail", cx));
}

/// The section index marks each section with a setting the selected
/// scope's file sets with an accent dot, and no other; the Common section
/// for the theme cards' `theme.mode` too.
#[gpui::test]
fn a_changed_section_has_its_index_mark(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    let sections = editor.read_with(cx, |editor, _| editor.sections().to_vec());
    for section in &sections {
        assert!(drawn(&format!("settings-nav-{}", section.title), cx));
        let mark = format!("settings-nav-mark-{}", section.title);
        assert!(!drawn(&mark, cx), "{mark}: nothing is changed");
    }
    fs::write(dirs.user(), "[terminal]\nfont_size = 15\n").unwrap();
    reload(&editor, cx);
    assert!(drawn("settings-nav-mark-Terminal", cx));
    let mark = bounds("settings-nav-mark-Terminal", cx);
    assert_eq!(mark.size, size(px(6.), px(6.)), "a dot");
    assert!(within(mark, bounds("settings-nav-Terminal", cx)));
    for title in ["Common", "Editor", "Vim", "Workbench"] {
        assert!(!drawn(&format!("settings-nav-mark-{title}"), cx), "{title}");
    }
    // The Common section's settings, and the theme cards', mark it.
    fs::write(dirs.user(), "[vim]\nenabled = true\n").unwrap();
    reload(&editor, cx);
    assert!(drawn("settings-nav-mark-Common", cx));
    assert!(!drawn("settings-nav-mark-Terminal", cx));
    assert!(
        !drawn("settings-nav-mark-Vim", cx),
        "vim.enabled is Common's"
    );
    fs::write(dirs.user(), "[theme]\nmode = \"light\"\n").unwrap();
    reload(&editor, cx);
    assert!(drawn("settings-nav-mark-Common", cx));
    assert!(drawn("settings-nav-mark-Theme", cx));
    // The project's file's, under This project.
    fs::write(dirs.user(), "").unwrap();
    fs::write(dirs.workspace(), "[editor]\ntab_size = 8\n").unwrap();
    reload(&editor, cx);
    assert!(!drawn("settings-nav-mark-Editor", cx), "You: the user's");
    click("segment:This project", cx);
    assert!(drawn("settings-nav-mark-Editor", cx));
}

/// Whether `inner` lies within `outer`.
fn within(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
        && inner.left() >= outer.left()
        && inner.right() <= outer.right()
}

/// The first section is Common: the Yohen, Hakuji and Match system cards
/// in a row under its heading, then rail position, editor font and size
/// and Vim mode, each drawn below the one before, and first in the index.
#[gpui::test]
fn the_first_section_is_common(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    let rows = row_texts(&editor, cx);
    assert_eq!(
        rows[..5],
        [
            "# Common",
            "workbench.rail",
            "editor.font_family",
            "editor.font_size",
            "vim.enabled"
        ]
    );
    assert_eq!(COMMON.len(), 4);
    let sections = editor.read_with(cx, |editor, _| editor.sections().to_vec());
    assert_eq!(
        (sections[0].title.as_str(), sections[0].count),
        ("Common", 4)
    );
    // Then the sections the most often changed first (the UI audit's SE1).
    let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
    let order = [
        "Common",
        "Editor",
        "Theme",
        "Workbench",
        "Vim",
        "Terminal",
        "LSP",
        "Tree",
        "Status Bar",
        "Chat",
        "Extensions",
        "Update",
    ];
    assert_eq!(titles[..order.len()], order);
    let heading = bounds("settings-group-Common", cx);
    let cards =
        ["dark", "light", "system"].map(|mode| bounds(&format!("settings-theme-{mode}"), cx));
    let names = [
        "workbench-settings-theme-yohen",
        "workbench-settings-theme-hakuji",
        "workbench-settings-theme-system",
    ]
    .map(|key| kiln_l10n::message(key).to_string());
    assert_eq!(names, ["Yohen", "Hakuji", "Match system"]);
    for pair in cards.windows(2) {
        assert!(pair[0].right() < pair[1].left(), "in a row: {cards:?}");
        assert_eq!(pair[0].top(), pair[1].top());
    }
    assert!(cards[0].top() >= heading.bottom(), "under the heading");
    let mut above = cards[0].bottom();
    let labels = ["Rail position", "Editor font", "Font size", "Vim mode"];
    for (key, label) in COMMON.iter().zip(labels) {
        let row = bounds(&format!("settings-row-{key}"), cx);
        assert!(row.top() >= above, "{key} below what's before it");
        above = row.bottom();
        let shown = editor.read_with(cx, |editor, _| {
            let model = editor.model().unwrap();
            let entry = model.entries().iter().find(|e| e.key_path() == *key);
            SettingsEditor::label(entry.unwrap())
        });
        assert_eq!(shown, label, "{key}");
    }
    // Default: Yohen is chosen, outlined.
    let chosen = editor.read_with(cx, |editor, cx| {
        let model = editor.model().unwrap();
        let entry = model
            .entries()
            .iter()
            .find(|e| e.key_path() == "theme.mode");
        editor.row_value(entry.unwrap(), cx).unwrap()
    });
    assert_eq!(chosen, ("dark".to_owned(), false));
}

/// A click on a theme card writes `theme.mode`: Hakuji light, Match system
/// system, and Yohen, the default, removes it. In a process of its own, as
/// it writes the user's settings.
#[gpui::test]
fn a_theme_card_sets_the_mode(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("a_theme_card_sets_the_mode") else {
        return;
    };
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    let mode = |editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, cx| {
            let model = editor.model().unwrap();
            let entry = model
                .entries()
                .iter()
                .find(|e| e.key_path() == "theme.mode");
            editor.row_value(entry.unwrap(), cx).unwrap()
        })
    };
    click("settings-theme-light", cx);
    assert_eq!(mode(&editor, cx), ("light".to_owned(), true));
    let file = fs::read_to_string(dirs.user()).unwrap();
    assert!(file.contains("mode = \"light\""), "{file}");
    click("settings-theme-system", cx);
    assert_eq!(mode(&editor, cx), ("system".to_owned(), true));
    click("settings-theme-dark", cx);
    assert_eq!(mode(&editor, cx), ("dark".to_owned(), false));
    let file = fs::read_to_string(dirs.user()).unwrap_or_default();
    let set = file
        .lines()
        .any(|line| line.trim_start().starts_with("mode"));
    assert!(!set, "the default removes it: {file}");
}

/// The control of `key`'s row, of its kind: what's 26 px tall.
fn control_selector(kind: &SettingKind, key: &str) -> Option<String> {
    let name = match kind {
        SettingKind::Bool => return None,
        SettingKind::Enum(_) | SettingKind::Choice(_) => "setting-select",
        SettingKind::Int { .. } | SettingKind::Float { .. } | SettingKind::Text { .. } => {
            "setting-input"
        }
        SettingKind::TextList => "setting-add",
        SettingKind::Other => "setting-edit",
    };
    Some(format!("{name}-{key}"))
}

/// Every setting's row, found by its key, shows its label, its key in
/// settings.toml, one line of help, its catalogue message for the key, and
/// one control 26 px tall; none has the accent dot or Reset unchanged.
#[gpui::test]
fn every_row_has_label_key_help_and_control(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    let model = SettingsModel::new(None, None);
    for entry in model.entries() {
        let key = entry.key_path();
        search(&editor, &key, cx);
        let row = bounds(&format!("settings-row-{key}"), cx);
        for part in ["settings-label", "settings-key", "settings-description"] {
            let part = bounds(&format!("{part}-{key}"), cx);
            assert!(within(part, row), "{key}: {part:?} in {row:?}");
        }
        assert_eq!(
            bounds(&format!("settings-description-{key}"), cx)
                .size
                .height,
            px(14.),
            "{key}: one line of help"
        );
        // The catalogue's message for the key, its first paragraph; for
        // a setting the schema doesn't describe, its table's.
        let mut message = kiln_l10n::message(&format!("settings-{}", key.replace('.', "-")));
        if entry.description.is_empty() {
            let table = &key[..key.rfind('.').unwrap()];
            message = kiln_l10n::message(&format!("settings-{}", table.replace('.', "-")));
        }
        let help = crate::settings_editor::help_line_for_tests(&message);
        assert!(!help.is_empty(), "{key} has help");
        let shown = editor.read_with(cx, |editor, _| {
            let entry = editor
                .model()
                .unwrap()
                .entries()
                .iter()
                .find(|e| e.key_path() == key);
            editor.help(entry.unwrap())
        });
        assert_eq!(shown, help, "{key}");
        // Its label is the catalogue's message for the key.
        let label_key = format!("workbench-settings-label-{}", key.replace('.', "-"));
        let message = kiln_l10n::try_format(&label_key, &kiln_l10n::Args::new());
        let message = message.unwrap_or_else(|| panic!("{label_key} is in the catalogue"));
        assert_eq!(SettingsEditor::label(entry), message.text, "{key}");
        let value = bounds(&format!("settings-value-{key}"), cx);
        assert_eq!(value.size.height, CONTROL_HEIGHT, "{key}: one control");
        assert!(within(value, row), "{key}: {value:?} in {row:?}");
        if let Some(control) = control_selector(&entry.kind, &key) {
            let control = bounds(&control, cx);
            assert_eq!(control.size.height, CONTROL_HEIGHT, "{key}: {control:?}");
        }
        assert!(!drawn(&format!("settings-modified-{key}"), cx), "{key}");
        assert!(!drawn(&format!("setting-reset-{key}"), cx), "{key}");
    }
}

/// A row the selected scope's file sets has the accent dot before its
/// label and Reset after its key; Reset removes it from the file, and the
/// dot and Reset go.
#[gpui::test]
fn a_changed_row_has_its_dot_and_reset(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let key = "editor.font_size";
    fs::write(dirs.workspace(), "[editor]\nfont_size = 15\n").unwrap();
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    assert!(!drawn(&format!("settings-modified-{key}"), cx), "You");
    click("segment:This project", cx);
    let dot = bounds(&format!("settings-modified-{key}"), cx);
    let label = bounds(&format!("settings-label-{key}"), cx);
    let reset = bounds(&format!("setting-reset-{key}"), cx);
    let shown = bounds(&format!("settings-key-{key}"), cx);
    assert_eq!(dot.size, size(px(6.), px(6.)), "a dot");
    assert!(dot.right() <= label.left(), "before the label");
    assert!(reset.left() >= shown.right(), "after the key");
    let row = bounds(&format!("settings-row-{key}"), cx);
    assert!(within(reset, row));
    click(&format!("setting-reset-{key}"), cx);
    draw(cx);
    let file = fs::read_to_string(dirs.workspace()).unwrap();
    assert!(!file.contains("font_size"), "{file}");
    assert!(!drawn(&format!("settings-modified-{key}"), cx));
    assert!(!drawn(&format!("setting-reset-{key}"), cx));
    let shown = editor.read_with(cx, |editor, cx| {
        let model = editor.model().unwrap();
        let entry = model.entries().iter().find(|e| e.key_path() == key);
        editor.row_value(entry.unwrap(), cx).unwrap()
    });
    assert_eq!(shown, ("13.0".to_owned(), false), "the default");
}

/// The settings the user's file sets: its lines with a key, not its
/// comments or table headers.
fn theme_file(dirs: &Dirs) -> String {
    let file = fs::read_to_string(dirs.user()).unwrap_or_default();
    (file.lines())
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && line.contains('='))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A theme card stands for Tenmoku's own theme: with `theme.name`,
/// `theme.dark` or `theme.light` showing another, no card is the one shown,
/// and picking one removes what would show another theme in its place. In
/// a process of its own, as it writes the user's settings.
#[gpui::test]
fn a_theme_card_clears_what_overrides_it(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("a_theme_card_clears_what_overrides_it") else {
        return;
    };
    let chosen = |editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, cx| editor.chosen_theme_card(cx))
    };
    // A bundled dark theme by name: Yohen isn't what dark shows.
    fs::write(dirs.user(), "[theme]\nname = \"Tenmoku Ember\"\n").unwrap();
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    assert_eq!(chosen(&editor, cx), None);
    assert!(!drawn("settings-theme-ring-dark", cx));
    click("settings-theme-dark", cx);
    assert_eq!(chosen(&editor, cx), Some(ThemeCard::Yohen));
    assert_eq!(theme_file(&dirs), "");

    // An installed light theme in the slot, light mode: not Hakuji.
    fs::write(
        dirs.user(),
        "[theme]\nmode = \"light\"\nlight = \"acme.solarized/solarized-light\"\n",
    )
    .unwrap();
    reload(&editor, cx);
    assert_eq!(chosen(&editor, cx), None);
    click("settings-theme-light", cx);
    assert_eq!(chosen(&editor, cx), Some(ThemeCard::Hakuji));
    assert_eq!(theme_file(&dirs), "mode = \"light\"");

    // Match system with both overridden: a dark slot, and a light theme by
    // name: both go.
    fs::write(
        dirs.user(),
        "[theme]\ndark = \"acme.x/dark\"\nname = \"Tenmoku Light\"\n",
    )
    .unwrap();
    reload(&editor, cx);
    click("settings-theme-system", cx);
    assert_eq!(chosen(&editor, cx), Some(ThemeCard::System));
    assert_eq!(theme_file(&dirs), "mode = \"system\"");

    // A slot takes an installed theme's ID: Yohen's and Hakuji's are
    // `builtin/yohen` and `builtin/hakuji`, which keep Tenmoku's own,
    // whatever the name says, as the app chooses them.
    fs::write(
        dirs.user(),
        "[theme]\ndark = \"builtin/yohen\"\nname = \"Tenmoku Ember\"\n",
    )
    .unwrap();
    reload(&editor, cx);
    assert_eq!(chosen(&editor, cx), Some(ThemeCard::Yohen));
    fs::write(
        dirs.user(),
        "[theme]\nmode = \"light\"\nlight = \"builtin/hakuji\"\n",
    )
    .unwrap();
    reload(&editor, cx);
    assert_eq!(chosen(&editor, cx), Some(ThemeCard::Hakuji));

    // A name in a slot is no installed theme's ID, so the app doesn't show
    // Yohen or Hakuji for it: no card is shown, and picking one removes it.
    fs::write(dirs.user(), "[theme]\ndark = \"yohen\"\n").unwrap();
    reload(&editor, cx);
    assert_eq!(chosen(&editor, cx), None);
    click("settings-theme-dark", cx);
    assert_eq!(chosen(&editor, cx), Some(ThemeCard::Yohen));
    assert_eq!(theme_file(&dirs), "");
    fs::write(
        dirs.user(),
        "[theme]\nmode = \"light\"\nlight = \"Hakuji\"\n",
    )
    .unwrap();
    reload(&editor, cx);
    assert_eq!(chosen(&editor, cx), None);
    click("settings-theme-light", cx);
    assert_eq!(chosen(&editor, cx), Some(ThemeCard::Hakuji));
    assert_eq!(theme_file(&dirs), "mode = \"light\"");
}

/// The theme cards take the keys: Down from the search selects their row,
/// the card shown gets the focus ring, and Left, Right, Home and End pick
/// the card before or after it, or the first or last, as a radio group's
/// do; Down goes on to the first setting. In a process of its own, as it
/// writes the user's settings.
#[gpui::test]
fn the_theme_cards_take_the_keys(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("the_theme_cards_take_the_keys") else {
        return;
    };
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    let chosen = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, cx| editor.chosen_theme_card(cx))
    };
    let press = |keys: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(keys);
        draw(cx);
    };
    press("down", cx);
    assert!(editor.read_with(cx, |editor, _| editor.cards_selected()));
    assert!(drawn("settings-theme-ring-dark", cx));
    let ring = bounds("settings-theme-ring-dark", cx);
    assert!(
        within(bounds("settings-theme-dark", cx), ring),
        "around the card"
    );
    press("right", cx);
    assert_eq!(chosen(cx), Some(ThemeCard::Hakuji));
    assert!(drawn("settings-theme-ring-light", cx));
    assert!(!drawn("settings-theme-ring-dark", cx));
    press("end", cx);
    assert_eq!(chosen(cx), Some(ThemeCard::System));
    press("right", cx);
    assert_eq!(chosen(cx), Some(ThemeCard::System), "the last stays");
    press("left", cx);
    assert_eq!(chosen(cx), Some(ThemeCard::Hakuji));
    press("home", cx);
    assert_eq!(chosen(cx), Some(ThemeCard::Yohen));
    assert_eq!(
        theme_file(&dirs),
        "",
        "Yohen, the default, removes the mode"
    );
    // On to the settings, and back.
    press("down", cx);
    assert!(!editor.read_with(cx, |editor, _| editor.cards_selected()));
    assert!(!drawn("settings-theme-ring-dark", cx));
    press("up", cx);
    assert!(editor.read_with(cx, |editor, _| editor.cards_selected()));
}

/// Under This project the theme can't be set: the cards take no click and
/// no key.
#[gpui::test]
fn the_theme_cards_are_off_for_a_project(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    click("segment:This project", cx);
    click("settings-theme-light", cx);
    cx.simulate_keystrokes("right");
    draw(cx);
    assert!(!dirs.workspace().exists() || !theme_in(&dirs.workspace()));
    let chosen = editor.read_with(cx, |editor, cx| editor.chosen_theme_card(cx));
    assert_eq!(chosen, Some(ThemeCard::Yohen));
}

fn theme_in(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|text| text.contains("[theme]"))
}

/// In a list too short for the Common section's heading and a row, Up onto
/// the rail position's row leaves it in view, not its heading with the
/// cards: the heading's height is the one drawn.
#[gpui::test]
fn going_up_keeps_the_row_in_view_under_a_tall_heading(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    cx.simulate_resize(size(px(1200.), px(330.)));
    draw(cx);
    let list = bounds("settings-list", cx);
    let heading = bounds("settings-group-Common", cx);
    assert!(
        list.size.height < heading.size.height + px(74.) + px(64.),
        "too short for the heading, the cards and a row: {list:?}"
    );
    let focus = editor.read_with(cx, |editor, _| editor.list_focus().clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    for _ in 0..3 {
        cx.simulate_keystrokes("down");
        draw(cx);
    }
    cx.simulate_keystrokes("up");
    draw(cx);
    let selected = editor.read_with(cx, |editor, _| editor.selected());
    assert_eq!(selected, Some(1), "the rail position's row");
    let list = bounds("settings-list", cx);
    let row = bounds("settings-row-workbench.rail", cx);
    assert!(
        row.top() >= list.top() && row.bottom() <= list.bottom(),
        "{row:?} in view of {list:?}"
    );
}

/// Whether `inner` lies, up and down, within `outer`.
fn shown_within(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.top() >= outer.top() && inner.bottom() <= outer.bottom()
}

/// In a list short and narrow enough that the cards wrap one to a line and
/// don't all show, the keys bring the card they pick, and its focus ring,
/// into view: entering the cards from below, and End and Home. In a process
/// of its own, as it writes the user's settings.
#[gpui::test]
fn the_keys_show_the_card_they_pick(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("the_keys_show_the_card_they_pick") else {
        return;
    };
    let (_workbench, editor, cx) = open_settings(cx, &dirs);
    cx.simulate_resize(size(px(560.), px(400.)));
    draw(cx);
    let (dark, system) = (
        bounds("settings-theme-dark", cx),
        bounds("settings-theme-system", cx),
    );
    assert!(system.top() > dark.bottom(), "wrapped: {dark:?} {system:?}");
    let list = bounds("settings-list", cx);
    assert!(
        list.size.height < system.bottom() - dark.top(),
        "too short for them all: {list:?}"
    );
    let press = |keys: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(keys);
        draw(cx);
        draw(cx);
    };
    let assert_ring = |mode: &str, cx: &mut VisualTestContext| {
        let ring = bounds(&format!("settings-theme-ring-{mode}"), cx);
        let list = bounds("settings-list", cx);
        assert!(shown_within(ring, list), "{mode}: {ring:?} in {list:?}");
    };
    // Into the list, down to the first settings, and back up to the cards.
    let focus = editor.read_with(cx, |editor, _| editor.list_focus().clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    press("down", cx);
    press("down", cx);
    press("down", cx);
    press("up", cx);
    press("up", cx);
    assert!(editor.read_with(cx, |editor, _| editor.cards_selected()));
    assert_ring("dark", cx);
    press("end", cx);
    let chosen = editor.read_with(cx, |editor, cx| editor.chosen_theme_card(cx));
    assert_eq!(chosen, Some(ThemeCard::System));
    assert_ring("system", cx);
    press("home", cx);
    assert_ring("dark", cx);
    press("right", cx);
    assert_ring("light", cx);
}
