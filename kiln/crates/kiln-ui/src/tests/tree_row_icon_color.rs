// SPDX-License-Identifier: Apache-2.0

//! A row's icon colour: the masked icon's tint, selected or not, unless
//! the row is an error's or ignored.

use gpui::{Hsla, hsla};

use crate::{Theme, TreeRow, UiTheme};

fn tint() -> Hsla {
    hsla(0.3, 0.6, 0.5, 1.)
}

/// The icon takes the colour unselected, selected, and selected and
/// focused; the background and edge are as an uncoloured row's.
#[test]
fn an_icon_colour_tints_the_icon_in_every_selection_state() {
    let theme = UiTheme::default_dark();
    let tokens = &Theme::from_ui_theme(&theme).colors;
    let row = || TreeRow::new("row", "name", &theme).icon_color(tint());
    assert_eq!(row().icon_tint(), Some(tint()));
    assert_eq!(TreeRow::new("row", "name", &theme).icon_tint(), None);

    let unselected = row().colors(tokens);
    assert_eq!((unselected.background, unselected.edge), (None, None));
    assert_eq!((unselected.label, unselected.icon), (theme.text, tint()));

    let unfocused = row().selected(true).colors(tokens);
    assert_eq!(unfocused.background, Some(theme.row_selected_inactive));
    assert_eq!(unfocused.edge, None);
    assert_eq!(unfocused.icon, tint());

    let focused = row().selected(true).focused(true).colors(tokens);
    assert_eq!(focused.background, Some(theme.row_selected));
    assert_eq!(focused.edge, Some(tokens.accent.base));
    assert_eq!(focused.icon, tint());

    let dir = row().directory(true).selected(true).colors(tokens);
    assert_eq!((dir.icon, dir.chevron), (tint(), theme.chevron));
}

/// An error row's icon is in the error colour, and an ignored row's keeps
/// the list's, whatever icon colour it was given.
#[test]
fn error_and_ignored_rows_still_win_over_an_icon_colour() {
    let theme = UiTheme::default_dark();
    let tokens = &Theme::from_ui_theme(&theme).colors;
    let row = || TreeRow::new("row", "name", &theme).icon_color(tint());

    for error in [row().error(true), row().selected(true).error(true)] {
        let colors = error.colors(tokens);
        assert_eq!(
            (colors.label, colors.icon),
            (theme.error_text, theme.error_text)
        );
    }
    let ignored = row().ignored(true).colors(tokens);
    assert_eq!(
        (ignored.label, ignored.icon),
        (theme.muted_text, theme.icon)
    );
    let ignored = row().directory(false).ignored(true).colors(tokens);
    assert_eq!(
        (ignored.label, ignored.icon),
        (theme.muted_text, theme.folder_icon)
    );
}
