//! Convert Indentation to Spaces / Tabs and Detect Indentation from Content,
//! as VS Code's `editor.action.indentationToSpaces`, `indentationToTabs` and
//! `detectIndentation`.
//!
//! The conversions rewrite the leading whitespace of every row by its width
//! in columns at the model's tab size, text only: a row inside a multi-line
//! string changes too. They set the indent style to match, and make their
//! edits as one transaction.

use std::ops::Range;

use fe_buffer::{ResolvedSelection, Text};
use fe_command::{Effect, Registry};

use crate::{
    EditorModel, IndentStyle,
    commands::{Editor, add},
    detect_indent_style,
    join_lines::map_before,
};

/// Registers the three commands. The conversions edit the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    add(
        registry,
        "convert_indentation_to_spaces",
        true,
        |model, _| Ok(effect(model, |model| model.convert_indentation_to_spaces())),
    );
    add(registry, "convert_indentation_to_tabs", true, |model, _| {
        Ok(effect(model, |model| model.convert_indentation_to_tabs()))
    });
    add(registry, "detect_indentation", false, |model, _| {
        Ok(effect(model, |model| model.detect_indentation()))
    });
}

/// Runs `change` and reports it: `Changed` for an edit, `Notify` when only the
/// indent style moved (which the dispatcher's text comparison can't see), and
/// `Nothing` otherwise.
fn effect(model: &mut EditorModel, change: impl FnOnce(&mut EditorModel) -> bool) -> Effect {
    let version = model.snapshot().version();
    if !change(model) {
        Effect::Nothing
    } else if model.snapshot().version() != version {
        Effect::Changed
    } else {
        Effect::Notify
    }
}

/// The edits that rewrite the leading spaces and tabs of each row of `text`
/// as spaces, or as tabs with the remainder in spaces, with tab stops every
/// `tab_size` columns. Rows already written that way have none.
pub(crate) fn indentation_edits(
    text: &Text,
    tab_size: usize,
    to_tabs: bool,
) -> Vec<(Range<usize>, String)> {
    let tab_size = tab_size.max(1);
    let mut edits = Vec::new();
    for row in 0..text.row_count() {
        let start = text.row_start(row);
        let len = text.row_len(row) as usize;
        let content = text.slice_to_string(start..start + len);
        let lead = content.len() - content.trim_start_matches([' ', '\t']).len();
        if lead == 0 {
            continue;
        }
        let mut width = 0;
        for byte in content[..lead].bytes() {
            width += if byte == b'\t' {
                tab_size - width % tab_size
            } else {
                1
            };
        }
        let new = if to_tabs {
            "\t".repeat(width / tab_size) + &" ".repeat(width % tab_size)
        } else {
            " ".repeat(width)
        };
        if new != content[..lead] {
            edits.push((start..start + lead, new));
        }
    }
    edits
}

/// Where `offset` of `text` is once `edits` are applied. An offset inside a
/// rewritten prefix keeps its column: in a tab run it rounds down to the tab
/// before it.
fn map_offset(
    text: &Text,
    tab_size: usize,
    to_tabs: bool,
    edits: &[(Range<usize>, String)],
    offset: usize,
) -> usize {
    let inside = edits
        .iter()
        .find(|(range, _)| range.start < offset && offset < range.end);
    let Some((range, new)) = inside else {
        return map_before(edits, offset);
    };
    let mut column = 0;
    for byte in text.slice_to_string(range.start..offset).bytes() {
        column += if byte == b'\t' {
            tab_size - column % tab_size
        } else {
            1
        };
    }
    let tabs = new.bytes().take_while(|&byte| byte == b'\t').count();
    let within = if to_tabs && column < tabs * tab_size {
        column / tab_size
    } else if to_tabs {
        tabs + column - tabs * tab_size
    } else {
        column
    };
    map_before(edits, range.start) + within
}

impl EditorModel {
    /// Rewrites the leading tabs of every row as spaces at the tab size, and
    /// indents with spaces of that width from now on. Returns whether the
    /// text or style changed; selections stay over their text.
    pub fn convert_indentation_to_spaces(&mut self) -> bool {
        let edited = self.convert_indentation(false);
        let width = u8::try_from(self.tab_size()).unwrap_or(u8::MAX);
        self.restyle(Some(IndentStyle::Spaces(width))) || edited
    }

    /// Rewrites each full tab size of leading spaces on every row as a tab,
    /// and indents with tabs from now on. Returns whether the text or style changed;
    /// selections stay over their text.
    pub fn convert_indentation_to_tabs(&mut self) -> bool {
        let edited = self.convert_indentation(true);
        self.restyle(Some(IndentStyle::Tabs)) || edited
    }

    /// Indents as the text does (see [`detect_indent_style`]), replacing any
    /// style set before. With too few indented rows the style stays. Returns
    /// whether it changed.
    pub fn detect_indentation(&mut self) -> bool {
        self.sync_display();
        let Some(style) = detect_indent_style(self.text()) else {
            return false;
        };
        self.restyle(Some(style))
    }

    /// Sets the indent style; whether it differs from before.
    fn restyle(&mut self, style: Option<IndentStyle>) -> bool {
        let before = self.indent_style();
        self.set_indent_style(style);
        self.indent_style() != before
    }

    fn convert_indentation(&mut self, to_tabs: bool) -> bool {
        self.sync_display();
        let edits = indentation_edits(self.text(), self.tab_size(), to_tabs);
        if edits.is_empty() {
            return false;
        }
        let (text, tab_size) = (self.text(), self.tab_size());
        let map = |offset| map_offset(text, tab_size, to_tabs, &edits, offset);
        let selections = self
            .selections()
            .iter()
            .map(|selection| ResolvedSelection {
                range: map(selection.range.start)..map(selection.range.end),
                reversed: selection.reversed,
            })
            .collect();
        self.apply_row_edits(edits, selections, Vec::new());
        true
    }
}

#[cfg(test)]
#[path = "indent_convert_tests.rs"]
mod tests;
