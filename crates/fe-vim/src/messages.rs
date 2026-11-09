//! vim's messages, worded in fe-vim's catalogue (`l10n/en-US/vim.ftl`),
//! whose en-US is nvim's English byte for byte wherever nvim has the
//! message (the oracle tests compare them). An E-code such as `E492:`
//! stays outside the catalogue, prepended by [`coded`], as nvim keeps it in
//! every language (`docs/decisions/xte5e7hqsw-i18n-design.md` § 4).
//!
//! Other crates that say one of vim's messages call these, so the keys stay
//! fe-vim's own.

use kiln_l10n::{SharedString, tr};

use crate::ExError;

/// `{code}: {text}`: an E-code, which is an id, before a message.
pub(crate) fn coded(code: &str, text: SharedString) -> String {
    format!("{code}: {text}")
}

/// `E492: Not an editor command: {line}`.
pub fn not_an_editor_command(line: &str) -> String {
    ExError::NotACommand(line.to_string()).to_string()
}

/// `E481: No range allowed`.
pub fn no_range_allowed() -> String {
    ExError::NoRange.to_string()
}

/// `E20: Mark not set`.
pub fn mark_not_set() -> String {
    ExError::MarkNotSet.to_string()
}

/// `E35: No previous regular expression`.
pub fn no_previous_regular_expression() -> String {
    ExError::NoPreviousSearch.to_string()
}

/// `E486: Pattern not found: {pattern}`.
pub fn pattern_not_found(pattern: &str) -> String {
    ExError::NotFound(pattern.to_string()).to_string()
}

/// What a shell command asked for by anyone but the user, or run by `:g`
/// or `:normal`, says: `Shell commands run only from your own keys`.
pub fn shell_refused() -> String {
    ExError::ShellRefused.to_string()
}

/// `E19: Mark has invalid line number`.
pub fn mark_has_invalid_line_number() -> String {
    coded("E19", tr!("vim-e19"))
}
