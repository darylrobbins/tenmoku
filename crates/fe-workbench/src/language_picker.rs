//! Change Language Mode (`language.select`): a [`Picker`] over Plain Text
//! and every language compiled in, which sets an editor's language.

use fe_gpui::EditorView;
use gpui::{
    AnyElement, App, Context, Focusable as _, IntoElement as _, ParentElement as _, SharedString,
    Styled as _, WeakEntity, Window, div,
};
use kiln_shell::picker::joined_label;

use kiln_l10n::tr;

use crate::{
    picker::{Picker, PickerDelegate},
    status_bar::{language_label, plain_text},
};

/// A row: the language's id, `None` for Plain Text, and its label.
type Row = (Option<&'static str>, SharedString);

pub(crate) struct LanguagePicker {
    /// Plain Text, then the languages by label, A–Z.
    rows: Vec<Row>,
    /// The rows matching the query, as indices into `rows`.
    matches: Vec<usize>,
    /// The editor it's opened for.
    editor: Option<WeakEntity<EditorView>>,
    /// That editor's language when it opened, `Some(None)` for Plain Text;
    /// `None` while a language loads, which marks no row.
    current: Option<Option<&'static str>>,
}

impl LanguagePicker {
    pub(crate) fn new() -> Self {
        let mut languages: Vec<Row> = fe_syntax::language_names()
            .map(|id| (Some(id), language_label(id)))
            .collect();
        languages.sort_by_key(|(_, label)| label.to_lowercase());
        let rows: Vec<Row> = [(None, plain_text())]
            .into_iter()
            .chain(languages)
            .collect();
        Self {
            matches: (0..rows.len()).collect(),
            rows,
            editor: None,
            current: None,
        }
    }

    /// The picker is opening for `editor`, whose language it marks current.
    pub(crate) fn opened(&mut self, editor: &gpui::Entity<EditorView>, cx: &App) {
        self.editor = Some(editor.downgrade());
        let editor = editor.read(cx);
        self.current = match editor.syntax() {
            Some(syntax) => Some(Some(syntax.language().name())),
            None if editor.is_loading_language() => None,
            None => Some(None),
        };
    }

    fn row(&self, ix: usize) -> &Row {
        &self.rows[self.matches[ix]]
    }

    /// Whether match `ix` is the editor's language, as it was on opening.
    fn is_current(&self, ix: usize) -> bool {
        self.current == Some(self.row(ix).0)
    }

    /// Focuses the editor it's opened for, if it's still open.
    fn focus_editor(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.editor.take().and_then(|editor| editor.upgrade()) {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }
}

#[cfg(test)]
impl LanguagePicker {
    /// The matches' labels, each with whether it's marked current.
    pub(crate) fn labels(&self) -> Vec<(SharedString, bool)> {
        (0..self.matches.len())
            .map(|ix| (self.row(ix).1.clone(), self.is_current(ix)))
            .collect()
    }
}

impl PickerDelegate for LanguagePicker {
    /// Rows whose label or id holds `query`, ignoring case.
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query.to_lowercase();
        self.matches = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, (id, label))| {
                label.to_lowercase().contains(&query)
                    || id.is_some_and(|id| id.to_lowercase().contains(&query))
            })
            .map(|(ix, _)| ix)
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The language's id, or the empty string for Plain Text.
    fn match_id(&self, ix: usize) -> SharedString {
        self.row(ix).0.unwrap_or_default().into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let label = self.row(ix).1.clone();
        let current = self.is_current(ix).then(|| {
            div()
                .ml_2()
                .text_color(fe_ui::ui_theme(cx).muted_text)
                .child(tr!("workbench-setup-current"))
        });
        div()
            .flex()
            .items_center()
            .child(label)
            .children(current)
            .into_any_element()
    }

    /// The language's label, then current if it's the editor's.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        let label = self.row(ix).1.clone();
        let current = self.is_current(ix).then(|| tr!("workbench-setup-current"));
        joined_label([Some(label), current].into_iter().flatten())
    }

    /// Sets the editor's language to the row's, then focuses it. If the
    /// editor has closed, the pick only dismisses the picker.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let language = self.row(ix).0;
        if let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade) {
            editor.update(cx, |editor, cx| editor.set_language(language, cx));
        }
        self.focus_editor(window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.focus_editor(window, cx);
    }
}
