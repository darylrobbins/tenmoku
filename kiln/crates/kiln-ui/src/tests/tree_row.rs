// SPDX-License-Identifier: LicenseRef-Proprietary

use crate::{TreeRow, UiTheme};

#[test]
fn colors_follow_state() {
    let theme = UiTheme::default_dark();
    let row = || TreeRow::new("row", "name", &theme);

    let focused = row().selected(true).focused(true).colors();
    assert_eq!(focused.background, Some(theme.row_selected));
    assert_eq!(focused.border, Some(theme.row_focus_border));
    let unfocused = row().selected(true).colors();
    assert_eq!(unfocused.background, Some(theme.row_selected_inactive));
    assert_eq!(unfocused.border, None);
    for unselected in [row(), row().focused(true)] {
        let colors = unselected.colors();
        assert_eq!((colors.background, colors.border), (None, None));
    }

    for ignored in [row().ignored(true), row().directory(true).ignored(true)] {
        let colors = ignored.colors();
        assert_eq!(colors.label, theme.muted_text);
        assert_eq!(colors.icon, theme.muted_text);
        assert_eq!(colors.chevron, theme.muted_text);
    }
    for error in [row().error(true), row().directory(false).error(true)] {
        let colors = error.colors();
        assert_eq!(colors.label, theme.error_text);
        assert_eq!(colors.icon, theme.error_text);
    }

    let dir = row().directory(false).colors();
    assert_eq!(
        (dir.label, dir.icon, dir.chevron),
        (theme.text, theme.folder_icon, theme.chevron)
    );
    let file = row().colors();
    assert_eq!((file.label, file.icon), (theme.text, theme.icon));
}
