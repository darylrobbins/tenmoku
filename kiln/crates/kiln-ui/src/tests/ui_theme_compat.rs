// SPDX-License-Identifier: Apache-2.0

//! The chrome looks the same after its widgets moved from the flat
//! `UiTheme` to `cx.theme()`'s tokens: for any `UiTheme` an app sets, each
//! token a widget now reads is the colour the widget drew before.

use gpui::{Hsla, TestAppContext, hsla, rgba};

use crate::{ActiveTheme as _, Theme, Tokens, UiTheme, set_ui_theme};

/// What a widget drew from a `UiTheme`, and what it reads now.
struct Read {
    /// Who reads it, for the failure message.
    what: &'static str,
    before: fn(&UiTheme) -> Hsla,
    now: fn(&Tokens<Hsla>) -> Hsla,
}

/// Every `UiTheme` field, the token the ported widgets read in its place,
/// and the colours they computed from the flat theme.
const READS: &[Read] = &[
    Read {
        what: "background: TextField",
        before: |ui| ui.background,
        now: |c| c.surface.background,
    },
    Read {
        what: "panel_background: ActivityRail, MenuPanel, PaletteFrame, Prompt, StatusBarFrame, \
               TabBar, Tooltip",
        before: |ui| ui.panel_background,
        now: |c| c.surface.panel,
    },
    Read {
        what: "border: ActivityRail, MenuPanel, PaletteFrame, Prompt, SplitPane, StatusBarFrame, \
               TabBar, Tab, TextField, Tooltip",
        before: |ui| ui.border,
        now: |c| c.border.default,
    },
    Read {
        what: "text: HighlightedLabel, MenuStrip, MenuPanel, PaletteFrame, Prompt, \
               StatusBarFrame, Tab, TreeRow",
        before: |ui| ui.text,
        now: |c| c.text.primary,
    },
    Read {
        what: "muted_text: Breadcrumbs, IconButton, MenuPanel, PaletteFrame, Prompt, Tab, \
               TextField, TextInput, ToggleButton, Tooltip, TreeRow",
        before: |ui| ui.muted_text,
        now: |c| c.text.muted,
    },
    Read {
        what: "error_text: Prompt, TreeRow",
        before: |ui| ui.error_text,
        now: |c| c.status.error,
    },
    Read {
        what: "row_hover: Breadcrumbs, IconButton, MenuStrip, Prompt, RailButton, StatusEntry, \
               Tab, ToggleButton, TreeRow",
        before: |ui| ui.row_hover,
        now: |c| c.list.row_hover,
    },
    Read {
        what: "row_selected: MenuStrip, MenuPanel, TextField, ToggleButton, TreeRow",
        before: |ui| ui.row_selected,
        now: |c| c.list.row_selected,
    },
    Read {
        what: "row_selected_inactive: TreeRow",
        before: |ui| ui.row_selected_inactive,
        now: |c| c.list.row_selected_inactive,
    },
    Read {
        what: "row_focus_border: TextField, TreeRow",
        before: |ui| ui.row_focus_border,
        now: |c| c.list.row_focus_border,
    },
    Read {
        what: "icon: Breadcrumbs, IconButton, RailButton, TreeRow",
        before: |ui| ui.icon,
        now: |c| c.list.icon,
    },
    Read {
        what: "folder_icon: TreeRow",
        before: |ui| ui.folder_icon,
        now: |c| c.list.folder_icon,
    },
    Read {
        what: "chevron: TreeRow",
        before: |ui| ui.chevron,
        now: |c| c.list.chevron,
    },
    Read {
        what: "indent_guide: TreeRow",
        before: |ui| ui.indent_guide,
        now: |c| c.list.indent_guide,
    },
    Read {
        what: "tab_active: Tab",
        before: |ui| ui.tab_active,
        now: |c| c.tab.active,
    },
    Read {
        what: "tab_active_text: HighlightedLabel, MenuStrip, MenuPanel, Prompt, RailButton, Tab, \
               TextField, TextInput, ToggleButton, Tooltip",
        before: |ui| ui.tab_active_text,
        now: |c| c.tab.active_text,
    },
    Read {
        what: "tab_inactive: Tab",
        before: |ui| ui.tab_inactive,
        now: |c| c.tab.inactive,
    },
    Read {
        what: "dirty_dot: Tab",
        before: |ui| ui.dirty_dot,
        now: |c| c.state.dirty,
    },
    Read {
        what: "drop_target: the app's own drop markers",
        before: |ui| ui.drop_target,
        now: |c| c.state.drop_target,
    },
    Read {
        what: "accent: Prompt, RailButton",
        before: |ui| ui.accent,
        now: |c| c.accent.base,
    },
    Read {
        what: "the thumb: ScrollArea",
        before: |ui| ui.muted_text.opacity(0.5),
        now: |c| c.scrollbar.thumb,
    },
    Read {
        what: "the hovered thumb: ScrollArea",
        before: |ui| ui.text.opacity(0.6),
        now: |c| c.scrollbar.thumb_hover,
    },
    Read {
        what: "the scrim: Prompt",
        before: |_| rgba(0x00000080).into(),
        now: |c| c.surface.overlay,
    },
];

/// A flat theme with every field a different colour, none of them a whole
/// 8-bit colour, on a background `lightness` light.
pub(super) fn distinct(lightness: f32) -> UiTheme {
    let mut next = 0.;
    let mut color = || {
        next += 1.;
        hsla(
            next / 23. + 0.0137,
            0.613,
            0.2 + next / 31.,
            0.71 + next / 97.,
        )
    };
    UiTheme {
        background: hsla(0.613, 0.137, lightness, 1.),
        panel_background: color(),
        border: color(),
        text: color(),
        muted_text: color(),
        error_text: color(),
        row_hover: color(),
        row_selected: color(),
        row_selected_inactive: color(),
        row_focus_border: color(),
        icon: color(),
        folder_icon: color(),
        chevron: color(),
        indent_guide: color(),
        tab_active: color(),
        tab_active_text: color(),
        tab_inactive: color(),
        dirty_dot: color(),
        drop_target: color(),
        accent: color(),
    }
}

/// The fields of `ui`, by reading.
fn fields(ui: &UiTheme) -> Vec<Hsla> {
    READS[..20].iter().map(|read| (read.before)(ui)).collect()
}

#[test]
fn every_field_is_read_and_the_test_themes_are_distinct() {
    // READS starts with one entry per field, in UiTheme's order.
    let ui = distinct(0.13);
    let named = [
        ui.background,
        ui.panel_background,
        ui.border,
        ui.text,
        ui.muted_text,
        ui.error_text,
        ui.row_hover,
        ui.row_selected,
        ui.row_selected_inactive,
        ui.row_focus_border,
        ui.icon,
        ui.folder_icon,
        ui.chevron,
        ui.indent_guide,
        ui.tab_active,
        ui.tab_active_text,
        ui.tab_inactive,
        ui.dirty_dot,
        ui.drop_target,
        ui.accent,
    ];
    assert_eq!(fields(&ui), named);
    for ui in [distinct(0.13), distinct(0.93)] {
        let fields = fields(&ui);
        for (ix, color) in fields.iter().enumerate() {
            assert!(!fields[..ix].contains(color), "field {ix} repeats");
        }
    }
}

#[gpui::test]
fn set_ui_theme_keeps_every_widget_color(cx: &mut TestAppContext) {
    let themes = [
        ("default dark", UiTheme::default_dark()),
        ("distinct dark", distinct(0.13)),
        ("distinct light", distinct(0.93)),
    ];
    for (name, ui) in themes {
        cx.update(|cx| set_ui_theme(cx, ui));
        cx.read(|cx| {
            let tokens = &cx.theme().colors;
            for read in READS {
                assert_eq!(
                    (read.now)(tokens),
                    (read.before)(&ui),
                    "{name}: {}",
                    read.what
                );
            }
        });
    }
}

/// With no theme set, as an app that only draws with
/// `UiTheme::default_dark()` has it, the dark base's tokens are those
/// colours. The thumb is a theme file's 8-bit colour, at 128/255 alpha
/// rather than exactly half.
#[gpui::test]
fn the_dark_base_is_the_default_dark_chrome(cx: &mut TestAppContext) {
    let ui = UiTheme::default_dark();
    cx.read(|cx| {
        let tokens = &cx.theme().colors;
        for read in READS {
            let (now, before) = ((read.now)(tokens), (read.before)(&ui));
            if read.what == "the thumb: ScrollArea" {
                assert_eq!(
                    now,
                    Hsla {
                        a: 128. / 255.,
                        ..before
                    }
                );
            } else {
                assert_eq!(now, before, "{}", read.what);
            }
        }
    });
    // The flat colours back into a theme are the dark base, but for that.
    let flat = Theme::from_ui_theme(&ui);
    assert_eq!(flat.data(), Theme::dark().data());
    let mut colors = flat.colors;
    colors.scrollbar.thumb = Theme::dark().colors.scrollbar.thumb;
    assert_eq!(colors, Theme::dark().colors);
}
