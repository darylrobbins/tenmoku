//! Go to Tracked File (`git.show_files`, LazyVim's `<leader>fg`): a
//! [`Picker`] over the files git tracks in the repository that holds the
//! window's root, and the untracked ones it doesn't ignore
//! ([`Repo::listed_files`]), under the root, each by its path from there.
//! Outside a repository it says "Not a git repository".
//!
//! The files are listed off the UI thread each time the picker opens. The
//! last list for each root is kept with the HEAD commit and the index's
//! modified time it was made at: while they're the same, the kept list
//! shows at once, until the new one lands in its place (untracked files
//! come and go without either changing).
//!
//! Rows match the query by subsequence, ignoring case, in path order; Enter
//! opens the file in the active pane.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use fe_git::Repo;
use fe_ui::{HighlightedLabel, char_ranges, file_icon_ref};
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, Global, InteractiveElement as _,
    IntoElement as _, ParentElement as _, SharedString, Styled as _, Task, WeakEntity, Window, div,
    px,
};
use kiln_l10n::{text, tr};

use crate::{
    Workbench,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
    workbench::Palette,
};

/// What says a kept list still holds: HEAD's commit and the index's
/// modified time.
type Key = (Option<String>, Option<SystemTime>);

/// The last list for each root.
#[derive(Default)]
struct Kept(HashMap<PathBuf, (Key, Arc<Vec<Row>>)>);

impl Global for Kept {}

/// A file, by its path from the root.
#[derive(Clone, PartialEq, Eq)]
struct Row {
    path: PathBuf,
    label: String,
}

/// Where the list is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Listing {
    /// Being made, nothing kept to show meanwhile.
    Pending,
    /// No folder is open.
    NoRoot,
    NotRepo,
    Failed(String),
    /// The rows are shown: `fresh` once it's the list made for this opening.
    Listed {
        fresh: bool,
    },
}

pub(crate) struct GitFilesPicker {
    workbench: WeakEntity<Workbench>,
    root: Option<PathBuf>,
    listing: Listing,
    rows: Arc<Vec<Row>>,
    query: String,
    /// The rows matching the query, each with the chars of its label it
    /// matched.
    matches: Vec<(usize, Vec<usize>)>,
    /// Focused before it opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    _list: Option<Task<()>>,
}

/// What the background finds of the repository holding a root.
enum Found {
    NotRepo,
    Failed(String),
    Repo(Box<Repo>, Key),
}

/// The repository holding `root`, and its key.
fn find(root: &Path) -> Found {
    match Repo::discover(root) {
        Ok(Some(repo)) => {
            let key = (repo.head_id(), repo.index_mtime());
            Found::Repo(Box::new(repo), key)
        }
        Ok(None) => Found::NotRepo,
        Err(error) => Found::Failed(error.to_string()),
    }
}

/// `files`, git's absolute paths, as rows: those under `root`, by their
/// paths from there with `/`s.
fn rows_under(root: &Path, files: Vec<PathBuf>) -> Vec<Row> {
    // The repository's paths are absolute, as `Repo::discover` made its
    // working tree; the root may be spelled otherwise (a symlink to it).
    let root = std::path::absolute(root).unwrap_or_else(|_| root.to_owned());
    let canonical = root.canonicalize().ok();
    (files.into_iter())
        .filter_map(|path| {
            let rest = (path.strip_prefix(&root).ok())
                .or_else(|| path.strip_prefix(canonical.as_ref()?).ok())?;
            let label = (rest.components())
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            Some(Row { label, path })
        })
        .collect()
}

impl GitFilesPicker {
    fn new(
        workbench: WeakEntity<Workbench>,
        root: Option<PathBuf>,
        previous_focus: Option<FocusHandle>,
    ) -> Self {
        Self {
            workbench,
            listing: match root {
                Some(_) => Listing::Pending,
                None => Listing::NoRoot,
            },
            root,
            rows: Arc::default(),
            query: String::new(),
            matches: Vec::new(),
            previous_focus,
            _list: None,
        }
    }

    /// Lists the root's files in the background, showing the kept list
    /// meanwhile if it still holds.
    fn list(&mut self, cx: &mut Context<Picker<Self>>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let kept = (cx.try_global::<Kept>()).and_then(|kept| kept.0.get(&root).cloned());
        self._list = Some(cx.spawn(async move |picker, cx| {
            let at = root.clone();
            let found = cx.background_spawn(async move { find(&at) }).await;
            let (repo, key) = match found {
                Found::Repo(repo, key) => (repo, key),
                Found::NotRepo => return landed(&picker, Listing::NotRepo, None, cx),
                Found::Failed(error) => return landed(&picker, Listing::Failed(error), None, cx),
            };
            if let Some((_, rows)) = kept.filter(|(kept, _)| *kept == key) {
                landed(&picker, Listing::Listed { fresh: false }, Some(rows), cx);
            }
            let at = root.clone();
            let listed = cx
                .background_spawn(
                    async move { repo.listed_files().map(|files| rows_under(&at, files)) },
                )
                .await;
            match listed {
                Ok(rows) => {
                    let rows = Arc::new(rows);
                    cx.update(|cx| {
                        (cx.default_global::<Kept>().0).insert(root, (key, rows.clone()))
                    });
                    landed(&picker, Listing::Listed { fresh: true }, Some(rows), cx);
                }
                Err(error) => landed(&picker, Listing::Failed(error.to_string()), None, cx),
            }
        }));
    }

    /// Shows `rows`, if given, and `listing`, the selection staying on
    /// `selected`, the file selected until now, if it's still listed.
    fn land(
        &mut self,
        listing: Listing,
        rows: Option<Arc<Vec<Row>>>,
        selected: Option<PathBuf>,
        cx: &mut Context<Picker<Self>>,
    ) {
        self.listing = listing;
        match rows {
            // The same rows as shown: the selection stays.
            Some(rows) if rows == self.rows => return cx.notify(),
            Some(rows) => self.rows = rows,
            None => self.rows = Arc::default(),
        }
        self.refilter(false, selected, cx);
    }

    /// Matches the rows against the query: a new query selects its first
    /// match; a new list keeps the selection on `selected` if it's still
    /// listed, else where it is.
    fn refilter(
        &mut self,
        new_query: bool,
        selected: Option<PathBuf>,
        cx: &mut Context<Picker<Self>>,
    ) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| Some((ix, subsequence(&query, &row.label)?)))
            .collect();
        let kept = selected.and_then(|selected| {
            (self.matches.iter()).position(|(row, _)| self.rows[*row].path == selected)
        });
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| {
            picker.update(cx, |picker, cx| match kept {
                Some(ix) => picker.select(ix, cx),
                None => picker.matches_updated(new_query, cx),
            })
        });
    }

    /// The file the picker's selection is on, if any.
    fn selected_path(picker: &Picker<Self>) -> Option<PathBuf> {
        let (row, _) = picker.delegate.matches.get(picker.selected())?;
        Some(picker.delegate.rows[*row].path.clone())
    }

    fn row(&self, ix: usize) -> &Row {
        &self.rows[self.matches[ix].0]
    }
}

/// Lands `listing` and `rows` in `picker`, if it's still there.
fn landed(
    picker: &WeakEntity<Picker<GitFilesPicker>>,
    listing: Listing,
    rows: Option<Arc<Vec<Row>>>,
    cx: &mut gpui::AsyncApp,
) {
    picker
        .update(cx, |picker, cx| {
            let selected = GitFilesPicker::selected_path(picker);
            picker.delegate.land(listing, rows, selected, cx)
        })
        .ok();
}

#[cfg(test)]
impl GitFilesPicker {
    /// The matches' labels.
    pub(crate) fn labels(&self) -> Vec<String> {
        (0..self.matches.len())
            .map(|ix| self.row(ix).label.clone())
            .collect()
    }

    /// Whether this opening's list has landed, or there's none to make.
    pub(crate) fn settled(&self) -> bool {
        !matches!(
            self.listing,
            Listing::Pending | Listing::Listed { fresh: false }
        )
    }
}

impl PickerDelegate for GitFilesPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(true, None, cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        self.row(ix).label.clone().into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let row = self.row(ix);
        let highlights: Vec<Range<usize>> = char_ranges(&row.label, &self.matches[ix].1);
        let name = row.path.file_name().unwrap_or_default().to_string_lossy();
        let icon = file_icon_ref(&name);
        let path = icon.path();
        let icon = div()
            .size(px(14.))
            .flex_none()
            .debug_selector(|| format!("git-files-icon:{name}:{path}"))
            .child(icon.element(px(14.), ui.icon));
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .child(icon)
            .child(HighlightedLabel::new(row.label.clone(), highlights, &ui))
            .into_any_element()
    }

    /// Opens the file in the active pane, as a tree click does.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(workbench) = self.workbench.upgrade() else {
            return false;
        };
        let path = self.row(ix).path.clone();
        self.previous_focus = None;
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        Some(match &self.listing {
            Listing::Pending => tr!("workbench-git-files-listing"),
            Listing::NoRoot => tr!("workbench-git-files-no-folder"),
            Listing::NotRepo => tr!("workbench-git-files-not-repo"),
            Listing::Failed(error) => error.clone().into(),
            Listing::Listed { .. } if self.rows.is_empty() => tr!("workbench-git-files-none"),
            Listing::Listed { .. } if self.matches.is_empty() => {
                tr!("workbench-git-files-no-match")
            }
            Listing::Listed { .. } => tr!("workbench-git-files-count", count = self.matches.len()),
        })
    }
}

impl Workbench {
    /// The git files picker, while it shows.
    #[cfg(test)]
    pub(crate) fn git_files_picker(
        &self,
        cx: &App,
    ) -> Option<gpui::Entity<Picker<GitFilesPicker>>> {
        self.shown_palette(Palette::GitFiles, cx)
    }

    /// Shows the git files picker, focused, over the root's repository, in
    /// place of any other palette, or afresh if it shows.
    pub(crate) fn show_git_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_palette(window, cx);
        let previous = window.focused(cx);
        let root = self.root().map(Path::to_owned);
        let delegate = GitFilesPicker::new(cx.entity().downgrade(), root, previous);
        let picker = cx.new(|cx| {
            Picker::new(
                delegate,
                text!("workbench-git-files-placeholder"),
                fe_ui::ui_theme(cx),
                cx,
            )
        });
        picker.update(cx, |picker, cx| {
            picker.show(window, cx);
            picker.delegate.list(cx);
        });
        self.show_palette(Palette::GitFiles, picker, window, cx);
    }
}

#[cfg(test)]
#[path = "git_files_picker_tests.rs"]
mod git_files_picker_tests;
