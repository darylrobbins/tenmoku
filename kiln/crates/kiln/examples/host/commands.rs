// SPDX-License-Identifier: LicenseRef-Proprietary

//! The chat's commands: typed GPUI actions with a title, whose kiln IDs
//! are `chat.new_conversation` and so on (`NewConversation::id()`).
//! `keymap.toml` and `menus.toml` name them by those IDs, and the command
//! palette lists them. Sending is the conversation view's, so it applies
//! only where a conversation has focus; the rest are the window's.

kiln::commands!(
    chat,
    [
        /// New Conversation
        NewConversation,
        /// Send Message
        #[command(context = "conversation")]
        SendMessage,
        /// Close Conversation
        CloseConversation,
        /// Split Right
        SplitRight,
        /// Split Down
        SplitDown,
        /// Toggle Conversations
        ToggleConversations,
        /// Toggle Theme
        ToggleTheme,
    ]
);
