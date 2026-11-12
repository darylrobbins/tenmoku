//! Insert Snippet (`workbench.insert_snippet`) and Configure User Snippets
//! (`workbench.configure_snippets`), each a [`Picker`] in the modal layer.
//!
//! - **Insert Snippet** lists the snippets for the active editor's
//!   language (or for every language): the user's
//!   ([`crate::user_snippets`], read again if their files changed), by
//!   name, then extensions', by prefix; each with its prefixes, and its
//!   description or whose it is. The query matches a row's name or
//!   prefixes, its chars in order, or is in its description, case aside. Enter inserts the
//!   snippet in place of the primary selection, through the snippet engine
//!   ([`EditorModel::insert_snippet`](fe_editor::EditorModel::insert_snippet)):
//!   its first tabstop selected, Tab walking the rest. The engine expands
//!   one snippet at a time, so other carets are left as they were. With no
//!   editor active it does nothing, and in a read-only one it says so.
//! - **Configure User Snippets** lists "New Global Snippets file", then
//!   every language, those with a file first. Enter creates the file if
//!   it's missing (`snippets/<language>.json`, or
//!   `snippets/global.code-snippets`) with a commented example
//!   ([`example_file`]), never overwriting one, then opens it in a tab.
//!   With no config directory it says so.

use std::{fs, io, path::PathBuf, rc::Rc};

use fe_config::user_snippets::{GLOBAL_SNIPPETS_FILE, example_file, language_file};
use fe_editor::snippet::Snippet;
use fe_gpui::EditorView;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div,
};
use kiln_l10n::{text, tr};
use kiln_shell::ModalId;

use crate::{
    Workbench,
    extension_snippets::{SnippetText, USER_SNIPPET, extension_offers},
    picker::{Picker, PickerDelegate},
    status_bar::language_label,
    toolbar_picker::subsequence,
    user_snippets::{user_snippet_set, user_snippets_dir},
};

/// Insert Snippet's id in the modal layer.
pub(crate) const INSERT_SNIPPET: ModalId = ModalId("insert_snippet");

/// Configure User Snippets' id in the modal layer.
pub(crate) const CONFIGURE_SNIPPETS: ModalId = ModalId("configure_snippets");

/// One of Insert Snippet's rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SnippetRow {
    /// The user snippet's name, or the extension snippet's prefix.
    pub(crate) name: String,
    /// What's typed to offer it, by comma.
    pub(crate) prefixes: String,
    /// Its description, or else whose it is.
    pub(crate) detail: String,
    /// Its text, shared with its completions' offers.
    pub(crate) text: Rc<SnippetText>,
    pub(crate) user: bool,
}

/// The snippets for `language` (`None`: plain text): the user's by name,
/// then extensions' by prefix. Their text is shared, not copied.
fn snippet_rows(language: Option<&str>, cx: &mut App) -> Vec<SnippetRow> {
    // One read: each snippet with its own text.
    let set = user_snippet_set(cx);
    let mut user: Vec<SnippetRow> = (set.snippets.iter().zip(set.texts.iter()))
        .filter(|(snippet, _)| snippet.is_for(language))
        .map(|(snippet, text)| SnippetRow {
            name: snippet.name.clone(),
            prefixes: snippet.prefixes.join(", "),
            detail: (snippet.description.clone()).unwrap_or_else(|| USER_SNIPPET.to_owned()),
            text: text.clone(),
            user: true,
        })
        .collect();
    user.sort_by_key(|row| row.name.to_lowercase());
    let mut extensions: Vec<SnippetRow> = (extension_offers(cx).iter())
        .filter(|offer| offer.is_for(language))
        .map(|offer| SnippetRow {
            name: offer.prefix.clone(),
            prefixes: offer.prefix.clone(),
            detail: match &offer.text.description {
                Some(description) => tr!(
                    "workbench-lsp-snippet-detail",
                    description = description,
                    source = offer.detail.to_string()
                )
                .into(),
                None => offer.detail.to_string(),
            },
            text: offer.text.clone(),
            user: false,
        })
        .collect();
    extensions.sort_by_key(|row| row.name.to_lowercase());
    user.extend(extensions);
    user
}

/// Insert Snippet's delegate.
pub(crate) struct InsertSnippetPicker {
    rows: Vec<SnippetRow>,
    matches: Vec<usize>,
    /// The editor it inserts into.
    editor: WeakEntity<EditorView>,
}

impl InsertSnippetPicker {
    fn new(editor: &Entity<EditorView>, cx: &mut App) -> Self {
        let language = (editor.read(cx).syntax()).map(|syntax| syntax.language().name());
        let rows = snippet_rows(language, cx);
        Self {
            matches: (0..rows.len()).collect(),
            rows,
            editor: editor.downgrade(),
        }
    }

    fn focus_editor(&self, window: &mut Window, cx: &mut App) {
        if let Some(editor) = self.editor.upgrade() {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }
}

#[cfg(test)]
impl InsertSnippetPicker {
    /// The matching rows, in order.
    pub(crate) fn rows(&self) -> Vec<SnippetRow> {
        (self.matches.iter())
            .map(|&ix| self.rows[ix].clone())
            .collect()
    }
}

/// Whether `query`, lowercase, matches any of `texts`: its chars in order,
/// case aside.
fn matches_any<'a>(query: &[char], texts: impl IntoIterator<Item = &'a str>) -> bool {
    (texts.into_iter()).any(|text| subsequence(query, text).is_some())
}

/// Lowercase `query`'s chars.
fn query_chars(query: &str) -> Vec<char> {
    query.trim().chars().flat_map(char::to_lowercase).collect()
}

/// Has `picker` take its new matches, once the update setting them ends.
fn matches_updated<D: PickerDelegate>(cx: &mut Context<Picker<D>>) {
    cx.notify();
    let picker = cx.entity();
    // `Picker::matches_updated` borrows the picker, which is being updated
    // now.
    cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
}

impl PickerDelegate for InsertSnippetPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let chars = query_chars(&query);
        let query = query.trim().to_lowercase();
        self.matches = (self.rows.iter().enumerate())
            .filter(|(_, row)| {
                matches_any(&chars, [row.name.as_str(), row.prefixes.as_str()])
                    || row.detail.to_lowercase().contains(&query)
            })
            .map(|(ix, _)| ix)
            .collect();
        matches_updated(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.matches[ix].to_string().into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let row = &self.rows[self.matches[ix]];
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .overflow_hidden()
            .child(div().flex_none().child(row.name.clone()))
            .child(
                div()
                    .flex_none()
                    .text_color(ui.muted_text)
                    .child(row.prefixes.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(ui.muted_text)
                    .child(row.detail.clone()),
            )
            .into_any_element()
    }

    /// The snippet's name, then its prefixes and detail, as the row shows them.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        let row = &self.rows[self.matches[ix]];
        let parts = [&row.name, &row.prefixes, &row.detail];
        let parts: Vec<&str> = (parts.into_iter())
            .map(|part| part.as_str())
            .filter(|part| !part.is_empty())
            .collect();
        parts.join(", ").into()
    }

    /// Inserts the row's snippet in place of the editor's primary
    /// selection, then focuses it.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(row) = self.matches.get(ix).map(|&ix| &self.rows[ix]) else {
            return false;
        };
        if let Some(editor) =
            (self.editor.upgrade()).filter(|editor| !editor.read(cx).is_read_only())
        {
            let snippet = Snippet::parse(&row.text.body);
            editor.update(cx, |view, cx| {
                let range = view.model().primary_selection().range;
                view.model_mut().insert_snippet(range, &snippet);
                cx.notify();
            });
        }
        self.focus_editor(window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.focus_editor(window, cx);
    }

    fn status(&self) -> Option<SharedString> {
        (self.rows.is_empty()).then(|| tr!("workbench-lsp-snippet-none"))
    }
}

/// One of Configure User Snippets' rows: a language, by fastedit's name,
/// or `None` for the global file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConfigureRow {
    pub(crate) language: Option<&'static str>,
    pub(crate) label: SharedString,
    /// Whether its file exists, as the picker opened.
    pub(crate) exists: bool,
}

/// Configure User Snippets' delegate.
pub(crate) struct ConfigureSnippetsPicker {
    dir: PathBuf,
    rows: Vec<ConfigureRow>,
    matches: Vec<usize>,
    workbench: WeakEntity<Workbench>,
    /// The editor active as it opened, focused again if it's dismissed.
    editor: Option<WeakEntity<EditorView>>,
}

impl ConfigureSnippetsPicker {
    fn new(
        dir: PathBuf,
        workbench: WeakEntity<Workbench>,
        editor: Option<&Entity<EditorView>>,
    ) -> Self {
        let global = ConfigureRow {
            language: None,
            label: tr!("workbench-lsp-snippet-new-global"),
            exists: dir.join(GLOBAL_SNIPPETS_FILE).is_file(),
        };
        let mut languages: Vec<ConfigureRow> = fe_syntax::language_names()
            .map(|language| ConfigureRow {
                language: Some(language),
                label: language_label(language),
                exists: language_file(&dir, language).is_file(),
            })
            .collect();
        languages.sort_by_key(|row| (!row.exists, row.label.to_lowercase()));
        let rows: Vec<ConfigureRow> = [global].into_iter().chain(languages).collect();
        Self {
            dir,
            matches: (0..rows.len()).collect(),
            rows,
            workbench,
            editor: editor.map(Entity::downgrade),
        }
    }

    /// The file `row` configures.
    fn file(&self, row: &ConfigureRow) -> PathBuf {
        match row.language {
            Some(language) => language_file(&self.dir, language),
            None => self.dir.join(GLOBAL_SNIPPETS_FILE),
        }
    }

    /// What `row` notes after its label: that its file exists, if it does.
    fn exists_note(&self, row: &ConfigureRow) -> Option<String> {
        let file = self.file(row);
        let name = (file.file_name()).map(|name| name.to_string_lossy().into_owned());
        row.exists.then(|| {
            tr!(
                "workbench-lsp-snippet-exists",
                file = name.unwrap_or_default()
            )
            .into()
        })
    }
}

#[cfg(test)]
impl ConfigureSnippetsPicker {
    /// The matching rows, in order.
    pub(crate) fn rows(&self) -> Vec<ConfigureRow> {
        (self.matches.iter())
            .map(|&ix| self.rows[ix].clone())
            .collect()
    }
}

/// Writes `file`, `language`'s snippet file (`None`: the global one), with
/// a commented example, and its directory, unless it exists, which is never
/// overwritten.
fn create_snippet_file(file: &std::path::Path, language: Option<&str>) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let created = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(file);
    match created {
        Ok(mut created) => io::Write::write_all(&mut created, example_file(language).as_bytes()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

impl PickerDelegate for ConfigureSnippetsPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        let query = query_chars(&query);
        self.matches = (self.rows.iter().enumerate())
            .filter(|(_, row)| {
                matches_any(
                    &query,
                    [row.label.as_ref(), row.language.unwrap_or("global")],
                )
            })
            .map(|(ix, _)| ix)
            .collect();
        matches_updated(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        let row = &self.rows[self.matches[ix]];
        row.language.unwrap_or("").into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let row = &self.rows[self.matches[ix]];
        let exists =
            (self.exists_note(row)).map(|note| div().text_color(ui.muted_text).child(note));
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .child(row.label.clone())
            .children(exists)
            .into_any_element()
    }

    /// The row's label, then whether its file exists, as the row shows them.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        let row = &self.rows[self.matches[ix]];
        match self.exists_note(row) {
            Some(note) => tr!(
                "workbench-lsp-snippet-label-note",
                label = &row.label,
                note = note
            ),
            None => row.label.clone(),
        }
    }

    /// Creates the row's file if it's missing, then opens it in a tab.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(row) = self.matches.get(ix).map(|&ix| self.rows[ix].clone()) else {
            return false;
        };
        let file = self.file(&row);
        if let Err(error) = create_snippet_file(&file, row.language) {
            let message = tr!(
                "workbench-lsp-snippet-cant-create",
                path = file.display().to_string(),
                error = error.to_string()
            )
            .into();
            Workbench::notify(cx, log::Level::Error, message, None);
            return true;
        }
        if let Some(workbench) = self.workbench.upgrade() {
            let pane = workbench.read(cx).pane().clone();
            pane.update(cx, |pane, cx| pane.open(file, window, cx));
        }
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade) {
            window.focus(&editor.focus_handle(cx), cx);
        }
    }
}

impl Workbench {
    /// Insert Snippet: see the module docs.
    pub(crate) fn insert_snippet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        if editor.read(cx).is_read_only() {
            let message = tr!("workbench-lsp-snippet-read-only").into();
            Workbench::notify(cx, log::Level::Info, message, None);
            return;
        }
        self.dismiss_palette(window, cx);
        let delegate = InsertSnippetPicker::new(&editor, cx);
        let placeholder = text!("workbench-lsp-snippet-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        self.show_snippet_modal(INSERT_SNIPPET, picker, window, cx);
    }

    /// Configure User Snippets: see the module docs.
    pub(crate) fn configure_snippets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = user_snippets_dir(cx) else {
            let message = tr!("workbench-lsp-snippet-no-config-dir").into();
            Workbench::notify(cx, log::Level::Warn, message, None);
            return;
        };
        self.dismiss_palette(window, cx);
        let editor = self.active_editor(cx).cloned();
        let delegate = ConfigureSnippetsPicker::new(dir, cx.weak_entity(), editor.as_ref());
        let placeholder = text!("workbench-lsp-snippet-file-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        self.show_snippet_modal(CONFIGURE_SNIPPETS, picker, window, cx);
    }

    fn show_snippet_modal<D: PickerDelegate>(
        &mut self,
        id: ModalId,
        picker: Entity<Picker<D>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        picker.update(cx, |picker, cx| picker.show(window, cx));
        let modals = self.shell().read(cx).modals().clone();
        modals.update(cx, |layer, cx| layer.show_modal(id, picker, window, cx));
    }

    /// The snippet picker `id` names, if it shows.
    #[cfg(test)]
    pub(crate) fn snippet_picker<D: PickerDelegate>(
        &self,
        id: ModalId,
        cx: &App,
    ) -> Option<Entity<Picker<D>>> {
        let layer = self.shell().read(cx).modals().read(cx);
        if layer.modal() != Some(id) {
            return None;
        }
        layer.modal_view()?.clone().downcast().ok()
    }
}
