//! The palettes' picker is kiln-shell's ([`kiln_shell::picker`]): a
//! [`TextInput`](fe_ui::TextInput) whose text a [`PickerDelegate`]
//! matches, the matches below it, one selected. Quick open is a delegate,
//! and so are the language, toolbar and navigation pickers.
//!
//! The picker's keys (`picker.*`, and `text_input.*` for any input) are
//! kiln-shell's; [`init`] installs them. `quick_open.toggle` and
//! `go_to_line.toggle` are typed commands on the workbench.
//!
//! With vim on, the pickers have telescope's insert and normal modes
//! (`kiln_shell::picker`'s module docs): [`init`] hands kiln-shell
//! [`fe_gpui::vim_enabled`] as its switch.

use gpui::App;

#[cfg(test)]
pub(crate) use kiln_shell::picker::VISIBLE_ROWS;
pub use kiln_shell::picker::{Picker, PickerDelegate};
pub(crate) use kiln_shell::picker::{PickerOp as PaletteOp, op};

/// Installs the picker's commands, and has the pickers take vim keys while
/// vim is on ([`fe_gpui::vim_enabled`]).
pub(crate) fn init(cx: &mut App) {
    kiln_shell::picker::init(cx);
    kiln_shell::picker::set_vim_keys(cx, fe_gpui::vim_enabled);
}
