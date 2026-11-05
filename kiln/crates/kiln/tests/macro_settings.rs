// SPDX-License-Identifier: Apache-2.0

//! `#[kiln::settings(section = "..")]`: one struct is a section of the
//! app's `settings.toml`, its `Default` from each field's `default = ..`,
//! its schema's descriptions from the fields' doc lines. Sections share
//! the file, each reading its own table, a key of another's not unknown to
//! it; and in a `kiln::app` a view reading one with `cx.setting::<S>()`
//! redraws after the file reloads.

use std::{
    cell::{Cell, RefCell},
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{
    AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    StyleRefinement, Styled as _, TestAppContext, Window, div,
};
use kiln::{
    Config, SettingsSection,
    core::Diagnostic,
    platform::config::{self, BatchSender, batch_channel},
    prelude::*,
    serde::{Deserialize, Serialize},
    testing::TestApp,
    with_cx,
};

/// `[notes]` in settings.toml.
#[kiln::settings(section = "notes")]
#[derive(Clone, Debug)]
pub struct NotesSettings {
    /// The editor's font size, in points.
    #[setting(default = 14.0)]
    pub font_size: f32,
    /// The font, by family name.
    #[setting(default = "Menlo")]
    pub font: String,
    /// Whether lines wrap.
    pub wrap: bool,
}

/// `[outline]` in settings.toml.
#[kiln::settings(section = "outline")]
#[derive(Clone, Debug)]
pub struct OutlineSettings {
    /// How deep the outline goes.
    #[setting(default = 2)]
    pub depth: u32,
    /// The marks before each level.
    pub marks: Marks,
}

/// A table within a section: `[outline.marks]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, kiln::schemars::JsonSchema)]
#[serde(crate = "kiln::serde", default)]
#[schemars(crate = "kiln::schemars")]
pub struct Marks {
    /// The mark of the top level.
    pub top: String,
}

impl Default for Marks {
    fn default() -> Self {
        Self { top: "#".into() }
    }
}

/// `[conditional]` in settings.toml: keys on some targets only.
#[kiln::settings(section = "conditional")]
pub struct ConditionalSettings {
    /// On every target this test builds for.
    #[cfg(test)]
    #[setting(default = 1)]
    pub enabled: u32,
    /// On none.
    #[cfg(any())]
    #[setting(default = 2)]
    pub disabled: u32,
    /// On none, through a `cfg_attr`.
    #[cfg_attr(all(), cfg(any()))]
    #[setting(default = 3)]
    pub disabled_by_cfg_attr: u32,
    /// On every target: its `cfg_attr` doesn't apply.
    #[cfg_attr(any(), cfg(any()))]
    #[setting(default = 4)]
    pub enabled_by_cfg_attr: u32,
    /// Renamed by a `cfg_attr` that holds no condition.
    #[cfg_attr(all(), serde(rename = "renamed_key"))]
    #[setting(default = 5)]
    pub renamed: u32,
}

#[test]
fn a_default_initializes_the_fields_their_cfg_leaves_in_alone() {
    let conditional = ConditionalSettings::default();
    assert_eq!(conditional.enabled, 1);
    assert_eq!(conditional.enabled_by_cfg_attr, 4);
    assert_eq!(conditional.renamed, 5);
    let schema = kiln::settings_schema::<ConditionalSettings>();
    let mut keys: Vec<&str> = (schema["properties"].as_object().unwrap().keys())
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["enabled", "enabled_by_cfg_attr", "renamed_key"]);
}

#[test]
fn the_default_carries_each_setting_s_default_and_the_schema_its_doc_lines() {
    let notes = NotesSettings::default();
    assert_eq!(notes.font_size, 14.0);
    assert_eq!(notes.font, "Menlo");
    assert!(!notes.wrap, "no default = .. is the type's Default");
    assert_eq!(OutlineSettings::default().depth, 2);
    assert_eq!(NotesSettings::SECTION, "notes");

    let schema = kiln::settings_schema::<NotesSettings>();
    assert_eq!(schema["description"], "`[notes]` in settings.toml.");
    let properties = schema["properties"].as_object().unwrap();
    let keys: Vec<&str> = properties.keys().map(String::as_str).collect();
    assert_eq!(keys.len(), 3, "each field, and nothing else: {keys:?}");
    for (key, description) in [
        ("font_size", "The editor's font size, in points."),
        ("font", "The font, by family name."),
        ("wrap", "Whether lines wrap."),
    ] {
        assert_eq!(properties[key]["description"], description, "{key}");
    }
    assert_eq!(properties["font_size"]["default"], 14.0);
}

/// A directory of its own under the target's, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("macro_settings-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// Writes `text` to `path` and sends a batch naming it, as the watcher
/// would.
fn save(path: &Path, text: &str, sender: &BatchSender) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.to_owned()]);
}

/// Each diagnostic's key and severity, sorted.
fn keys(diagnostics: &[Diagnostic]) -> Vec<(String, bool)> {
    let mut keys: Vec<_> = (diagnostics.iter())
        .map(|diagnostic| {
            (
                diagnostic.key.clone().unwrap_or_default(),
                diagnostic.is_error(),
            )
        })
        .collect();
    keys.sort();
    keys
}

#[gpui::test]
fn two_sections_read_their_own_tables_of_one_settings_toml(cx: &mut TestAppContext) {
    let dir = TempDir::new("sections");
    let file = dir.0.join("settings.toml");
    fs::write(
        &file,
        "[notes]\nfont_size = 18\nwrap = true\n\n[outline]\ndepth = 3\n\n[outline.marks]\ntop = \"*\"\n",
    )
    .unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .settings_file(&file)
        .section::<NotesSettings>()
        .section::<OutlineSettings>()
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(
        diagnostics,
        [],
        "neither reports the other's keys as unknown"
    );
    cx.update(|cx| {
        let notes = cx.settings::<NotesSettings>();
        assert_eq!((notes.font_size, notes.wrap), (18.0, true));
        assert_eq!(
            notes.font, "Menlo",
            "a key the file leaves out keeps its default"
        );
        let outline = cx.settings::<OutlineSettings>();
        assert_eq!((outline.depth, outline.marks.top.as_str()), (3, "*"));
    });

    // An unknown key inside a section, or a nested table of one, is still
    // a warning, as is a table no section has, each reported once.
    save(
        &file,
        "[notes]\nfont_size = 16\nfont_colour = \"red\"\n\n[outline]\ndepth = 4\n\n\
         [outline.marks]\nbottom = \"-\"\n\n[nope]\nx = 1\n",
        &sender,
    );
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            keys(config::diagnostics(cx)),
            [
                ("nope".to_owned(), false),
                ("notes.font_colour".to_owned(), false),
                ("outline.marks.bottom".to_owned(), false),
            ]
        );
        assert_eq!(
            cx.settings::<NotesSettings>().font_size,
            16.0,
            "warnings don't reject it"
        );
        assert_eq!(cx.settings::<OutlineSettings>().depth, 4);
        assert_eq!(cx.settings::<OutlineSettings>().marks.top, "#");
    });

    // A value one section can't take keeps that section's last good table,
    // the error at its key, while the other section reloads.
    save(
        &file,
        "[notes]\nfont_size = 12\n\n[outline]\ndepth = \"deep\"\n",
        &sender,
    );
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            keys(config::diagnostics(cx)),
            [("outline.depth".to_owned(), true)]
        );
        assert_eq!(cx.settings::<NotesSettings>().font_size, 12.0);
        assert_eq!(cx.settings::<OutlineSettings>().depth, 4, "the last good");
    });

    // A syntax error is the file's, reported once.
    save(&file, "[notes\n", &sender);
    cx.run_until_parked();
    cx.update(|cx| {
        let diagnostics = config::diagnostics(cx);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(diagnostics[0].is_error());
        assert_eq!(cx.settings::<NotesSettings>().font_size, 12.0);
    });
}

/// What a [`Reader`] saw: how often it drew, and the font size it read.
#[derive(Clone, Default)]
struct Seen {
    renders: Rc<Cell<usize>>,
    font_size: Rc<Cell<f32>>,
}

/// The app's view: the notes' font size, read with `cx.setting`.
struct Reader(Seen);

impl Render for Reader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let seen = self.0.clone();
        with_cx(window, cx, |cx| {
            let font_size = cx.setting::<NotesSettings>().font_size;
            seen.renders.set(seen.renders.get() + 1);
            seen.font_size.set(font_size);
            div().size_full().child(format!("{font_size}pt"))
        })
    }
}

/// The window's view: the reader drawn cached, so that a frame drawn
/// redraws it only if something invalidated it, as GPUI draws a view that
/// isn't notified.
struct Host(Entity<Reader>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let reader = AnyView::from(self.0.clone()).cached(StyleRefinement::default().size_full());
        div().size_full().child(reader)
    }
}

#[gpui::test]
fn in_an_app_a_view_reads_its_section_and_redraws_after_the_file_reloads(cx: &mut TestAppContext) {
    let seen = Seen::default();
    let made = seen.clone();
    let reader: Rc<RefCell<Option<Entity<Reader>>>> = Rc::default();
    let kept = reader.clone();
    let builder = kiln::app("com.example.Notes", "Notes").root(move |_, cx| {
        let reader = cx.new(|_| Reader(made));
        *kept.borrow_mut() = Some(reader.clone());
        cx.new(|_| Host(reader)).into()
    });
    let mut app = TestApp::open(builder, cx);
    assert_eq!(seen.font_size.get(), 14.0, "the default, with no file");
    let reader = reader.borrow().clone().expect("the reader was made");
    let notified = Rc::new(Cell::new(0));
    let _notifications = app.cx().update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&reader, move |_, _| notified.set(notified.get() + 1))
    });

    // Drawn cached, it isn't drawn again by a frame alone.
    let renders = seen.renders.get();
    app.draw();
    assert_eq!(seen.renders.get(), renders, "not redrawn without a reload");

    app.write_user_file(
        "settings.toml",
        "[notes]\nfont_size = 18\n\n[outline]\ndepth = 5\n",
    );
    assert_eq!(seen.font_size.get(), 18.0, "it redraws after the reload");
    assert_eq!(seen.renders.get(), renders + 1);
    assert_eq!(notified.get(), 1, "notified by its cx.setting");
    app.cx().update(|_, cx| {
        assert_eq!(config::diagnostics(cx), []);
        assert_eq!(cx.settings::<OutlineSettings>().depth, 5);
    });

    // A change to the other section alone leaves this one as it was.
    app.write_user_file(
        "settings.toml",
        "[notes]\nfont_size = 18\n\n[outline]\ndepth = 6\n",
    );
    assert_eq!(seen.font_size.get(), 18.0);
    assert_eq!(notified.get(), 1, "not notified: its section didn't change");
    app.cx()
        .update(|_, cx| assert_eq!(cx.settings::<OutlineSettings>().depth, 6));
}
