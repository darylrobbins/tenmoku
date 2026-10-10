// SPDX-License-Identifier: LicenseRef-Proprietary

//! The active theme's tokens: `cx.theme()`, setting a theme or a dark and
//! light pair, the flat chrome colours as a view of it, and controls drawn
//! from it.

use std::{cell::Cell, path::Path, rc::Rc};

use gpui::{
    Context, IntoElement, ParentElement as _, Render, TestAppContext, Window, WindowAppearance,
    div, rgb,
};
use kiln_core::theme::ThemeFile;

use crate::{
    ActiveTheme as _, ActiveUiTheme, Appearance, Button, ButtonVariant, Checkbox, Color,
    GlobalTheme, Switch, Theme, ThemeData, ThemeMode, UiTheme, hsla, set_theme, set_themes,
    set_ui_theme, system_appearance_changed, ui_theme,
};

/// A theme read from `source`, over the base its appearance names.
fn theme(source: &str) -> Theme {
    let (file, diagnostics) = ThemeFile::parse(Path::new("test.toml"), source, &[]);
    assert_eq!(diagnostics, []);
    Theme::new(ThemeData::from_file(&file.unwrap()))
}

fn name(cx: &mut TestAppContext) -> String {
    cx.read(|cx| cx.theme().name.to_string())
}

#[gpui::test]
fn default_tokens_are_the_default_dark_chrome(cx: &mut TestAppContext) {
    assert_eq!(cx.read(ui_theme), UiTheme::default_dark());
    assert_eq!(cx.read(|cx| cx.theme().ui_theme()), UiTheme::default_dark());
    assert_eq!(Theme::dark().ui_theme(), UiTheme::default_dark());
    assert_eq!(cx.read(|cx| cx.theme().clone()), Theme::dark());
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Dark);
    // The flat colours back into a theme are the dark base, every token.
    assert_eq!(
        Theme::from_ui_theme(&UiTheme::default_dark()),
        Theme::dark()
    );
}

#[gpui::test]
fn set_theme_notifies_observers(cx: &mut TestAppContext) {
    let (themes, chrome) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let _subscriptions = cx.update(|cx| {
        let (themes, chrome) = (themes.clone(), chrome.clone());
        [
            cx.observe_global::<GlobalTheme>(move |_| themes.set(themes.get() + 1)),
            cx.observe_global::<ActiveUiTheme>(move |_| chrome.set(chrome.get() + 1)),
        ]
    });
    cx.update(|cx| set_theme(cx, Theme::light()));
    cx.run_until_parked();

    assert_eq!((themes.get(), chrome.get()), (1, 1));
    assert_eq!(name(cx), "Kiln Light");
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Light);
    // The flat chrome colours follow the theme.
    assert_eq!(cx.read(ui_theme), Theme::light().ui_theme());
    assert_eq!(cx.read(|cx| cx.global::<GlobalTheme>().mode()), None);
}

#[gpui::test]
fn a_theme_file_resolves_to_gpui_colors(cx: &mut TestAppContext) {
    let paper = theme(
        r##"
        name = "Paper"
        appearance = "light"
        [accent]
        base = "#c84f7c"
        "##,
    );
    cx.update(|cx| set_theme(cx, paper));
    let accent = Color::rgb(0xc84f7c);
    cx.read(|cx| {
        let colors = &cx.theme().colors;
        assert_eq!(colors.accent.base, hsla(accent));
        assert_eq!(colors.accent.base, rgb(0xc84f7c).into());
        assert_eq!(colors.list.row_selected, hsla(accent.with_alpha(0x33)));
        assert_eq!(
            colors.surface.background,
            hsla(ThemeData::light().tokens.surface.background)
        );
        assert_eq!(cx.theme().data().name, "Paper");
    });
}

/// The older way in still works, and moves `cx.theme()` with it.
#[gpui::test]
fn set_ui_theme_sets_the_theme_too(cx: &mut TestAppContext) {
    let paper = UiTheme {
        background: rgb(0xfafafa).into(),
        text: rgb(0x202020).into(),
        accent: rgb(0xc84f7c).into(),
        ..UiTheme::default_dark()
    };
    cx.update(|cx| set_ui_theme(cx, paper));
    assert_eq!(cx.read(ui_theme), paper);
    cx.read(|cx| {
        let colors = &cx.theme().colors;
        assert_eq!(cx.theme().appearance, Appearance::Light);
        assert_eq!(colors.surface.background, paper.background);
        assert_eq!(colors.text.primary, paper.text);
        assert_eq!(colors.accent.base, paper.accent);
        // Not a chrome colour, so derived from the accent it changed.
        assert_eq!(colors.border.focused, paper.accent);
    });
}

#[gpui::test]
fn theme_mode_picks_from_the_pair(cx: &mut TestAppContext) {
    let pair = |mode, cx: &mut TestAppContext| {
        cx.update(|cx| set_themes(cx, mode, Theme::dark(), Theme::light()));
        name(cx)
    };
    assert_eq!(pair(ThemeMode::Dark, cx), "Kiln Dark");
    assert_eq!(pair(ThemeMode::Light, cx), "Kiln Light");
    // The test platform's appearance is light.
    assert_eq!(pair(ThemeMode::System, cx), "Kiln Light");
    assert_eq!(
        cx.read(|cx| cx.global::<GlobalTheme>().mode()),
        Some(ThemeMode::System)
    );

    let changes = Rc::new(Cell::new(0));
    let _subscription = cx.update(|cx| {
        let changes = changes.clone();
        cx.observe_global::<GlobalTheme>(move |_| changes.set(changes.get() + 1))
    });
    cx.update(|cx| system_appearance_changed(cx, WindowAppearance::VibrantDark));
    cx.run_until_parked();
    assert_eq!(name(cx), "Kiln Dark");
    assert_eq!(changes.get(), 1);
    // No change, no notification.
    cx.update(|cx| system_appearance_changed(cx, WindowAppearance::Dark));
    cx.run_until_parked();
    assert_eq!(changes.get(), 1);

    // A fixed mode, or a theme set alone, ignores the system.
    pair(ThemeMode::Light, cx);
    cx.update(|cx| system_appearance_changed(cx, WindowAppearance::Dark));
    assert_eq!(name(cx), "Kiln Light");
    cx.update(|cx| set_theme(cx, Theme::light()));
    cx.update(|cx| system_appearance_changed(cx, WindowAppearance::Dark));
    assert_eq!(name(cx), "Kiln Light");
}

/// Counts its renders, drawing each control.
struct Form {
    renders: Rc<Cell<u32>>,
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div()
            .child(Button::new("ok", "OK").primary())
            .child(Button::new("cancel", "Cancel"))
            .child(Button::new("delete", "Delete").danger().disabled(true))
            .child(Checkbox::new("wrap").checked(true).label("Wrap"))
            .child(Switch::new("sync").on(true).label("Sync"))
    }
}

#[gpui::test]
fn controls_render_with_tokens(cx: &mut TestAppContext) {
    let renders = Rc::new(Cell::new(0));
    let (_, cx) = cx.add_window_view(|_, _| Form {
        renders: renders.clone(),
    });
    cx.run_until_parked();
    let before = renders.get();
    assert!(before > 0);

    let paper = theme(
        r##"
        appearance = "light"
        [accent]
        base = "#c84f7c"
        [text]
        on_accent = "#fffff0"
        "##,
    );
    cx.update(|_, cx| set_theme(cx, paper));
    cx.run_until_parked();
    assert!(renders.get() > before, "setting a theme redraws the window");

    cx.update(|_, cx| {
        let colors = &cx.theme().colors;
        let (bg, fg, border, hover) = ButtonVariant::Primary.colors(colors);
        assert_eq!(bg, hsla(Color::rgb(0xc84f7c)));
        assert_eq!(border, bg);
        assert_eq!(fg, hsla(Color::rgb(0xfffff0)));
        assert_eq!(hover, colors.accent.hover);
        let (bg, fg, border, _) = ButtonVariant::Secondary.colors(colors);
        assert_eq!(
            (bg, fg, border),
            (
                colors.surface.panel,
                colors.text.primary,
                colors.border.default
            )
        );
        assert_eq!(
            ButtonVariant::Danger.colors(colors).0,
            hsla(ThemeData::light().tokens.status.error)
        );
    });
}
