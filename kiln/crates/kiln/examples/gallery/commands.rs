// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery's commands: typed GPUI actions with a title, whose kiln IDs
//! are `gallery.next_page` and so on. `keymap.toml` and `menus.toml` name
//! them by those IDs; the views handle them with `.on_action(..)`.

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
        /// Quit Kiln Gallery
        Quit,
        /// Delete Backward
        #[command(context = "text_input")]
        InputBackspace,
        /// Delete Word Backward
        #[command(context = "text_input")]
        InputDeleteWord,
    ]
);
