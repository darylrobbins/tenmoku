//! The setup wizard's sheet over a workbench in a headless window, every
//! write to a temporary `settings.toml` and marker.

use std::path::{Path, PathBuf};

use fe_buffer::Buffer;
use fe_command::{Invocation, Source};
use fe_config::settings::SettingsStore;
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Role,
    TestAppContext, VisualTestContext,
};
use kiln_platform::command_actions::dispatch_invocation;

use crate::{
    Launch, Workbench,
    setup_wizard::{SetupPaths, SetupWizard, Stop},
    setup_wizard_model::{ControlId, Step},
    workbench::toast::Level,
};

struct Sheet<'a> {
    dir: tempfile::TempDir,
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
}

/// A workbench with `a.txt` and `b.txt` open, `b.txt` active and focused.
fn open(cx: &mut TestAppContext) -> Sheet<'_> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    for name in ["a.txt", "b.txt"] {
        std::fs::write(root.join(name), "alpha\n").unwrap();
    }
    let paths = SetupPaths {
        settings: Some(dir.path().join("settings.toml")),
        state: Some(dir.path().join("state")),
    };
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        cx.set_global(paths);
    });
    let launch = Launch {
        root: Some(root.clone()),
        file: Some((root.join("a.txt"), Some(Buffer::new("alpha\n").into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| pane.open(root.join("b.txt"), window, cx));
    });
    draw(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    Sheet { dir, workbench, cx }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn click_at(at: gpui::Point<gpui::Pixels>, cx: &mut VisualTestContext) {
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

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let at = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} isn't drawn"))
        .center();
    click_at(at, cx);
}

impl Sheet<'_> {
    fn user(&self) -> PathBuf {
        self.dir.path().join("settings.toml")
    }

    fn state(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn show(&mut self) {
        let workbench = self.workbench.clone();
        self.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| workbench.show_setup(window, cx));
        });
        draw(self.cx);
    }

    fn sheet(&mut self) -> Entity<SetupWizard> {
        let setup = (self.workbench).read_with(self.cx, |workbench, _| workbench.setup().cloned());
        setup.expect("the sheet shows")
    }

    fn showing(&mut self) -> bool {
        (self.workbench).read_with(self.cx, |workbench, _| workbench.is_setup_showing())
    }

    fn step(&mut self) -> Step {
        let sheet = self.sheet();
        sheet.read_with(self.cx, |sheet, _| sheet.current_step())
    }

    fn stop(&mut self) -> Stop {
        let sheet = self.sheet();
        sheet.read_with(self.cx, |sheet, _| sheet.current_stop())
    }

    fn nodes(&mut self) -> Vec<(Role, String)> {
        draw(self.cx);
        let sheet = self.sheet();
        sheet.read_with(self.cx, |sheet, _| sheet.nodes())
    }

    /// Keys pressed, one at a time, with what they do done and drawn.
    fn press(&mut self, keys: &str) {
        for key in keys.split_whitespace() {
            self.cx.simulate_keystrokes(key);
            draw(self.cx);
        }
    }

    fn active_editor_focused(&mut self) -> bool {
        let workbench = self.workbench.clone();
        self.cx.update(|window, cx| {
            let editor = workbench.read(cx).active_editor(cx).cloned().unwrap();
            editor.focus_handle(cx).is_focused(window)
        })
    }

    fn active_tab(&mut self) -> Option<usize> {
        let pane = (self.workbench).read_with(self.cx, |workbench, _| workbench.pane().clone());
        pane.read_with(self.cx, |pane, _| pane.active())
    }

    fn tab_count(&mut self) -> usize {
        let pane = (self.workbench).read_with(self.cx, |workbench, _| workbench.pane().clone());
        pane.read_with(self.cx, |pane, _| pane.tabs().len())
    }

    fn palette_shows(&mut self) -> bool {
        (self.workbench).read_with(self.cx, |workbench, cx| {
            workbench.palette.is_some()
                || workbench
                    .shell()
                    .read(cx)
                    .modals()
                    .read(cx)
                    .modal()
                    .is_some()
        })
    }

    fn toasts(&mut self) -> Vec<(Level, String)> {
        let layer = (self.workbench).read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| (entry.toast.level, entry.toast.message.clone()))
                .collect()
        })
    }
}

/// What `user` holds, through the store that reads it for the app.
fn settings(user: &Path) -> fe_config::Settings {
    SettingsStore::load(Some(user.to_owned()), None)
        .settings()
        .as_ref()
        .clone()
}

fn outcome(state: &Path) -> String {
    let marker = state.join(fe_config::first_run::MARKER_FILE);
    let text = std::fs::read_to_string(&marker).unwrap_or_default();
    let table: toml::Table = text.parse().unwrap();
    (table.get("outcome").and_then(toml::Value::as_str))
        .unwrap_or("")
        .to_owned()
}

#[gpui::test]
fn shows_over_the_workbench(cx: &mut TestAppContext) {
    let mut s = open(cx);
    assert!(!s.showing());
    s.show();

    assert!(s.showing());
    assert_eq!(s.step(), Step::Appearance);
    let sheet = s.sheet();
    let in_sheet = |s: &mut Sheet| {
        let sheet = s.sheet();
        s.cx.update(|window, cx| sheet.read(cx).focus_handle().is_focused(window))
    };
    assert!(in_sheet(&mut s));
    // And the workbench's own handle leads there, as the app's focus at
    // launch does.
    let (workbench, handle) = (
        s.workbench.clone(),
        sheet.read_with(s.cx, |sheet, _| sheet.focus_handle()),
    );
    assert!(workbench.read_with(s.cx, |workbench, cx| workbench.focus_handle(cx) == handle));

    // A click on the scrim, on the tab that isn't active: no pane hears it,
    // and focus stays.
    let tab = (s.cx.debug_bounds("tab-icon:a.txt:file-text"))
        .expect("the tab is drawn")
        .center();
    let card = s.cx.debug_bounds("setup-card").expect("the card is drawn");
    assert!(
        !card.contains(&tab),
        "the tab is under the scrim, not the card"
    );
    let active = s.active_tab();
    click_at(tab, s.cx);
    assert_eq!(s.active_tab(), active);
    assert!(s.showing());
    assert!(in_sheet(&mut s));

    // Focus taken from elsewhere comes back, so keys can't reach what's under.
    let editor = workbench.read_with(s.cx, |workbench, cx| {
        workbench.active_editor(cx).cloned().unwrap()
    });
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(s.cx);
    assert!(in_sheet(&mut s));
}

#[gpui::test]
fn skip_and_escape_record_skipped(cx: &mut TestAppContext) {
    let mut s = open(cx);
    // The button.
    s.show();
    s.press("enter enter right");
    click("setup-skip", s.cx);
    assert!(!s.showing());
    assert!(s.active_editor_focused());
    assert_eq!(outcome(&s.state()), "skipped");
    let user = s.user();
    assert!(settings(&user).vim.enabled, "what was chosen stays");

    // Escape, from a later page and a marker that says otherwise.
    fe_config::first_run::record(&s.state(), fe_config::first_run::Outcome::Shown).unwrap();
    s.show();
    s.press("enter enter");
    assert_eq!(s.step(), Step::Keyboard);
    s.press("escape");
    assert!(!s.showing());
    assert!(s.active_editor_focused());
    assert_eq!(outcome(&s.state()), "skipped");
    assert!(settings(&user).vim.enabled, "what was chosen stays");

    // Tab to the button and Space presses it as well.
    fe_config::first_run::record(&s.state(), fe_config::first_run::Outcome::Shown).unwrap();
    s.show();
    while s.stop() != Stop::Skip {
        s.press("tab");
    }
    s.press("space");
    assert!(!s.showing());
    assert_eq!(outcome(&s.state()), "skipped");
}

#[gpui::test]
fn finish_records_completed(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.show();
    for step in [Step::Appearance, Step::Font, Step::Keyboard, Step::Editing] {
        assert_eq!(s.step(), step);
        click("setup-primary", s.cx);
    }
    assert_eq!(s.step(), Step::Ready);
    assert!(
        s.nodes()
            .contains(&(Role::Button, "Start Editing".to_owned()))
    );
    assert_eq!(outcome(&s.state()), "", "nothing's recorded before the end");
    click("setup-primary", s.cx);
    assert!(!s.showing());
    assert!(s.active_editor_focused());
    assert_eq!(outcome(&s.state()), "completed");
}

#[gpui::test]
fn keyboard_only_walkthrough(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let user = s.user();
    s.show();
    // Around the ring and back, then on through two pages.
    let first = s.stop();
    assert_eq!(first, Stop::Control(ControlId::ThemeMode));
    s.press("shift-tab");
    assert_eq!(s.stop(), Stop::Primary);
    s.press("tab");
    assert_eq!(s.stop(), first);
    // Enter on a control is Continue; Enter on the focused button presses it.
    s.press("enter");
    assert_eq!(s.step(), Step::Font);
    s.press("shift-tab shift-tab");
    assert_eq!(s.stop(), Stop::Back);
    s.press("enter");
    assert_eq!(s.step(), Step::Appearance);
    s.press("enter enter");
    assert_eq!(s.step(), Step::Keyboard);

    // Vim, by an arrow: its own two controls show.
    assert_eq!(s.stop(), Stop::Control(ControlId::Keyboard));
    s.press("right");
    let file = settings(&user);
    assert!(file.vim.enabled);
    // LazyVim.
    s.press("tab");
    assert_eq!(s.stop(), Stop::Control(ControlId::VimPreset));
    s.press("down");
    let file = settings(&user);
    assert_eq!(file.vim.preset, fe_config::settings::VimPreset::Lazyvim);
    // Relative line numbers.
    s.press("tab");
    assert_eq!(s.stop(), Stop::Control(ControlId::LineNumbers));
    s.press("right");
    let file = settings(&user);
    assert_eq!(
        file.editor.line_numbers,
        fe_config::settings::LineNumbersSetting::Relative
    );
    assert_eq!(s.step(), Step::Keyboard, "each before the next page");
    s.press("enter");

    // Format on save, by Space; arrows don't touch a switch.
    assert_eq!(s.step(), Step::Editing);
    assert_eq!(s.stop(), Stop::Control(ControlId::FormatOnSave));
    s.press("right");
    assert!(!settings(&user).editor.format_on_save);
    s.press("space");
    assert!(settings(&user).editor.format_on_save);
    assert_eq!(s.step(), Step::Editing);

    // The file spells them as the criteria do.
    let text = std::fs::read_to_string(&user).unwrap();
    let table: toml::Table = text.parse().unwrap();
    assert_eq!(table["vim"]["enabled"].as_bool(), Some(true));
    assert_eq!(table["vim"]["preset"].as_str(), Some("lazyvim"));
    assert_eq!(table["editor"]["line_numbers"].as_str(), Some("relative"));
    assert_eq!(table["editor"]["format_on_save"].as_bool(), Some(true));

    // And out by the keyboard alone: Tab to Start Editing on Ready.
    s.press("enter");
    assert_eq!(s.step(), Step::Ready);
    assert_eq!(s.stop(), Stop::Primary);
    s.press("space");
    assert!(!s.showing());
    assert_eq!(outcome(&s.state()), "completed");
}

#[gpui::test]
fn workbench_commands_wait(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.show();
    let tabs = s.tab_count();
    let page = s.step();
    let run = |command: &str, s: &mut Sheet| {
        let invocation = Invocation::of(command).unwrap();
        s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
        draw(s.cx);
    };

    // By key.
    s.press("cmd-p cmd-shift-p cmd-w");
    assert!(!s.palette_shows());
    assert_eq!(s.tab_count(), tabs);
    assert_eq!(s.step(), page);

    // As a RunCommand: the criterion's name, and the command's real one.
    for command in [
        "workbench.quick_open",
        "quick_open.toggle",
        "workbench.close_tab",
    ] {
        run(command, &mut s);
        assert!(!s.palette_shows(), "{command}");
        assert_eq!(s.tab_count(), tabs, "{command}");
    }

    // As a menu pick does: the typed command's action, not a RunCommand.
    for command in [
        "quick_open.toggle",
        "command_palette.toggle",
        "workbench.close_tab",
    ] {
        let invocation = Invocation::of(command).unwrap().source(Source::Menu);
        s.cx.update(|window, cx| dispatch_invocation(invocation, window, cx).unwrap());
        draw(s.cx);
        assert!(!s.palette_shows(), "{command}");
        assert_eq!(s.tab_count(), tabs, "{command}");
    }
    assert!(s.showing());
    (s.workbench).read_with(s.cx, |workbench, _| assert_eq!(workbench.commands_run, []));

    // The app's own still go, and quit by its key.
    s.press("secondary-q");
    let ran = (s.workbench).read_with(s.cx, |workbench, _| workbench.commands_run.clone());
    assert_eq!(ran.len(), 1, "{ran:?}");
    assert_eq!(ran[0].command.as_str(), "app.quit");
    assert_eq!(ran[0].source, Source::Keyboard);

    // With the sheet gone, the commands are back.
    s.press("escape");
    s.press("cmd-p");
    assert!(s.palette_shows());
}

#[gpui::test]
fn write_failure_toasts(cx: &mut TestAppContext) {
    let mut s = open(cx);
    // A folder where the file goes can't be written.
    std::fs::create_dir(s.user()).unwrap();
    s.show();
    s.press("enter enter");
    assert_eq!(s.step(), Step::Keyboard);
    s.press("right");
    let toasts = s.toasts();
    assert_eq!(toasts.len(), 1, "{toasts:?}");
    assert_eq!(toasts[0].0, Level::Error);
    assert!(toasts[0].1.contains("vim.enabled"), "{toasts:?}");
    assert_eq!(s.step(), Step::Keyboard, "the page stays");
    assert!(s.showing());

    // Two steps at once: both fail, and the size goes back past both.
    assert_eq!(s.stop(), Stop::Control(ControlId::Keyboard));
    click("setup-back", s.cx);
    assert_eq!(s.step(), Step::Font);
    s.press("tab");
    assert_eq!(s.stop(), Stop::Control(ControlId::FontSize));
    s.cx.simulate_keystrokes("right");
    s.cx.simulate_keystrokes("right");
    draw(s.cx);
    let toasts = s.toasts();
    assert!(
        toasts
            .iter()
            .any(|(_, message)| message.contains("editor.font_size")),
        "{toasts:?}"
    );
    s.press("enter");

    // The controls show what the file has, so Vim's own two are gone, and the
    // Ready page counts neither.
    assert_eq!(s.step(), Step::Keyboard);
    let nodes = s.nodes();
    assert!(
        !nodes.contains(&(Role::RadioGroup, "Vim keys".to_owned())),
        "{nodes:?}"
    );
    s.press("enter enter");
    assert_eq!(s.step(), Step::Ready);
    let nodes = s.nodes();
    assert!(
        nodes.contains(&(Role::Label, "Tenmoku's defaults are set".to_owned())),
        "{nodes:?}"
    );
}

#[gpui::test]
fn labels_and_roles(cx: &mut TestAppContext) {
    let mut s = open(cx);
    s.show();
    let pages = [
        ("Appearance", vec!["Mode", "Dark theme"], vec![]),
        ("Font", vec!["Font family"], vec!["Ligatures"]),
        ("Keyboard", vec!["Keys"], vec![]),
        (
            "Editing",
            vec!["Auto save", "Inline diagnostics"],
            vec!["Format on save", "Minimap", "Inlay hints", "Soft wrap"],
        ),
        ("Ready", vec![], vec![]),
    ];
    for (ix, (title, groups, switches)) in pages.into_iter().enumerate() {
        let nodes = s.nodes();
        let dialog = format!("Set Up Tenmoku, step {} of 5, {title}", ix + 1);
        assert!(nodes.contains(&(Role::Dialog, dialog)), "{nodes:?}");
        for group in groups {
            let group = (Role::RadioGroup, group.to_owned());
            assert!(nodes.contains(&group), "{title}: {group:?} in {nodes:?}");
        }
        for switch in switches {
            let switch = (Role::Switch, switch.to_owned());
            assert!(nodes.contains(&switch), "{title}: {switch:?} in {nodes:?}");
        }
        for (role, label) in &nodes {
            assert!(!label.is_empty(), "{title}: a {role:?} with no label");
        }
        let has = |role, label: &str| nodes.contains(&(role, label.to_owned()));
        assert!(has(Role::Button, "Skip setup"), "{title}");
        assert_eq!(has(Role::Button, "Back"), ix > 0, "{title}");
        assert!(
            has(
                Role::Button,
                if ix < 4 { "Continue" } else { "Start Editing" }
            ),
            "{title}"
        );
        // A group's radio buttons are labelled from the model.
        if title == "Appearance" {
            for option in ["Dark", "Light", "Match System"] {
                assert!(has(Role::RadioButton, option), "{option}");
            }
        }
        if title == "Font" {
            assert!(has(Role::SpinButton, "Font size"));
            assert!(has(Role::Button, "Decrease font size"));
            assert!(has(Role::Button, "Increase font size"));
        }
        s.press("enter");
        if ix < 4 {
            assert!(s.showing());
        }
    }
    assert!(!s.showing());

    // Vim's two, once chosen, are groups with the model's labels too.
    s.show();
    s.press("enter enter right");
    let nodes = s.nodes();
    for (role, label) in [
        (Role::RadioButton, "Standard"),
        (Role::RadioButton, "Vim"),
        (Role::RadioGroup, "Vim keys"),
        (Role::RadioButton, "LazyVim, Space as leader"),
        (Role::RadioGroup, "Line numbers"),
        (Role::RadioButton, "Relative"),
    ] {
        assert!(
            nodes.contains(&(role, label.to_owned())),
            "{label}: {nodes:?}"
        );
    }
}

/// The label a key sequence shows as, written as the default keymap's. The tests' keymap is
/// `fe_keymap::DEFAULT_MACOS` on every host, so write `cmd-` keys, not the platform-relative ones.
fn label(keys: &str) -> String {
    fe_keymap::shortcut_label(&fe_keymap::KeySequence::parse(keys).unwrap())
}

#[gpui::test]
fn ready_counts_and_shortcuts(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let has =
        |nodes: &[(Role, String)], text: &str| nodes.contains(&(Role::Label, text.to_owned()));
    // None changed.
    s.show();
    s.press("enter enter enter enter");
    assert_eq!(s.step(), Step::Ready);
    let nodes = s.nodes();
    assert!(has(&nodes, "Tenmoku's defaults are set"), "{nodes:?}");
    let settings_hint = format!("Change any of them in Settings ({})", label("cmd-,"));
    assert!(has(&nodes, &settings_hint), "{nodes:?}");
    let hints = [
        format!("Command Palette ({})", label("cmd-shift-p")),
        format!("Go to File ({})", label("cmd-p")),
        format!("Open Folder ({})", label("cmd-o")),
    ];
    for hint in &hints {
        assert!(has(&nodes, hint), "{hint}: {nodes:?}");
    }
    s.press("escape");

    // One, then two, in the words the page uses for each.
    s.show();
    s.press("enter enter right enter enter");
    assert_eq!(s.step(), Step::Ready);
    let nodes = s.nodes();
    assert!(has(&nodes, "You changed 1 setting"), "{nodes:?}");
    s.press("escape");

    s.show();
    s.press("enter enter right tab right enter enter");
    assert_eq!(s.step(), Step::Ready);
    let nodes = s.nodes();
    assert!(has(&nodes, "You changed 2 settings"), "{nodes:?}");
    s.press("escape");

    // The active keymap's keys, not the default's.
    let user = s.dir.path().join("keymap.toml");
    let rebound = "[[bindings]]\nkeys = \"f6\"\ncommand = \"command_palette.toggle\"\ncontext = \"workbench\"\n";
    std::fs::write(&user, rebound).unwrap();
    let store = fe_keymap::KeymapStore::load(fe_keymap::DEFAULT_MACOS, Some(user), None);
    assert_eq!(store.diagnostics(), []);
    let keymap = store.keymap().clone();
    s.cx.update(|_, cx| kiln_platform::keys::set_keymap(cx, keymap));
    s.show();
    s.press("enter enter enter enter");
    let nodes = s.nodes();
    assert!(
        has(&nodes, &format!("Command Palette ({})", label("f6"))),
        "{nodes:?}"
    );
}

/// The mouse does what the keys do, and moves the ring.
#[gpui::test]
fn clicks_choose_and_write(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let user = s.user();
    s.show();
    s.press("enter");
    // The stepper, to its top and no further.
    let size = settings(&user).editor.font_size;
    click("setup-step-up-FontSize", s.cx);
    assert_eq!(s.stop(), Stop::Control(ControlId::FontSize));
    assert_eq!(settings(&user).editor.font_size, size + 1.);
    click("setup-step-down-FontSize", s.cx);
    assert_eq!(settings(&user).editor.font_size, size);
    // A switch.
    let ligatures = settings(&user).editor.font_ligatures;
    click("setup-switch-FontLigatures", s.cx);
    assert_eq!(s.stop(), Stop::Control(ControlId::FontLigatures));
    assert_eq!(settings(&user).editor.font_ligatures, !ligatures);
    // A segment, on the next page; Back and Continue, by their buttons.
    click("setup-back", s.cx);
    assert_eq!(s.step(), Step::Appearance);
    click("setup-primary", s.cx);
    click("setup-primary", s.cx);
    assert_eq!(s.step(), Step::Keyboard);
    click("setup-radio-Keyboard-1", s.cx);
    assert!(settings(&user).vim.enabled);
    assert_eq!(s.stop(), Stop::Control(ControlId::Keyboard));
}

/// Under high contrast (the system's Increase Contrast, or
/// `workbench.contrast = "high"`) the focused stop's ring is the design's
/// 2 px, opaque, and every control stays where it was.
#[gpui::test]
fn high_contrast_ring_is_two_pixels_in_place(cx: &mut TestAppContext) {
    use gpui::px;
    use kiln::{core::theme::Contrast, ui::GlobalTheme};

    let mut s = open(cx);
    s.show();
    let rings = |s: &mut Sheet<'_>| {
        draw(s.cx);
        let sheet = s.sheet();
        sheet.read_with(s.cx, |sheet, _| sheet.rings())
    };
    let controls = [
        "setup-radio-ThemeMode-0",
        "setup-radio-ThemeMode-2",
        "setup-primary",
    ];
    let bounds = |s: &mut Sheet<'_>| {
        draw(s.cx);
        controls.map(|selector| s.cx.debug_bounds(selector))
    };

    let normal = rings(&mut s);
    assert!(normal.iter().any(|&(focused, _, _)| focused), "{normal:?}");
    assert!(
        normal
            .iter()
            .all(|&(_, border, padding)| (border, padding) == (px(1.), px(1.)))
    );
    let before = bounds(&mut s);
    assert!(before.iter().all(Option::is_some), "{before:?}");

    s.cx.update(|_, cx| GlobalTheme::set_contrast(cx, Contrast::High));
    let high = rings(&mut s);
    assert!(high.iter().any(|&(focused, _, _)| focused), "{high:?}");
    assert!(
        high.iter()
            .all(|&(_, border, padding)| (border, padding) == (px(2.), px(0.)))
    );
    assert_eq!(bounds(&mut s), before, "a control moved");
    let ring =
        s.cx.update(|_, cx| fe_ui::ActiveTheme::theme(cx).colors.state.focus_ring);
    assert_eq!(ring.a, 1., "the ring is opaque");

    // Off again: back to 1 px.
    s.cx.update(|_, cx| GlobalTheme::set_contrast(cx, Contrast::Normal));
    assert_eq!(rings(&mut s), normal);
}

/// In a window too short for the Ready page, tabbing to Install scrolls
/// it into the body's view, above the footer, ring and all, so Enter never
/// presses a button that can't be seen (T1343 review F3).
#[gpui::test]
fn tabbing_to_install_reveals_it_in_a_short_window(cx: &mut TestAppContext) {
    if !cfg!(target_os = "macos") {
        // Install is the Mac's only.
        return;
    }
    let mut s = open(cx);
    s.cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(260.)));
    draw(s.cx);
    s.show();
    for _ in [Step::Appearance, Step::Font, Step::Keyboard, Step::Editing] {
        click("setup-primary", s.cx);
    }
    assert_eq!(s.step(), Step::Ready);
    let visible = |s: &mut Sheet<'_>| {
        draw(s.cx);
        draw(s.cx);
        let install =
            s.cx.debug_bounds("setup-install-shell")
                .expect("Install is drawn");
        let card = s.cx.debug_bounds("setup-card").expect("the card is drawn");
        let footer =
            s.cx.debug_bounds("setup-primary")
                .expect("the footer is drawn");
        install.top() >= card.top() && install.bottom() <= footer.top()
    };
    // Short enough that it starts out of view, under the footer.
    assert!(!visible(&mut s), "the window is too tall to test this");
    while s.stop() != Stop::Install {
        s.press("tab");
    }
    assert!(visible(&mut s), "Install has the ring but is out of view");
}
