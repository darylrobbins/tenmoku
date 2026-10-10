// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery's commands: typed GPUI actions with a title, whose kiln IDs
//! are `gallery.next_page` and so on (`NextPage::id()`). `keymap.toml` and
//! `menus.toml` name them by those IDs; the views handle them with
//! `.on_action(..)`. Quitting is kiln's own `app.quit`.

kiln::commands!(
    gallery,
    [
        /// Next Page
        NextPage,
        /// Previous Page
        PreviousPage,
        /// Toggle Theme
        ToggleTheme,
        /// Show Prompt
        ShowPrompt,
        /// Delete Backward
        #[command(context = "text_input")]
        InputBackspace,
        /// Delete Word Backward
        #[command(context = "text_input")]
        InputDeleteWord,
    ]
);
