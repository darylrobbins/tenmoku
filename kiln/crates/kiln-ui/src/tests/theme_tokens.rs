// SPDX-License-Identifier: Apache-2.0

//! The active theme's tokens: `cx.theme()`, setting a theme or a dark and
//! light pair, the flat chrome colours as a view of it, and controls drawn
//! from it.

use std::{cell::Cell, path::Path, rc::Rc};

use gpui::{
    Context, IntoElement, ParentElement as _, Render, TestAppContext, Window, WindowAppearance,
    div, rgb,
};
use kiln_core::theme::{Contrast, ThemeFile};
use kiln_platform::system_a11y::{SystemA11y, set_system_a11y};

use crate::{
    ActiveTheme as _, ActiveUiTheme, Appearance, Button, ButtonVariant, Checkbox, Color,
    GlobalTheme, Switch, Theme, ThemeData, ThemeMode, Tokens, UiTheme, hsla, set_theme, set_themes,
    set_ui_theme, system_appearance_changed,
    theme::{set_platform_appearance, system_appearance},
    ui_theme,
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
    // The flat colours back into a theme are the dark base, every token but
    // the scrollbar thumb (tests::ui_theme_compat).
    assert_eq!(
        Theme::from_ui_theme(&UiTheme::default_dark()).data(),
        Theme::dark().data()
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

/// The design's new roles reach `cx.theme()` as GPUI colours, as every
/// token does; a theme's accent leaves the agent's colour alone.
#[gpui::test]
fn the_design_roles_resolve_to_gpui_colors(cx: &mut TestAppContext) {
    let paper = theme(
        r##"
        appearance = "dark"
        [accent]
        base = "#c84f7c"
        "##,
    );
    cx.update(|cx| set_theme(cx, paper));
    let base = &ThemeData::dark().tokens;
    cx.read(|cx| {
        let colors = &cx.theme().colors;
        assert_eq!(colors.surface.chrome, colors.surface.panel);
        assert_eq!(colors.border.control, colors.border.default);
        assert_eq!(colors.text.faint, hsla(base.text.faint));
        assert_eq!(colors.status.modified, hsla(base.status.modified));
        assert_eq!(colors.vcs.deleted, colors.status.error);
        assert_eq!(colors.agent.base, hsla(base.agent.base));
        assert_eq!(colors.agent.soft, hsla(base.agent.base.with_alpha(0x26)));
        assert_ne!(colors.agent.base, colors.accent.base);
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
        let primary = ButtonVariant::Primary.look(false, colors);
        let (bg, fg, border, hover) = (
            primary.background,
            primary.text,
            primary.border,
            primary.hover,
        );
        assert_eq!(bg, hsla(Color::rgb(0xc84f7c)));
        assert_eq!(border, bg);
        assert_eq!(fg, hsla(Color::rgb(0xfffff0)));
        assert_eq!(hover, colors.accent.hover);
        let secondary = ButtonVariant::Secondary.look(false, colors);
        assert_eq!(
            (secondary.background, secondary.text, secondary.border),
            (
                gpui::transparent_black(),
                colors.text.primary,
                colors.border.control
            )
        );
        assert_eq!(
            ButtonVariant::Danger.look(false, colors).background,
            hsla(ThemeData::light().tokens.status.error)
        );
    });
}

/// Every background text is drawn on in `tokens`, worked out here rather
/// than by kiln-core's `text_grounds`, so a ground the pass forgets is
/// still checked: the four surfaces over the window's background, and a
/// hovered, selected or inactive-selected row (by the state tokens and the
/// list's own) over each surface.
fn rendered_grounds(tokens: &Tokens) -> Vec<(String, Color)> {
    use kiln_core::theme::over;

    let background = over(tokens.surface.background, Color::rgb(0));
    let surfaces = [
        ("background", background),
        ("panel", over(tokens.surface.panel, background)),
        ("chrome", over(tokens.surface.chrome, background)),
        ("raised", over(tokens.surface.raised, background)),
    ];
    let rows = [
        ("state.hover", tokens.state.hover),
        ("state.selected", tokens.state.selected),
        ("state.selected_inactive", tokens.state.selected_inactive),
        ("list.row_hover", tokens.list.row_hover),
        ("list.row_selected", tokens.list.row_selected),
        (
            "list.row_selected_inactive",
            tokens.list.row_selected_inactive,
        ),
    ];
    let mut grounds: Vec<(String, Color)> = (surfaces.iter())
        .map(|&(name, color)| (name.to_owned(), color))
        .collect();
    for (row, color) in rows {
        for (surface, under) in surfaces {
            grounds.push((format!("{row} over {surface}"), over(color, under)));
        }
    }
    grounds
}

/// Checks the high-contrast pass took `normal` to `high` as the rule says
/// (`kiln_core::theme::high_contrast`), naming the theme in each failure.
fn assert_high_contrast(name: &str, normal: &Tokens, high: &Tokens) {
    use std::collections::BTreeSet;

    use kiln_core::theme::{
        HIGH_CONTRAST_FAINT, HIGH_CONTRAST_TEXT, RULES, TOKEN_KEYS, contrast_ratio, over,
    };

    let grounds = rendered_grounds(high);
    let ratio = |text: Color, (ground, color): &(String, Color)| {
        (contrast_ratio(over(text, *color), *color), ground.clone())
    };
    // Muted text reads at 7:1 and faint at 4.5:1 on every surface and every
    // hovered or selected row; primary text never reads weaker than muted.
    for ground in &grounds {
        let (muted, on) = ratio(high.text.muted, ground);
        assert!(
            muted >= HIGH_CONTRAST_TEXT,
            "{name}: muted {muted:.2}:1 on {on}"
        );
        let (faint, on) = ratio(high.text.faint, ground);
        assert!(
            faint >= HIGH_CONTRAST_FAINT,
            "{name}: faint {faint:.2}:1 on {on}"
        );
        let (primary, on) = ratio(high.text.primary, ground);
        assert!(
            primary >= muted - 0.01,
            "{name}: primary {primary:.2}:1 on {on}"
        );
    }
    assert_eq!((high.text.muted.a(), high.text.faint.a()), (0xff, 0xff));
    // Borders and dividers are at least as strong as the muted text, and
    // the muted text's colour where they were weaker.
    let background = grounds[0].1;
    let strength = |color| contrast_ratio(over(color, background), background);
    let borders = [
        ("border.default", normal.border.default, high.border.default),
        ("border.subtle", normal.border.subtle, high.border.subtle),
        ("border.control", normal.border.control, high.border.control),
    ];
    for (key, before, after) in borders {
        assert!(
            strength(after) >= strength(high.text.muted) - 0.01,
            "{name}: {key} {:.2}:1",
            strength(after)
        );
        let expected = match strength(before) >= strength(high.text.muted) {
            true => before,
            false => high.text.muted,
        };
        assert_eq!(after, expected, "{name}: {key}");
    }
    // The focus ring and a focused row's outline are opaque, in their own
    // colour.
    for (key, before, after) in [
        (
            "state.focus_ring",
            normal.state.focus_ring,
            high.state.focus_ring,
        ),
        (
            "list.row_focus_border",
            normal.list.row_focus_border,
            high.list.row_focus_border,
        ),
    ] {
        assert_eq!(after, before.with_alpha(0xff), "{name}: {key}");
    }
    // The selections are at twice their alpha, up to opaque, and hovered
    // rows as they were, unless that leaves no room for 7:1 text: then
    // each is at the most alpha that does, in its own colour.
    let surfaces: Vec<Color> = grounds[..4].iter().map(|&(_, color)| color).collect();
    let (white, black) = (Color::rgb(0xffffff), Color::rgb(0));
    let least = |text: Color, row: Color| {
        (surfaces.iter())
            .map(|&surface| contrast_ratio(text, over(row, surface)))
            .fold(f32::INFINITY, f32::min)
    };
    let room = |row: Color| least(white, row).max(least(black, row)) >= HIGH_CONTRAST_TEXT;
    for (key, before, after, raised) in [
        ("state.hover", normal.state.hover, high.state.hover, false),
        (
            "list.row_hover",
            normal.list.row_hover,
            high.list.row_hover,
            false,
        ),
        (
            "state.selected",
            normal.state.selected,
            high.state.selected,
            true,
        ),
        (
            "state.selected_inactive",
            normal.state.selected_inactive,
            high.state.selected_inactive,
            true,
        ),
        (
            "list.row_selected",
            normal.list.row_selected,
            high.list.row_selected,
            true,
        ),
        (
            "list.row_selected_inactive",
            normal.list.row_selected_inactive,
            high.list.row_selected_inactive,
            true,
        ),
    ] {
        let wanted = match raised {
            true => before.a().saturating_mul(2),
            false => before.a(),
        };
        assert_eq!(
            after.with_alpha(0xff),
            before.with_alpha(0xff),
            "{name}: {key}"
        );
        assert!(after.a() <= wanted, "{name}: {key}");
        assert!(room(after), "{name}: {key} leaves no room for text");
        if after.a() < wanted {
            assert!(
                !room(after.with_alpha(after.a() + 1)),
                "{name}: {key} lower than needed"
            );
        }
    }
    // Every other token keeps its colour, but those derived from one the
    // pass sets.
    let mut moved: BTreeSet<&str> = [
        "text.primary",
        "text.muted",
        "text.faint",
        "border.default",
        "border.subtle",
        "border.control",
        "state.focus_ring",
        "list.row_focus_border",
        "state.hover",
        "state.selected",
        "state.selected_inactive",
        "list.row_hover",
        "list.row_selected",
        "list.row_selected_inactive",
    ]
    .into();
    for &(key, rule) in RULES {
        if rule.inputs().any(|input| moved.contains(input)) {
            moved.insert(key);
        }
    }
    for key in TOKEN_KEYS.iter().filter(|key| !moved.contains(*key)) {
        assert_eq!(high.get(key), normal.get(key), "{name}: {key} moved");
    }
}

/// kiln's shipped themes, each as `cx.theme()` would have it: the dark and
/// light bases, and the flat default chrome a [`UiTheme`] gives.
fn shipped_themes() -> [Theme; 3] {
    [
        Theme::dark(),
        Theme::light(),
        Theme::from_ui_theme(&UiTheme::default_dark()),
    ]
}

#[test]
fn high_contrast_rule_holds_for_every_shipped_theme() {
    for theme in shipped_themes() {
        let high = theme.with_contrast(Contrast::High);
        let name = theme.name.to_string();
        assert_high_contrast(&name, &theme.data().tokens, &high.data().tokens);
        assert_eq!(high.data().contrast, Contrast::High, "{name}");
        // GPUI's colours follow the tokens the pass moved, and keep the
        // rest exactly as they were.
        for key in kiln_core::theme::TOKEN_KEYS {
            let (normal, after) = (theme.data().tokens.get(key), high.data().tokens.get(key));
            let expected = match normal == after {
                true => theme.colors.get(key).copied(),
                false => after.map(|&color| hsla(color)),
            };
            assert_eq!(high.colors.get(key).copied(), expected, "{name}: {key}");
        }
        // Normal contrast is the theme itself, and the pass is done once.
        assert_eq!(theme.with_contrast(Contrast::Normal), theme);
        assert_eq!(high.with_contrast(Contrast::High), high);
    }
}

/// A theme whose muted text and borders are too faint for anyone, and
/// whose surfaces differ: the pass still lifts it all the way.
#[test]
fn high_contrast_lifts_a_low_contrast_theme() {
    for (appearance, source) in [
        (
            "dark",
            r##"
            appearance = "dark"
            [surface]
            background = "#202020"
            panel = "#2a2a2a"
            raised = "#333333"
            [text]
            primary = "#808080"
            muted = "#404040"
            [border]
            default = "#262626"
            "##,
        ),
        (
            "light",
            r##"
            appearance = "light"
            [surface]
            background = "#f0f0f0"
            panel = "#e0e0e0"
            [text]
            primary = "#707070"
            muted = "#c0c0c0"
            [border]
            default = "#e8e8e8"
            "##,
        ),
    ] {
        let theme = theme(source);
        let high = theme.data().high_contrast();
        assert_high_contrast(appearance, &theme.data().tokens, &high.tokens);
        assert_eq!(
            high.tokens.border.default, high.tokens.text.muted,
            "{appearance}"
        );
    }
}

#[gpui::test]
fn set_contrast_draws_every_theme_at_it(cx: &mut TestAppContext) {
    let changes = Rc::new(Cell::new(0));
    let _subscription = cx.update(|cx| {
        set_themes(cx, ThemeMode::Dark, Theme::dark(), Theme::light());
        let changes = changes.clone();
        cx.observe_global::<GlobalTheme>(move |_| changes.set(changes.get() + 1))
    });
    let active = |cx: &mut TestAppContext| cx.read(|cx| cx.theme().clone());

    cx.update(|cx| GlobalTheme::set_contrast(cx, Contrast::High));
    let high_dark = Theme::dark().with_contrast(Contrast::High);
    assert_eq!(active(cx), high_dark);
    assert_eq!(cx.read(ui_theme), high_dark.ui_theme());
    assert_eq!(
        cx.read(|cx| cx.global::<GlobalTheme>().contrast()),
        Contrast::High
    );
    assert_eq!(changes.get(), 1);
    // The same contrast again changes nothing.
    cx.update(|cx| GlobalTheme::set_contrast(cx, Contrast::High));
    assert_eq!(changes.get(), 1);

    // A theme set later is drawn at it too: by mode, alone, or as flat
    // chrome colours.
    cx.update(|cx| set_themes(cx, ThemeMode::Light, Theme::dark(), Theme::light()));
    assert_eq!(active(cx), Theme::light().with_contrast(Contrast::High));
    cx.update(|cx| set_theme(cx, Theme::dark()));
    assert_eq!(active(cx), high_dark);
    cx.update(|cx| set_ui_theme(cx, UiTheme::default_dark()));
    let flat = Theme::from_ui_theme(&UiTheme::default_dark()).with_contrast(Contrast::High);
    assert_eq!(active(cx), flat);
    assert_eq!(cx.read(ui_theme), flat.ui_theme());

    // Back to normal: the theme exactly as set, and the flat colours as
    // given.
    cx.update(|cx| GlobalTheme::set_contrast(cx, Contrast::Normal));
    assert_eq!(active(cx), Theme::from_ui_theme(&UiTheme::default_dark()));
    assert_eq!(cx.read(ui_theme), UiTheme::default_dark());
    cx.update(|cx| set_themes(cx, ThemeMode::Dark, Theme::dark(), Theme::light()));
    assert_eq!(active(cx), Theme::dark());
}

#[gpui::test]
fn set_contrast_before_any_theme_raises_the_default(cx: &mut TestAppContext) {
    // Normal with no theme set leaves no theme set.
    cx.update(|cx| GlobalTheme::set_contrast(cx, Contrast::Normal));
    assert!(!cx.read(|cx| cx.has_global::<GlobalTheme>()));
    cx.update(|cx| GlobalTheme::set_contrast(cx, Contrast::High));
    assert_eq!(
        cx.read(|cx| cx.theme().clone()),
        Theme::dark().with_contrast(Contrast::High)
    );
}

/// Turns the system's Increase Contrast option on or off in the global
/// kiln-platform's reader keeps.
fn increase_contrast(cx: &mut TestAppContext, on: bool) {
    cx.update(|cx| {
        set_system_a11y(
            cx,
            SystemA11y {
                increase_contrast: on,
                ..Default::default()
            },
        )
    });
}

/// While Increase Contrast is on, GPUI maps the high-contrast appearance
/// names to light and logs "unknown appearance" for them; the system mode
/// then follows the platform's own dark or light reading and never asks
/// GPUI's mapping.
#[gpui::test]
fn increase_contrast_follows_the_platforms_appearance(cx: &mut TestAppContext) {
    cx.update(|cx| set_platform_appearance(cx, || Some(Appearance::Dark)));
    let gpui_asked = Rc::new(Cell::new(0));
    let read = |cx: &mut TestAppContext| {
        let gpui_asked = gpui_asked.clone();
        cx.read(|cx| {
            system_appearance(cx, |_| {
                gpui_asked.set(gpui_asked.get() + 1);
                // What GPUI answers for a high-contrast dark appearance.
                WindowAppearance::Light
            })
        })
    };

    // Off, GPUI's reading stands.
    assert_eq!(read(cx), WindowAppearance::Light);
    assert_eq!(gpui_asked.get(), 1);
    // On, the platform's, without asking GPUI.
    increase_contrast(cx, true);
    assert_eq!(read(cx), WindowAppearance::Dark);
    assert_eq!(
        gpui_asked.get(),
        1,
        "GPUI's mapping asked while contrast is high"
    );
    // A platform with no reader leaves it to GPUI.
    cx.update(|cx| set_platform_appearance(cx, || None));
    assert_eq!(read(cx), WindowAppearance::Light);
    assert_eq!(gpui_asked.get(), 2);

    // The system mode keeps the dark theme in a dark system, where the
    // test platform, like GPUI's mapping, says light; turning the option
    // on picks again.
    cx.update(|cx| set_platform_appearance(cx, || Some(Appearance::Dark)));
    increase_contrast(cx, false);
    cx.update(|cx| set_themes(cx, ThemeMode::System, Theme::dark(), Theme::light()));
    assert_eq!(name(cx), "Kiln Light");
    increase_contrast(cx, true);
    cx.update(|cx| GlobalTheme::set_contrast(cx, Contrast::High));
    assert_eq!(
        cx.read(|cx| cx.theme().clone()),
        Theme::dark().with_contrast(Contrast::High)
    );
    // A theme mode set while it's on picks by the platform too, and so
    // does a light system.
    cx.update(|cx| set_themes(cx, ThemeMode::System, Theme::dark(), Theme::light()));
    assert_eq!(name(cx), "Kiln Dark");
    cx.update(|cx| set_platform_appearance(cx, || Some(Appearance::Light)));
    cx.update(|cx| GlobalTheme::set_contrast(cx, Contrast::High));
    assert_eq!(name(cx), "Kiln Light");
    // The contrast setting alone (normal here) doesn't stop the platform
    // reading: the names are high-contrast whenever the option is on.
    cx.update(|cx| set_platform_appearance(cx, || Some(Appearance::Dark)));
    cx.update(|cx| GlobalTheme::set_contrast(cx, Contrast::Normal));
    assert_eq!(cx.read(|cx| cx.theme().clone()), Theme::dark());
}
