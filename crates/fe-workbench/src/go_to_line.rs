//! Go to Line/Column (`go_to_line.toggle`): a [`Picker`] whose query is a
//! line, or `line:column`, and whose one row moves the active editor's
//! caret there.

use fe_gpui::EditorView;
use gpui::{
    AnyElement, App, Context, Entity, Focusable as _, IntoElement as _, ParentElement as _,
    SharedString, WeakEntity, Window, div,
};

use crate::{
    Workbench,
    picker::{Picker, PickerDelegate},
};

/// A parsed query: 1-based line and, if given, 1-based column in chars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Target {
    line: usize,
    column: Option<usize>,
}

/// Digits, or `None` for anything else; a number too big to hold is the
/// biggest, which clamps like any other past the end.
fn number(text: &str) -> Option<usize> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(text.parse().unwrap_or(usize::MAX))
}

/// `N` or `N:C`, with surrounding spaces and one leading `:` ignored. Line
/// and column are 1 or more.
fn parse(query: &str) -> Option<Target> {
    let query = query.trim();
    let query = query.strip_prefix(':').unwrap_or(query).trim();
    let (line, column) = match query.split_once(':') {
        Some((line, column)) => (line.trim(), Some(column.trim())),
        None => (query, None),
    };
    let line = number(line).filter(|line| *line >= 1)?;
    let column = match column {
        Some(column) => Some(number(column).filter(|column| *column >= 1)?),
        None => None,
    };
    Some(Target { line, column })
}

pub(crate) struct GoToLine {
    workbench: WeakEntity<Workbench>,
    /// The editor it's opened for.
    editor: Option<WeakEntity<EditorView>>,
    /// That editor's caret's line (1-based) and its line count, as it opened.
    caret_line: usize,
    lines: usize,
    target: Option<Target>,
    query_empty: bool,
}

impl GoToLine {
    pub(crate) fn new() -> Self {
        Self {
            workbench: WeakEntity::new_invalid(),
            editor: None,
            caret_line: 1,
            lines: 1,
            target: None,
            query_empty: true,
        }
    }

    /// The picker is opening for `editor`, in `workbench`.
    pub(crate) fn opened(
        &mut self,
        editor: &Entity<EditorView>,
        workbench: WeakEntity<Workbench>,
        cx: &App,
    ) {
        let model = editor.read(cx).model();
        let snapshot = model.snapshot();
        let text = snapshot.text();
        self.caret_line = text.offset_to_point(model.primary_selection().head()).row as usize + 1;
        self.lines = text.row_count() as usize;
        self.editor = Some(editor.downgrade());
        self.workbench = workbench;
        self.target = None;
        self.query_empty = true;
    }

    /// Focuses the editor it's opened for, if it's still open.
    fn focus_editor(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.editor.take().and_then(|editor| editor.upgrade()) {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }

    fn label(&self) -> String {
        match self.target {
            Some(Target {
                line,
                column: Some(column),
            }) => format!("Go to line {line}, column {column}"),
            Some(Target { line, column: None }) => format!("Go to line {line}"),
            None => String::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn status_text(&self) -> String {
        self.status().unwrap_or_default().to_string()
    }
}

impl PickerDelegate for GoToLine {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query_empty = query.trim().is_empty();
        self.target = parse(&query);
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        usize::from(self.target.is_some())
    }

    fn match_id(&self, _: usize) -> SharedString {
        self.label().into()
    }

    fn render_match(&self, _: usize, _: bool, _: &App) -> AnyElement {
        div().child(self.label()).into_any_element()
    }

    /// Moves the editor to a single caret at the target, clamped, after
    /// recording where it was for back, and focuses it. If the editor has
    /// closed, the pick only dismisses the picker.
    fn confirm(&mut self, _: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let (Some(target), Some(editor)) = (
            self.target,
            self.editor.as_ref().and_then(WeakEntity::upgrade),
        ) else {
            self.focus_editor(window, cx);
            return true;
        };
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                // As a jump does: a running go-to-definition mustn't land
                // over this one.
                workbench.nav_state.goto.next();
                workbench.push_history(cx)
            });
        }
        let snapshot = editor.read(cx).model().snapshot();
        let text = snapshot.text();
        let row = (target.line - 1).min(text.row_count() as usize - 1) as u32;
        let start = text.row_start(row);
        let row_text = text.slice_to_string(start..start + text.row_len(row) as usize);
        let column = target.column.map_or(0, |column| column - 1);
        let offset = start
            + row_text
                .char_indices()
                .nth(column)
                .map_or(row_text.len(), |(at, _)| at);
        editor.update(cx, |editor, cx| {
            editor.select_and_reveal(offset..offset, cx)
        });
        self.focus_editor(window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.focus_editor(window, cx);
    }

    fn status(&self) -> Option<SharedString> {
        Some(if self.target.is_some() {
            SharedString::default()
        } else if self.query_empty {
            format!("Line {} of {}", self.caret_line, self.lines).into()
        } else {
            format!("Type a line number between 1 and {}", self.lines).into()
        })
    }
}

#[cfg(test)]
#[path = "go_to_line_tests.rs"]
mod tests;
