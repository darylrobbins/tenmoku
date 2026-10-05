//! The default (macOS) key bindings, as plain data: which command each
//! keystroke runs, and in what context. [`crate::init`] turns every row into
//! a GPUI binding of [`crate::commands::RunCommand`].

use crate::KEY_CONTEXT;

/// One key binding: `keys` (GPUI keystroke syntax, space-separated for a
/// sequence) runs `command` while `context` holds; `None` is everywhere.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DefaultBinding {
    pub command: &'static str,
    pub keys: &'static str,
    pub context: Option<&'static str>,
}

const fn editor(keys: &'static str, command: &'static str) -> DefaultBinding {
    DefaultBinding {
        command,
        keys,
        context: Some(KEY_CONTEXT),
    }
}

/// Every default binding.
pub const DEFAULT_BINDINGS: &[DefaultBinding] = &[
    editor("left", "editor.move_left"),
    editor("right", "editor.move_right"),
    editor("up", "editor.move_up"),
    editor("down", "editor.move_down"),
    editor("alt-left", "editor.move_word_left"),
    editor("alt-right", "editor.move_word_right"),
    editor("cmd-left", "editor.move_line_start"),
    editor("home", "editor.move_line_start"),
    editor("cmd-right", "editor.move_line_end"),
    editor("end", "editor.move_line_end"),
    editor("cmd-up", "editor.move_document_start"),
    editor("cmd-down", "editor.move_document_end"),
    editor("pageup", "editor.page_up"),
    editor("pagedown", "editor.page_down"),
    editor("shift-left", "editor.select_left"),
    editor("shift-right", "editor.select_right"),
    editor("shift-up", "editor.select_up"),
    editor("shift-down", "editor.select_down"),
    editor("alt-shift-left", "editor.select_word_left"),
    editor("alt-shift-right", "editor.select_word_right"),
    editor("cmd-shift-left", "editor.select_line_start"),
    editor("shift-home", "editor.select_line_start"),
    editor("cmd-shift-right", "editor.select_line_end"),
    editor("shift-end", "editor.select_line_end"),
    editor("cmd-shift-up", "editor.select_document_start"),
    editor("cmd-shift-down", "editor.select_document_end"),
    editor("shift-pageup", "editor.select_page_up"),
    editor("shift-pagedown", "editor.select_page_down"),
    editor("cmd-a", "editor.select_all"),
    editor("backspace", "editor.backspace"),
    editor("shift-backspace", "editor.backspace"),
    editor("delete", "editor.delete"),
    editor("alt-backspace", "editor.delete_word_left"),
    editor("enter", "editor.newline"),
    editor("tab", "editor.tab"),
    editor("cmd-alt-[", "editor.fold"),
    editor("cmd-alt-]", "editor.unfold"),
    editor("cmd-k cmd-0", "editor.fold_all"),
    editor("cmd-k cmd-j", "editor.unfold_all"),
    editor("alt-z", "editor.toggle_soft_wrap"),
    editor("cmd-c", "editor.copy"),
    editor("cmd-x", "editor.cut"),
    editor("cmd-v", "editor.paste"),
    editor("cmd-z", "editor.undo"),
    editor("cmd-shift-z", "editor.redo"),
    editor("cmd-s", "editor.save"),
    DefaultBinding {
        command: "app.quit",
        keys: "cmd-q",
        context: None,
    },
];
