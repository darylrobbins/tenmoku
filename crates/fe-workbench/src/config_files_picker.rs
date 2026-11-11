//! Open Config File… (`workbench.show_config_files`, LazyVim's
//! `<leader>fc`): a [`Picker`] over the files in the user's config folder
//! ([`fe_config::config_dir`]): `settings.toml`, `keymap.toml`, the vimrc,
//! themes, snippets and anything else there.
//!
//! The folder is walked off the UI thread by [`list_config_files`], a
//! small bounded walk: three folders deep at most, [`LIMIT`] entries read, and
//! leaving out dotfiles and any `cache`, `state` or `target` folder.
//! `settings.toml` comes first, then the rest by path. Rows match the query
//! by subsequence, ignoring case; Enter opens the file in the active pane.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use fe_ui::{HighlightedLabel, char_ranges, file_icon_ref};
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, InteractiveElement as _,
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

/// How many directory entries the walk reads, of any kind.
pub(crate) const LIMIT: usize = 500;

/// How many folders deep below the config folder the walk goes: a file at
/// `a/b/c/file` is the deepest found.
const DEPTH: usize = 3;

/// Folders the walk doesn't go into: what the app or a tool keeps there
/// isn't config.
const SKIPPED: [&str; 3] = ["cache", "state", "target"];

/// The files in `dir`, as the module docs say: walked a folder at a time,
/// reading at most `limit` entries in all, whatever they are (a dotfile or
/// a skipped folder counts), and stopping there. `settings.toml` (at the
/// top) is taken before any other and comes first, then the others by
/// path. Each folder's entries are taken in name order when it's read
/// whole; when the limit cuts one short, which of its entries were read is
/// the file system's order. A symlink to a file is listed; one to a folder
/// isn't followed. A folder that can't be read is passed over.
pub(crate) fn list_config_files(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let settings = dir.join(fe_config::paths::SETTINGS_FILE);
    // Taken before the walk, so no limit leaves it out.
    let mut files: Vec<PathBuf> = (limit > 0 && settings.is_file())
        .then(|| settings.clone())
        .into_iter()
        .collect();
    let took_settings = !files.is_empty();
    let mut left = limit - files.len();
    let mut folders = VecDeque::from([(dir.to_owned(), 0)]);
    while left > 0
        && let Some((folder, depth)) = folders.pop_front()
    {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        // No more entries are read than are left: the sort is of those.
        let mut read = Vec::new();
        for entry in entries {
            if left == 0 {
                break;
            }
            let Ok(entry) = entry else {
                left -= 1;
                continue;
            };
            // Already taken, and counted.
            if took_settings && depth == 0 && entry.file_name() == fe_config::paths::SETTINGS_FILE {
                continue;
            }
            left -= 1;
            read.push(entry);
        }
        read.sort_by_key(|entry| entry.file_name());
        for entry in read {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_file() || (kind.is_symlink() && path.is_file()) {
                files.push(path);
            } else if kind.is_dir() && depth < DEPTH && !SKIPPED.contains(&name.as_ref()) {
                folders.push_back((path, depth + 1));
            }
        }
    }
    files.sort_by(|a, b| (*a != settings).cmp(&(*b != settings)).then(a.cmp(b)));
    files
}

/// The user's config folder, if there is one. Under test, none unless a
/// test sets one, so no test reads the developer's.
fn config_dir(cx: &App) -> Option<PathBuf> {
    #[cfg(test)]
    return cx
        .try_global::<crate::open::ConfigDirOverride>()
        .and_then(|over| over.0.clone());
    #[cfg(not(test))]
    {
        let _ = cx;
        fe_config::config_dir()
    }
}

/// A config file, by its path from the config folder.
struct Row {
    path: PathBuf,
    label: String,
}

pub(crate) struct ConfigFilesPicker {
    workbench: WeakEntity<Workbench>,
    dir: Option<PathBuf>,
    /// Whether the walk has landed.
    listed: bool,
    rows: Vec<Row>,
    query: String,
    /// The rows matching the query, each with the chars of its label it
    /// matched.
    matches: Vec<(usize, Vec<usize>)>,
    /// Focused before it opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    _list: Option<Task<()>>,
}

impl ConfigFilesPicker {
    fn new(
        workbench: WeakEntity<Workbench>,
        dir: Option<PathBuf>,
        previous_focus: Option<FocusHandle>,
    ) -> Self {
        Self {
            workbench,
            listed: dir.is_none(),
            dir,
            rows: Vec::new(),
            query: String::new(),
            matches: Vec::new(),
            previous_focus,
            _list: None,
        }
    }

    /// Walks the config folder in the background.
    fn list(&mut self, cx: &mut Context<Picker<Self>>) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        self._list = Some(cx.spawn(async move |picker, cx| {
            let rows = cx
                .background_spawn(async move {
                    (list_config_files(&dir, LIMIT).into_iter())
                        .map(|path| {
                            let rest = path.strip_prefix(&dir).unwrap_or(&path);
                            let label = (rest.components())
                                .map(|part| part.as_os_str().to_string_lossy())
                                .collect::<Vec<_>>()
                                .join("/");
                            Row { path, label }
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            picker
                .update(cx, |picker, cx| {
                    picker.delegate.rows = rows;
                    picker.delegate.listed = true;
                    picker.delegate.refilter(cx);
                })
                .ok();
        }));
    }

    /// Matches the rows against the query, selecting the first.
    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| Some((ix, subsequence(&query, &row.label)?)))
            .collect();
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| picker.update(cx, |picker, cx| picker.matches_updated(true, cx)));
    }

    fn row(&self, ix: usize) -> &Row {
        &self.rows[self.matches[ix].0]
    }
}

#[cfg(test)]
impl ConfigFilesPicker {
    /// The matches' labels.
    pub(crate) fn labels(&self) -> Vec<String> {
        (0..self.matches.len())
            .map(|ix| self.row(ix).label.clone())
            .collect()
    }

    /// Whether the walk has landed.
    pub(crate) fn listed(&self) -> bool {
        self.listed
    }
}

impl PickerDelegate for ConfigFilesPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
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
        let highlights = char_ranges(&row.label, &self.matches[ix].1);
        let name = row.path.file_name().unwrap_or_default().to_string_lossy();
        let icon = file_icon_ref(&name);
        let path = icon.path();
        let icon = div()
            .size(px(14.))
            .flex_none()
            .debug_selector(|| format!("config-files-icon:{name}:{path}"))
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

    /// The file's path, as the row shows it.
    fn match_label(&self, ix: usize, _: &App) -> SharedString {
        self.row(ix).label.clone().into()
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
        if self.dir.is_none() {
            Some(tr!("workbench-navigation-config-files-no-folder"))
        } else if !self.listed {
            Some(tr!("workbench-navigation-config-files-listing"))
        } else if self.rows.is_empty() {
            Some(tr!("workbench-navigation-config-files-none"))
        } else if self.matches.is_empty() {
            Some(tr!("workbench-navigation-config-files-no-matches"))
        } else {
            None
        }
    }
}

impl Workbench {
    /// The config files picker, while it shows.
    #[cfg(test)]
    pub(crate) fn config_files_picker(
        &self,
        cx: &App,
    ) -> Option<gpui::Entity<Picker<ConfigFilesPicker>>> {
        self.shown_palette(Palette::ConfigFiles, cx)
    }

    /// Shows the config files picker, focused, in place of any other
    /// palette, or afresh if it shows.
    pub(crate) fn show_config_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_palette(window, cx);
        let previous = window.focused(cx);
        let dir = config_dir(cx);
        let delegate = ConfigFilesPicker::new(cx.entity().downgrade(), dir, previous);
        let placeholder = text!("workbench-navigation-config-files-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| {
            picker.show(window, cx);
            picker.delegate.list(cx);
        });
        self.show_palette(Palette::ConfigFiles, picker, window, cx);
    }
}

#[cfg(test)]
#[path = "config_files_picker_tests.rs"]
mod config_files_picker_tests;
