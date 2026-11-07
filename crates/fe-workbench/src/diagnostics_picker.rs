//! Search Diagnostics in File and in Workspace
//! (`diagnostics.search_document` and `diagnostics.search_workspace`,
//! LazyVim's `<leader>sd` and `<leader>sD`): a [`Picker`] over the problems
//! the Problems panel lists ([`ProblemsPanel::listed`]), from the language
//! servers, the finished tasks and the plugins alike, the document variant
//! the active editor's file's alone.
//!
//! Each row has its severity's icon in the editor's colour for it, its
//! message's first line, and `file:row`; errors come first, then warnings,
//! information and hints, each in the panel's order (by file, then
//! position). Rows match the query by subsequence over the message and the
//! file's path as the panel shows it, ignoring case.
//!
//! Moving the selection onto a problem in the active editor's file previews
//! it there as the symbol picker previews a symbol ([`RowPreview`]); one in
//! another file previews nothing, as previewing it would open that file.
//! Dismissing puts the scroll and the folds back. Enter opens the problem as
//! the panel does ([`Workbench::open_listed_problem`]): its file's editor
//! focused, the problem's range selected (its start, with vim on).

use std::path::{Path, PathBuf};

use fe_editor::diagnostics::Severity;
use fe_gpui::EditorView;
use fe_ui::{HighlightedLabel, Icon, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Hsla, InteractiveElement as _,
    IntoElement as _, ParentElement as _, SharedString, Styled as _, Subscription, WeakEntity,
    Window, div, px, svg,
};

use super::buffer_lines_picker::RowPreview;
use crate::{
    Workbench,
    dock::problems_panel::ListedProblem,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
    workbench::Palette,
};

/// Which problems the picker lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The active editor's file's.
    Document,
    /// Every file's.
    Workspace,
}

/// A problem listed.
struct Row {
    listed: ListedProblem,
    /// Its file as the row shows it: from the root, if it's under it.
    file: String,
    /// The message's first line.
    message: String,
    /// What the query matches: the message, then the file.
    matched: String,
    color: Hsla,
}

pub(crate) struct DiagnosticsPicker {
    workbench: WeakEntity<Workbench>,
    scope: Scope,
    preview: RowPreview,
    /// The active editor's file as it opened, the one previews show in.
    previewed_path: Option<PathBuf>,
    rows: Vec<Row>,
    query: String,
    /// The rows matching the query, each with the chars of `matched` it
    /// matched.
    matches: Vec<(usize, Vec<usize>)>,
    /// Focused before it opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    _follows: Option<Subscription>,
}

/// `path` as a row shows it: from `root` if it's under it.
fn display_path(path: &Path, root: Option<&Path>) -> String {
    let relative = root.and_then(|root| path.strip_prefix(root).ok());
    relative.unwrap_or(path).display().to_string()
}

/// A severity's icon.
fn severity_icon(severity: Severity) -> Icon {
    match severity {
        Severity::Error => Icon::CircleX,
        Severity::Warning => Icon::TriangleAlert,
        Severity::Information => Icon::CircleDot,
        Severity::Hint => Icon::MessageSquare,
    }
}

/// The problems the panel lists, of `path` alone if given, errors first.
fn rows(workbench: &Workbench, path: Option<&Path>, cx: &App) -> Vec<Row> {
    let panel = workbench.dock().read(cx).problems().read(cx);
    let root = workbench.root();
    let mut listed: Vec<ListedProblem> = (panel.listed().into_iter())
        .filter(|listed| path.is_none_or(|path| listed.path == path))
        .collect();
    // Stable: the panel's order within a severity.
    listed.sort_by_key(|listed| std::cmp::Reverse(listed.problem.severity));
    (listed.into_iter())
        .map(|listed| {
            let file = display_path(&listed.path, root);
            let message = (listed.problem.message.lines().next())
                .unwrap_or_default()
                .to_owned();
            let matched = format!("{message} {file}");
            let color = panel.severity_color(listed.problem.severity);
            Row {
                listed,
                file,
                message,
                matched,
                color,
            }
        })
        .collect()
}

impl DiagnosticsPicker {
    fn new(
        workbench: WeakEntity<Workbench>,
        scope: Scope,
        editor: Option<&Entity<EditorView>>,
        rows: Vec<Row>,
        previous_focus: Option<FocusHandle>,
        cx: &App,
    ) -> Self {
        let previewed_path =
            editor.and_then(|editor| editor.read(cx).model().path().map(Path::to_owned));
        let mut preview = RowPreview::new(editor.map(Entity::downgrade));
        let first = rows.first().map(|row| row.listed.problem.row);
        let first_here = rows
            .first()
            .is_some_and(|row| Some(&row.listed.path) == previewed_path.as_ref());
        preview.opened_at(first.filter(|_| first_here));
        Self {
            workbench,
            scope,
            preview,
            previewed_path,
            matches: (0..rows.len()).map(|ix| (ix, Vec::new())).collect(),
            rows,
            query: String::new(),
            previous_focus,
            _follows: None,
        }
    }

    /// Previews the selected problem from now on as the selection moves.
    fn follow(&mut self, cx: &mut Context<Picker<Self>>) {
        // `Picker::select` is the only way the selection moves, and it
        // notifies; the delegate has no hook for it.
        self._follows = Some(cx.observe_self(|picker, cx| {
            let selected = picker.selected();
            picker.delegate.preview(selected, cx);
        }));
    }

    /// Previews match `ix`, if it's in the previewed editor's file.
    fn preview(&mut self, ix: usize, cx: &mut App) {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return;
        };
        let listed = &self.rows[row].listed;
        if Some(&listed.path) != self.previewed_path.as_ref() {
            return;
        }
        self.preview.show(listed.problem.row, cx);
    }

    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| Some((ix, subsequence(&query, &row.matched)?)))
            .collect();
        let picker = cx.entity();
        // The picker is being updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }
}

#[cfg(test)]
impl DiagnosticsPicker {
    /// The matches: each severity, message and `file:row`.
    pub(crate) fn labels(&self) -> Vec<(Severity, String, String)> {
        (self.matches.iter())
            .map(|(row, _)| {
                let row = &self.rows[*row];
                let at = format!("{}:{}", row.file, row.listed.problem.row + 1);
                (row.listed.problem.severity, row.message.clone(), at)
            })
            .collect()
    }
}

impl PickerDelegate for DiagnosticsPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.matches[ix].0.to_string().into()
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (row, matched) = &self.matches[ix];
        let row = &self.rows[*row];
        // The chars matched in the message: those before the file.
        let length = row.message.chars().count();
        let in_message: Vec<usize> = (matched.iter().copied())
            .filter(|&ix| ix < length)
            .collect();
        let label = HighlightedLabel::new(
            row.message.clone(),
            char_ranges(&row.message, &in_message),
            &ui,
        );
        let severity = row.listed.problem.severity;
        let path = severity_icon(severity).path();
        let message = row.message.clone();
        let at = format!("{}:{}", row.file, row.listed.problem.row + 1);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(
                svg()
                    .debug_selector(|| format!("diagnostics-picker-icon:{message}:{path}"))
                    .path(path)
                    .size(px(14.))
                    .flex_none()
                    .text_color(row.color),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label),
            )
            .child(
                div()
                    .ml_auto()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_color(ui.muted_text)
                    .child(at),
            )
            .into_any_element()
    }

    /// Opens the problem as the Problems panel does.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return false;
        };
        let Some(workbench) = self.workbench.upgrade() else {
            return true;
        };
        if let Some(editor) = self.preview.editor() {
            editor.update(cx, |editor, _| self.preview.close_folds(editor));
        }
        self.preview.keep();
        self.previous_focus = None;
        let listed = self.rows[row].listed.clone();
        workbench.update(cx, |workbench, cx| {
            workbench.open_listed_problem(&listed, window, cx)
        });
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.preview.restore(cx);
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        if self.scope == Scope::Document && self.previewed_path.is_none() {
            Some("No file".into())
        } else if self.rows.is_empty() {
            Some(match self.scope {
                Scope::Document => "No problems in this file".into(),
                Scope::Workspace => "No problems".into(),
            })
        } else if self.matches.is_empty() {
            Some("No matching problems".into())
        } else {
            None
        }
    }
}

impl Workbench {
    /// The diagnostics picker, while it shows.
    #[cfg(test)]
    pub(crate) fn diagnostics_picker(&self, cx: &App) -> Option<Entity<Picker<DiagnosticsPicker>>> {
        self.shown_palette(Palette::Diagnostics, cx)
    }

    /// Shows the diagnostics picker, focused, over the problems the
    /// Problems panel lists, `scope`'s, in place of any other palette, or
    /// afresh if it shows.
    pub(crate) fn show_diagnostics(
        &mut self,
        scope: Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_palette(window, cx);
        // Listed as of now.
        self.sync_problems(cx);
        let previous = window.focused(cx);
        let editor = self.active_editor(cx).cloned();
        let path = editor
            .as_ref()
            .and_then(|editor| editor.read(cx).model().path().map(Path::to_owned));
        let rows = match scope {
            // No file: nothing of it to list.
            Scope::Document if path.is_none() => Vec::new(),
            Scope::Document => rows(self, path.as_deref(), cx),
            Scope::Workspace => rows(self, None, cx),
        };
        let delegate = DiagnosticsPicker::new(
            cx.entity().downgrade(),
            scope,
            editor.as_ref(),
            rows,
            previous,
            cx,
        );
        let placeholder = match scope {
            Scope::Document => "Search diagnostics in file…",
            Scope::Workspace => "Search diagnostics…",
        };
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.delegate.follow(cx);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Diagnostics, picker, window, cx);
    }
}

#[cfg(test)]
#[path = "diagnostics_picker_tests.rs"]
mod diagnostics_picker_tests;
