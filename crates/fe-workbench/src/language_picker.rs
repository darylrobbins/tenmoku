//! Change Language Mode (`language.select`): a [`Picker`] over Auto Detect,
//! Plain Text and every language there is, which sets an editor's language.
//!
//! A language picked, Plain Text too, is remembered for the file
//! ([`fe_config::language_choices`]), in its project's list, and an editor
//! opening the file later, in any window, takes it over what detection
//! finds ([`init`]'s resolver). Auto Detect forgets it and detects the
//! language again. A remembered language no longer there (its grammar
//! uninstalled) leaves it to detection. Lists are written off the UI
//! thread, in the order picked.

use std::path::{Path, PathBuf};

use fe_config::language_choices;
use fe_gpui::EditorView;
use gpui::{
    AnyElement, App, AppContext as _, Context, Focusable as _, Global, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, Task, WeakEntity, Window, div,
};
use kiln_shell::picker::joined_label;

use kiln_l10n::tr;

use crate::{
    picker::{Picker, PickerDelegate},
    status_bar::{language_label, plain_text},
};

/// What a row picks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    /// Forget the file's language and detect it.
    AutoDetect,
    /// The language with this id, `None` for Plain Text.
    Language(Option<&'static str>),
}

/// A row: what it picks, and its label.
type Row = (Pick, SharedString);

pub(crate) struct LanguagePicker {
    /// Auto Detect, Plain Text, then the languages by label, A–Z.
    rows: Vec<Row>,
    /// The rows matching the query, as indices into `rows`.
    matches: Vec<usize>,
    /// The editor it's opened for.
    editor: Option<WeakEntity<EditorView>>,
    /// The root of the window it's opened in, whose project a pick is
    /// remembered in.
    root: Option<PathBuf>,
    /// That editor's language when it opened, `Some(None)` for Plain Text;
    /// `None` while a language loads, which marks no row.
    current: Option<Option<&'static str>>,
}

impl LanguagePicker {
    pub(crate) fn new() -> Self {
        let mut languages: Vec<Row> = fe_syntax::language_names()
            .map(|id| (Pick::Language(Some(id)), language_label(id)))
            .collect();
        languages.sort_by_key(|(_, label)| label.to_lowercase());
        let rows: Vec<Row> = [
            (
                Pick::AutoDetect,
                tr!("workbench-workbench-language-auto-detect"),
            ),
            (Pick::Language(None), plain_text()),
        ]
        .into_iter()
        .chain(languages)
        .collect();
        Self {
            matches: (0..rows.len()).collect(),
            rows,
            editor: None,
            root: None,
            current: None,
        }
    }

    /// The picker is opening for `editor`, in a window on `root`, and
    /// marks its language current.
    pub(crate) fn opened(
        &mut self,
        editor: &gpui::Entity<EditorView>,
        root: Option<&Path>,
        cx: &App,
    ) {
        self.editor = Some(editor.downgrade());
        self.root = root.map(Path::to_owned);
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
        self.current
            .is_some_and(|current| self.row(ix).0 == Pick::Language(current))
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
            .filter(|(_, (pick, label))| {
                label.to_lowercase().contains(&query)
                    || matches!(pick, Pick::Language(Some(id)) if id.to_lowercase().contains(&query))
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

    /// The language's id, the empty string for Plain Text, and for Auto
    /// Detect `*`, which no language's id is.
    fn match_id(&self, ix: usize) -> SharedString {
        match self.row(ix).0 {
            Pick::AutoDetect => "*".into(),
            Pick::Language(id) => id.unwrap_or_default().into(),
        }
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

    /// Sets the editor's language to the row's, remembering it for its
    /// file, or for Auto Detect forgets it and detects the language again;
    /// then focuses the editor. If the editor has closed, the pick only
    /// dismisses the picker.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let pick = self.row(ix).0;
        if let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade) {
            let path = editor.read(cx).model().path().map(Path::to_owned);
            match pick {
                Pick::AutoDetect => editor.update(cx, |editor, cx| editor.detect_language(cx)),
                Pick::Language(language) => {
                    editor.update(cx, |editor, cx| editor.set_language(language, cx))
                }
            }
            if let Some(path) = path {
                keep(self.root.clone(), path, pick, cx);
            }
        }
        self.focus_editor(window, cx);
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.focus_editor(window, cx);
    }
}

/// Where the lists are kept, if not under the user's state directory.
struct StateDir(PathBuf);

impl Global for StateDir {}

/// Keeps the lists under `dir` from now on.
#[cfg(test)]
pub(crate) fn set_state_dir(cx: &mut App, dir: PathBuf) {
    cx.set_global(StateDir(dir));
}

/// The state directory the lists are kept in: the one set, or the user's.
/// Under test, none unless one is set, so no test touches the user's.
fn state_dir(cx: &App) -> Option<PathBuf> {
    match cx.try_global::<StateDir>() {
        Some(StateDir(dir)) => Some(dir.clone()),
        None if cfg!(test) => None,
        None => fe_config::paths::state_dir(),
    }
}

/// The latest write to the lists, which waits for the one before.
struct Writes(Task<()>);

impl Global for Writes {}

/// Remembers `pick` for the file at `path`, picked in a window on `root`,
/// or for Auto Detect forgets the file's language, off the UI thread and
/// after the writes asked before it.
fn keep(root: Option<PathBuf>, path: PathBuf, pick: Pick, cx: &mut App) {
    let Some(dir) = state_dir(cx) else {
        return;
    };
    let before = cx
        .try_global::<Writes>()
        .is_some()
        .then(|| cx.remove_global::<Writes>().0);
    let write = cx.background_spawn(async move {
        if let Some(before) = before {
            before.await;
        }
        let root = root.as_deref();
        let written = match pick {
            Pick::AutoDetect => language_choices::forget(&dir, root, &path),
            Pick::Language(language) => language_choices::remember(&dir, root, &path, language),
        };
        if let Err(error) = written {
            log::warn!("can't keep the language of {}: {error}", path.display());
        }
    });
    cx.set_global(Writes(write));
}

/// Has editors opening a file take the language remembered for it.
pub(crate) fn init(cx: &mut App) {
    EditorView::set_language_resolver(cx, |path, cx| {
        language_choices::remembered(&state_dir(cx)?, path)
    });
}
