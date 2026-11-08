// SPDX-License-Identifier: Apache-2.0

use super::ui_theme_compat::distinct;
use crate::{Theme, TreeRow, UiTheme};

/// A row's colours for its state are the flat theme's it was given before
/// it read tokens, under any flat theme an app sets.
#[test]
fn colors_follow_state() {
    for theme in [UiTheme::default_dark(), distinct(0.13), distinct(0.93)] {
        let tokens = &Theme::from_ui_theme(&theme).colors;
        let row = || TreeRow::new("row", "name", &theme);

        let focused = row().selected(true).focused(true).colors(tokens);
        assert_eq!(focused.background, Some(theme.row_selected));
        assert_eq!(focused.edge, Some(tokens.accent.base));
        let unfocused = row().selected(true).colors(tokens);
        assert_eq!(unfocused.background, Some(theme.row_selected_inactive));
        assert_eq!(unfocused.edge, None);
        for unselected in [row(), row().focused(true)] {
            let colors = unselected.colors(tokens);
            assert_eq!((colors.background, colors.edge), (None, None));
        }

        for ignored in [row().ignored(true), row().directory(true).ignored(true)] {
            let colors = ignored.colors(tokens);
            assert_eq!(colors.label, theme.muted_text);
            assert_eq!(colors.icon, theme.muted_text);
            assert_eq!(colors.chevron, theme.muted_text);
        }
        for error in [row().error(true), row().directory(false).error(true)] {
            let colors = error.colors(tokens);
            assert_eq!(colors.label, theme.error_text);
            assert_eq!(colors.icon, theme.error_text);
        }

        let dir = row().directory(false).colors(tokens);
        assert_eq!(
            (dir.label, dir.icon, dir.chevron),
            (theme.text, theme.folder_icon, theme.chevron)
        );
        let file = row().colors(tokens);
        assert_eq!((file.label, file.icon), (theme.text, theme.icon));
    }
}
