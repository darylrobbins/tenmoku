// SPDX-License-Identifier: Apache-2.0

//! A tree row's status: its label in a colour of its own, and a badge at
//! its right end.

use gpui::{
    Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div, hsla, px,
};

use super::ui_theme_compat::distinct;
use crate::{Theme, TreeRow, UiTheme};

/// The label takes its colour, but an error row keeps the error colour and
/// an ignored row stays muted; the icon and chevron keep theirs.
#[test]
fn label_color_tints_only_a_plain_label() {
    let tint = hsla(0.3, 0.7, 0.5, 1.);
    for theme in [UiTheme::default_dark(), distinct(0.13), distinct(0.93)] {
        let tokens = &Theme::from_ui_theme(&theme).colors;
        let row = || TreeRow::new("row", "name", &theme).label_color(tint);

        let file = row().colors(tokens);
        assert_eq!((file.label, file.icon), (tint, theme.icon));
        let dir = row().directory(true).colors(tokens);
        assert_eq!(
            (dir.label, dir.icon, dir.chevron),
            (tint, theme.folder_icon, theme.chevron)
        );
        for error in [row().error(true), row().directory(false).error(true)] {
            assert_eq!(error.colors(tokens).label, theme.error_text);
        }
        for ignored in [row().ignored(true), row().directory(true).ignored(true)] {
            assert_eq!(ignored.colors(tokens).label, theme.muted_text);
        }
        assert_eq!(file.badge, None);
        let badged = row().badge("M", tint).ignored(true).colors(tokens);
        assert_eq!(badged.badge, Some(tint));
    }
}

const WIDTH: f32 = 200.;
const LONG: &str = "a_file_name_far_too_long_to_fit_in_the_row.rs";

/// Two rows badged M: a short label and one too long for the row.
struct Rows;

impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = UiTheme::default_dark();
        let color = hsla(0.3, 0.7, 0.5, 1.);
        div()
            .w(px(WIDTH))
            .text_size(px(12.))
            .child(TreeRow::new("short", "a.rs", &theme).badge("M", color))
            .child(
                TreeRow::new("long", LONG, &theme)
                    .depth(2)
                    .badge("M", color),
            )
    }
}

#[gpui::test]
fn a_badge_sits_at_the_right_end_after_an_ellipsized_label(cx: &mut TestAppContext) {
    let (_, cx) = cx.add_window_view(|_, _| Rows);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let bounds = |selector: String, cx: &mut VisualTestContext| {
        cx.debug_bounds(selector.leak()).expect("drawn")
    };
    let m = cx.update(|window, _| {
        let style = window.text_style();
        let run = gpui::TextRun {
            len: 1,
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line =
            window
                .text_system()
                .shape_line("M".into(), px(12.), std::slice::from_ref(&run), None);
        line.width()
    });
    // The row's right padding is 8 px.
    let right_end = px(WIDTH - 8.);
    for label in ["a.rs", LONG] {
        let badge = bounds(format!("tree-badge:{label}"), cx);
        assert_eq!(badge.right(), right_end, "{label}");
        // Layout rounds the text's width up to a whole pixel.
        let width = badge.size.width;
        assert!(width >= m && width < m + px(1.), "{label}: an M, {width:?}");
        let text = bounds(format!("tree-label:{label}"), cx);
        assert!(text.right() < badge.left(), "{label}: {text:?} {badge:?}");
    }
    // The long label gave way: it fills what the badge leaves.
    let long = bounds(format!("tree-label:{LONG}"), cx);
    let badge = bounds(format!("tree-badge:{LONG}"), cx);
    assert_eq!(long.right() + px(6.), badge.left());
}
