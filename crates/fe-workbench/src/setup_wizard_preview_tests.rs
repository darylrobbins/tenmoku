//! The setup wizard's Appearance and Font pages over a workbench in a headless
//! window, and what becomes of the writes their choices queue.

use std::{cell::Cell, path::PathBuf, rc::Rc, sync::Arc};

use fe_buffer::Buffer;
use fe_config::{
    EditorPalette, Settings, SettingsStore,
    themes::{self, bundled_themes},
};
use fe_gpui::{EditorSettings, EditorTheme};
use fe_ui::{Theme, ThemeData, hsla};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Role,
    TestAppContext, VisualTestContext, px,
};
use kiln_platform::{
    config::{ConfigService, batch_channel},
    fonts,
};

use crate::{
    Launch, Workbench,
    setup_wizard::{SetupPaths, SetupWizard, Stop},
    setup_wizard_model::{Choices, ControlId, Step},
    setup_wizard_preview::{Swatch, TileArt, font_families, label_family},
};

struct Sheet<'a> {
    dir: tempfile::TempDir,
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
}

/// `id`'s bundled theme's data, as its file has it.
fn theme_data(id: &str) -> ThemeData {
    let theme = themes::find(id).expect("a bundled theme");
    ThemeData::from_file(&theme.file())
}

/// Makes `id` the bundled theme in use, as the host does when settings choose
/// it: the chrome's and the editors' colours.
fn activate(id: &str, cx: &mut gpui::App) -> EditorPalette {
    let data = theme_data(id);
    let (palette, warnings) = EditorPalette::for_theme(&data);
    assert_eq!(warnings, [] as [String; 0]);
    fe_ui::set_theme(cx, Theme::new(data));
    fe_gpui::set_editor_palette(cx, Arc::new(palette.clone()));
    palette
}

/// A workbench with Yohen, the default, in use, under `settings` as the config
/// service's, with the sheet up, and every write going to their file.
fn open<'a>(settings: &str, cx: &'a mut TestAppContext) -> Sheet<'a> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let user = dir.path().join("settings.toml");
    std::fs::write(&user, settings).unwrap();
    let paths = SetupPaths {
        settings: Some(user.clone()),
        state: Some(dir.path().join("state")),
    };
    let store = SettingsStore::load(Some(user), None).into_inner();
    let (_, batches) = batch_channel();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        cx.set_global(paths);
        ConfigService::new()
            .settings(store)
            .start_with(batches, cx)
            .detach();
        activate("yohen", cx);
    });
    let launch = Launch {
        root: Some(root),
        file: Some((
            dir.path().join("root/a.txt"),
            Some(Buffer::new("a\n").into()),
        )),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let handle = workbench.clone();
    cx.update(|window, cx| {
        handle.update(cx, |workbench, cx| workbench.show_setup(window, cx));
    });
    draw(cx);
    Sheet { dir, workbench, cx }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let at = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} isn't drawn"))
        .center();
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
    });
    draw(cx);
}

impl Sheet<'_> {
    fn user(&self) -> PathBuf {
        self.dir.path().join("settings.toml")
    }

    fn sheet(&mut self) -> Entity<SetupWizard> {
        let setup = (self.workbench).read_with(self.cx, |workbench, _| workbench.setup().cloned());
        setup.expect("the sheet shows")
    }

    fn press(&mut self, keys: &str) {
        for key in keys.split_whitespace() {
            self.cx.simulate_keystrokes(key);
            draw(self.cx);
        }
    }

    fn file(&self) -> Settings {
        SettingsStore::load(Some(self.user()), None)
            .settings()
            .as_ref()
            .clone()
    }

    /// What `id`'s control shows as chosen.
    fn shown(&mut self, id: ControlId) -> String {
        let sheet = self.sheet();
        sheet.read_with(self.cx, |sheet, _| {
            let control = sheet.control(id).expect("the control is listed");
            control.options[control.selected].label.clone()
        })
    }

    /// Later writes in this test go to `settings`: a folder fails.
    fn write_to(&mut self, settings: PathBuf) {
        let state = self.dir.path().join("state");
        self.cx.update(|_, cx| {
            cx.set_global(SetupPaths {
                settings: Some(settings),
                state: Some(state),
            })
        });
    }
}

/// What `id`'s file says a tile should be painted in, not going through the
/// code that paints it.
fn expected_swatch(id: &str) -> Swatch {
    let data = theme_data(id);
    let colors = Theme::new(data.clone()).colors;
    let (palette, _) = EditorPalette::for_theme(&data);
    let syntax = |capture| hsla(palette.syntax_color(capture).unwrap());
    Swatch {
        panel: colors.surface.panel,
        background: colors.surface.background,
        accent: colors.accent.base,
        keyword: syntax("keyword"),
        string: syntax("string"),
        function: syntax("function"),
    }
}

#[gpui::test]
fn tiles_use_each_themes_colours(cx: &mut TestAppContext) {
    let mut s = open("[theme]\nmode = \"system\"\n", cx);
    let sheet = s.sheet();
    assert_eq!(
        s.cx.update(|_, cx| fe_ui::ActiveTheme::theme(&*cx).name.clone()),
        theme_data("yohen").name,
    );
    let tiles = sheet.read_with(s.cx, |sheet, _| sheet.tiles());

    let bundled = bundled_themes();
    let ember = expected_swatch("tenmoku-ember");
    let latte = expected_swatch("catppuccin-latte");
    assert_ne!(
        ember, latte,
        "the themes differ, so a match means something"
    );
    for theme in bundled {
        let art = (tiles.iter().find(|(label, _)| label == theme.title))
            .unwrap_or_else(|| panic!("no tile for {}: {tiles:?}", theme.title))
            .1;
        assert_eq!(
            art,
            TileArt::Window(expected_swatch(theme.id)),
            "{}",
            theme.title
        );
    }

    // Mode's three are the chosen themes', the last both of them.
    let (dark, light) = (expected_swatch("yohen"), expected_swatch("hakuji"));
    let mode: Vec<_> = tiles.iter().take(3).cloned().collect();
    assert_eq!(
        mode,
        [
            ("Dark".to_owned(), TileArt::Window(dark)),
            ("Light".to_owned(), TileArt::Window(light)),
            ("Match System".to_owned(), TileArt::Split { dark, light }),
        ]
    );

    // Choosing a theme changes Mode's tile for it; the hidden slot's still shown.
    sheet.update(s.cx, |sheet, cx| {
        sheet.pick(ControlId::DarkTheme, "Tenmoku Ember", cx);
        sheet.pick(ControlId::ThemeMode, "Dark", cx);
    });
    draw(s.cx);
    let tiles = sheet.read_with(s.cx, |sheet, _| sheet.tiles());
    assert_eq!(tiles[0].1, TileArt::Window(ember));
    assert_eq!(tiles[1].1, TileArt::Window(light));
    assert_eq!(tiles[2].1, TileArt::Split { dark: ember, light });

    // A theme that isn't bundled has no file to read: the active one, if it's
    // of its appearance, or else the bundled base.
    let mut other = open_second(
        "[theme]\nmode = \"system\"\ndark = \"acme/night\"\nlight = \"acme/noon\"\n",
        s,
    );
    let sheet = other.sheet();
    let tiles = sheet.read_with(other.cx, |sheet, _| sheet.tiles());
    let art = |label: &str| {
        let tile = tiles.iter().find(|(tile, _)| tile == label);
        tile.unwrap_or_else(|| panic!("no tile {label}: {tiles:?}"))
            .1
    };
    assert_eq!(art("acme/night"), TileArt::Window(expected_swatch("yohen")));
    let base = ThemeData::base(fe_ui::Appearance::Light);
    let colors = Theme::new(base.clone()).colors;
    let TileArt::Window(noon) = art("acme/noon") else {
        panic!("a theme's tile is a window");
    };
    assert_eq!(noon.panel, colors.surface.panel);
    assert_eq!(noon.background, colors.surface.background);
}

/// `s`'s sheet closed and another opened under `settings`.
fn open_second<'a>(settings: &str, mut s: Sheet<'a>) -> Sheet<'a> {
    s.press("escape");
    std::fs::write(s.user(), settings).unwrap();
    let store = SettingsStore::load(Some(s.user()), None).into_inner();
    let (_, batches) = batch_channel();
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| {
        ConfigService::new()
            .settings(store)
            .start_with(batches, cx)
            .detach();
        workbench.update(cx, |workbench, cx| workbench.show_setup(window, cx));
    });
    draw(s.cx);
    s
}

#[gpui::test]
fn sample_follows_settings(cx: &mut TestAppContext) {
    let mut s = open("", cx);
    let user = s.user();
    let mut store = SettingsStore::load(Some(user.clone()), None);
    let sheet = s.sheet();
    let editor = sheet.read_with(s.cx, |sheet, _| sheet.sample_editor().cloned());
    let editor = editor.expect("the sample is there once the sheet has drawn");
    let start_size = editor.read_with(s.cx, |editor, _| editor.theme().font_size);
    assert_ne!(start_size, px(16.));
    assert_eq!(
        editor.read_with(s.cx, |editor, _| editor.theme().background),
        hsla(
            EditorPalette::for_theme(&theme_data("yohen"))
                .0
                .editor
                .background
        ),
    );

    // By the sheet's controls, as a user would.
    click("setup-radio-ThemeMode-1", s.cx);
    assert_eq!(s.file().theme.mode, fe_config::ThemeModeSetting::Light);
    s.press("enter");
    while s.file().editor.font_size < 16. {
        click("setup-step-up-FontSize", s.cx);
    }
    assert_eq!(s.file().editor.font_size, 16.);
    assert_eq!(
        editor.read_with(s.cx, |editor, _| editor.theme().font_size),
        start_size,
        "not until the settings reload",
    );

    // The reload those writes bring, as the host applies it to the window.
    let event = store
        .reload(std::slice::from_ref(&user))
        .expect("the file changed");
    let workbench = s.workbench.clone();
    let light = s.cx.update(|_, cx| {
        workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
        activate("hakuji", cx)
    });
    draw(s.cx);
    let want =
        s.cx.update(|_, cx| EditorTheme::from_palette(&light, &EditorSettings::default(), cx));
    let pane = workbench.read_with(s.cx, |workbench, _| workbench.pane().clone());
    let family = pane.read_with(s.cx, |pane, _| pane.theme().font_family.clone());
    editor.read_with(s.cx, |editor, _| {
        let theme = editor.theme();
        assert_eq!(theme.font_size, px(16.));
        assert_eq!(theme.font_family, family);
        assert_eq!(theme.background, want.background);
        assert_eq!(theme.foreground, want.foreground);
        assert_eq!(theme.syntax, want.syntax);
        assert!(editor.is_read_only());
    });

    // The family and ligatures follow too.
    fe_config::write_settings(
        &user,
        &[
            (
                &["editor", "font_family"],
                Some(fe_config::SettingValue::Str("Menlo")),
            ),
            (
                &["editor", "font_ligatures"],
                Some(fe_config::SettingValue::Bool(false)),
            ),
        ],
    )
    .unwrap();
    let event = store
        .reload(std::slice::from_ref(&user))
        .expect("the file changed");
    s.cx.update(|_, cx| {
        workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    });
    draw(s.cx);
    editor.read_with(s.cx, |editor, _| {
        let theme = editor.theme();
        assert_eq!(theme.font_family, "Menlo");
        assert_eq!(theme.font_features, gpui::FontFeatures::disable_ligatures());
        assert_eq!(theme.font_size, px(16.));
    });

    // A click on it: no focus, no caret moved, the ring where it was.
    let stop = sheet.read_with(s.cx, |sheet, _| sheet.current_stop());
    let selections = editor.read_with(s.cx, |editor, _| editor.model().selections());
    click("setup-sample", s.cx);
    let in_sample =
        s.cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    assert!(!in_sample);
    let in_sheet =
        s.cx.update(|window, cx| sheet.read(cx).focus_handle().is_focused(window));
    assert!(in_sheet);
    assert_eq!(sheet.read_with(s.cx, |sheet, _| sheet.current_stop()), stop);
    assert_eq!(
        editor.read_with(s.cx, |editor, _| editor.model().selections()),
        selections
    );
    let typed = editor.read_with(s.cx, |editor, _| editor.model().text().to_string());
    s.cx.simulate_input("x");
    draw(s.cx);
    assert_eq!(
        editor.read_with(s.cx, |editor, _| editor.model().text().to_string()),
        typed
    );

    // It's a picture to VoiceOver, on each of the two pages.
    let nodes = sheet.read_with(s.cx, |sheet, _| sheet.nodes());
    assert!(
        nodes.contains(&(Role::Figure, "Code sample".to_owned())),
        "{nodes:?}"
    );
    click("setup-back", s.cx);
    assert_eq!(
        sheet.read_with(s.cx, |sheet, _| sheet.current_step()),
        Step::Appearance
    );
    let nodes = sheet.read_with(s.cx, |sheet, _| sheet.nodes());
    assert!(
        nodes.contains(&(Role::Figure, "Code sample".to_owned())),
        "{nodes:?}"
    );
}

/// The labels the Font page lists under `settings`, given the families.
fn labels(settings: &Settings, installed: &[&str]) -> Vec<String> {
    let choices = Choices::from_settings(settings, installed);
    let controls = Step::Font.controls(&choices);
    let family = controls
        .iter()
        .find(|c| c.id == ControlId::FontFamily)
        .unwrap();
    family
        .options
        .iter()
        .map(|option| option.label.clone())
        .collect()
}

#[gpui::test]
fn font_list_is_curated(cx: &mut TestAppContext) {
    // Those of the curated list that are installed, in its order, however the
    // system lists them.
    // Spline Sans Mono, which Tenmoku bundles, first and the default.
    let system = [
        "Zapfino",
        "Ubuntu Mono",
        "Comic Sans MS",
        "Menlo",
        "Spline Sans Mono",
        "Arial",
    ];
    let installed = font_families(|family| system.contains(&family));
    assert_eq!(installed, ["Spline Sans Mono", "Menlo", "Ubuntu Mono"]);

    let mut settings = Settings::default();
    let default = "Default (Spline Sans Mono)";
    assert_eq!(
        labels(&settings, &installed),
        [default, "Spline Sans Mono", "Menlo", "Ubuntu Mono"]
    );
    // One set by hand stays listed while it's the current one.
    settings.editor.font_family = Some("Comic Code".to_owned());
    assert_eq!(
        labels(&settings, &installed),
        [
            default,
            "Spline Sans Mono",
            "Menlo",
            "Ubuntu Mono",
            "Comic Code"
        ]
    );
    // One of the curated list isn't listed twice.
    settings.editor.font_family = Some("Ubuntu Mono".to_owned());
    assert_eq!(
        labels(&settings, &installed),
        [default, "Spline Sans Mono", "Menlo", "Ubuntu Mono"]
    );
    // Without it, the next installed one is the default.
    let installed =
        font_families(|family| family != "Spline Sans Mono" && system.contains(&family));
    assert_eq!(
        labels(&Settings::default(), &installed),
        ["Default (Menlo)", "Menlo", "Ubuntu Mono"]
    );

    // Each is drawn in itself, the default in what it stands for.
    assert_eq!(label_family(default), "Spline Sans Mono");
    assert_eq!(label_family("Default (Menlo)"), "Menlo");
    assert_eq!(label_family("Comic Code"), "Comic Code");

    // And the sheet asks the text system family by family: what it lists is
    // just what the kiln helper finds installed, with the set font after.
    let mut s = open("[editor]\nfont_family = \"Comic Code\"\n", cx);
    s.press("enter");
    let sheet = s.sheet();
    let (control, nodes) = sheet.read_with(s.cx, |sheet, _| {
        (sheet.control(ControlId::FontFamily).unwrap(), sheet.nodes())
    });
    let listed: Vec<&str> = (control.options.iter())
        .map(|option| option.label.as_str())
        .collect();
    let (default, listed_rest) = listed.split_first().unwrap();
    let (custom, curated) = listed_rest.split_last().unwrap();
    assert_eq!(*custom, "Comic Code");
    let mut rest = fonts::MONO_FAMILIES.iter();
    assert!(
        curated.iter().all(|family| rest.any(|mono| mono == family)),
        "{listed:?} are the curated ones, in their order"
    );
    let first =
        (curated.first().copied()).unwrap_or(fonts::mono_last_resort(kiln::core::Os::current()));
    assert_eq!(*default, format!("Default ({first})"));
    for label in &listed {
        let node = (Role::RadioButton, (*label).to_owned());
        assert!(nodes.contains(&node), "{label}: {nodes:?}");
    }
}

/// T1341's finding: the sheet owned the last write, so a window closed under it
/// cancelled what was queued. Nothing runs before the test parks, so all three
/// are still waiting, the first ahead of the rest, when the window goes.
#[gpui::test]
fn closing_the_window_keeps_queued_writes(cx: &mut TestAppContext) {
    let mut s = open("", cx);
    let user = s.user();
    s.press("enter");
    let sheet = s.sheet();
    let (size, ligatures) = {
        let file = s.file();
        (file.editor.font_size, file.editor.font_ligatures)
    };
    let (first, last) = ((size + 1.).to_string(), (size + 2.).to_string());
    sheet.update(s.cx, |sheet, cx| {
        sheet.pick(ControlId::FontSize, &first, cx);
        sheet.pick(
            ControlId::FontLigatures,
            if ligatures { "Off" } else { "On" },
            cx,
        );
        sheet.pick(ControlId::FontSize, &last, cx);
    });
    assert_eq!(s.file().editor.font_size, size, "none has run yet");

    let Sheet { dir, workbench, cx } = s;
    let released = Rc::new(Cell::new(false));
    let workbench_released = Rc::new(Cell::new(false));
    cx.update(|_, cx| {
        let released = released.clone();
        cx.observe_release(&sheet, move |_, _| released.set(true))
            .detach();
        let released = workbench_released.clone();
        cx.observe_release(&workbench, move |_, _| released.set(true))
            .detach();
    });
    cx.update(|window, _| window.remove_window());
    drop((sheet, workbench));
    // The release comes with the effects' next flush. Parking only after it
    // means the writes were still waiting when the sheet went.
    TestAppContext::update(cx, |_| {});
    assert!(
        workbench_released.get(),
        "the workbench went with its window"
    );
    assert!(released.get(), "the sheet went with its window");
    cx.run_until_parked();

    let file = SettingsStore::load(Some(user), None)
        .settings()
        .as_ref()
        .clone();
    assert_eq!(
        file.editor.font_size,
        size + 2.,
        "the last of them is what stays"
    );
    assert_eq!(file.editor.font_ligatures, !ligatures);
    drop(dir);
}

/// T1341's finding: under a `theme.name` of the slot's, a theme is chosen and
/// then the bundled default before either write is done. The first write is the
/// one retiring the name; when it fails and the second doesn't, the name mustn't
/// survive to bring the old theme back over what the sheet shows.
fn failed_write_still_retires_name(
    settings: &str,
    id: ControlId,
    named: &str,
    other: &str,
    default: &str,
    cx: &mut TestAppContext,
) -> Settings {
    let mut s = open(settings, cx);
    let user = s.user();
    assert_eq!(s.shown(id), named);
    let folder = s.dir.path().join("a-folder");
    std::fs::create_dir(&folder).unwrap();
    let sheet = s.sheet();

    s.write_to(folder);
    sheet.update(s.cx, |sheet, cx| sheet.pick(id, other, cx));
    s.write_to(user);
    sheet.update(s.cx, |sheet, cx| sheet.pick(id, default, cx));
    draw(s.cx);

    assert_eq!(s.shown(id), default);
    s.file()
}

#[gpui::test]
fn failed_dark_write_still_retires_name(cx: &mut TestAppContext) {
    let file = failed_write_still_retires_name(
        "[theme]\nname = \"Ember\"\n",
        ControlId::DarkTheme,
        "Tenmoku Ember",
        "Dark Modern",
        "Yohen",
        cx,
    );
    assert_eq!(file.theme.name, None);
    assert_eq!(file.theme.dark, None);
}

#[gpui::test]
fn failed_light_write_still_retires_name(cx: &mut TestAppContext) {
    let file = failed_write_still_retires_name(
        "[theme]\nmode = \"light\"\nname = \"Catppuccin Latte\"\n",
        ControlId::LightTheme,
        "Catppuccin Latte",
        "Light 2026",
        "Hakuji",
        cx,
    );
    assert_eq!(file.theme.name, None);
    assert_eq!(file.theme.light, None);
    assert_eq!(file.theme.mode, fe_config::ThemeModeSetting::Light);
}

/// A write that fails with nothing queued after it goes back to what the file
/// has, the `theme.name` it would have retired included.
#[gpui::test]
fn failed_write_alone_goes_back(cx: &mut TestAppContext) {
    let mut s = open("[theme]\nname = \"Ember\"\n", cx);
    let folder = s.dir.path().join("a-folder");
    std::fs::create_dir(&folder).unwrap();
    let sheet = s.sheet();
    s.write_to(folder);
    sheet.update(s.cx, |sheet, cx| {
        sheet.pick(ControlId::DarkTheme, "Dark Modern", cx)
    });
    draw(s.cx);
    assert_eq!(s.shown(ControlId::DarkTheme), "Tenmoku Ember");
    assert_eq!(s.file().theme.name.as_deref(), Some("Ember"));
    assert_eq!(
        sheet.read_with(s.cx, |sheet, _| sheet.current_stop()),
        Stop::Control(ControlId::ThemeMode)
    );
}
