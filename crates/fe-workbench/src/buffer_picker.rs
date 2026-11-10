//! Show Buffers (`buffers.show`, LazyVim's `<leader>fb`, `<leader>,` and
//! `<leader>bb`): a [`Picker`] over the open editors across the panes.
//!
//! The rows are the active pane's tabs in order of use, the order
//! ctrl-tab's list keeps ([`crate::tab_switcher`]), then each other pane's
//! the same way, in the order the panes are laid out: a pane keeps no
//! clock its tabs could be ordered by across panes. A file open in several
//! panes is one row, where it was met first. Each row shows its name, a
//! dot if it has unsaved changes, and its folder from the window's root,
//! dimmed; the active editor's row is marked. Rows match the query by
//! subsequence over the path from the root, ignoring case.
//!
//! With no query the selection starts on the second row, the buffer used
//! before the current one, so `<leader>,` then Enter goes back to it, as
//! alt-tab does. Enter activates the row's tab in the active pane, or opens
//! its file there if only another pane shows it; an untitled buffer only
//! another pane shows is activated in that pane.
//!
//! kiln-shell's picker gives a delegate no keys of its own (in its normal
//! mode an unbound letter is dropped), so there's no `d` to close a buffer
//! from the list.

use std::path::{Path, PathBuf};

use fe_ui::{HighlightedLabel, char_ranges, file_icon_ref};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement as _, ParentElement as _, SharedString, Styled as _, WeakEntity, Window, div, px,
};
use kiln_l10n::{text, tr};
use kiln_shell::PaneView as _;

use crate::{
    Pane, Workbench,
    pane::TabId,
    picker::{Picker, PickerDelegate},
    toolbar_picker::subsequence,
    workbench::Palette,
};

/// An open editor.
struct Row {
    /// The pane it was listed from, and its tab there.
    pane: WeakEntity<Pane>,
    tab: TabId,
    path: Option<PathBuf>,
    /// Its tab's title: the file's name, or an untitled buffer's.
    name: String,
    /// Its folder from the root, or whole outside it; empty at the root
    /// and for an untitled buffer.
    folder: String,
    /// What the query matches: the folder and name, as a path.
    matched: String,
    /// Where the name starts in `matched`, in chars.
    name_start: usize,
    modified: bool,
    /// It's the active pane's active tab.
    current: bool,
}

pub(crate) struct BufferPicker {
    workbench: WeakEntity<Workbench>,
    rows: Vec<Row>,
    query: String,
    /// The rows matching the query, each with the chars of `matched` it
    /// matched.
    matches: Vec<(usize, Vec<usize>)>,
    /// Focused before it opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
}

impl BufferPicker {
    fn new(
        workbench: WeakEntity<Workbench>,
        rows: Vec<Row>,
        previous_focus: Option<FocusHandle>,
    ) -> Self {
        Self {
            workbench,
            rows,
            query: String::new(),
            matches: Vec::new(),
            previous_focus,
        }
    }

    /// Matches the rows against the query. A new query selects its first
    /// match, but for the empty query, which selects the buffer before the
    /// current one.
    fn refilter(&mut self, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (self.rows.iter().enumerate())
            .filter_map(|(ix, row)| Some((ix, subsequence(&query, &row.matched)?)))
            .collect();
        let previous =
            query.is_empty() && self.matches.len() > 1 && self.rows[self.matches[0].0].current;
        cx.notify();
        let picker = cx.entity();
        // `Picker::matches_updated` borrows the picker, which is being
        // updated now.
        cx.defer(move |cx| {
            picker.update(cx, |picker, cx| {
                picker.matches_updated(true, cx);
                if previous {
                    picker.select(1, cx);
                }
            })
        });
    }

    fn row(&self, ix: usize) -> &Row {
        &self.rows[self.matches[ix].0]
    }
}

#[cfg(test)]
impl BufferPicker {
    /// The matches: each name, folder, whether it's modified and whether
    /// it's current.
    pub(crate) fn labels(&self) -> Vec<(String, String, bool, bool)> {
        (0..self.matches.len())
            .map(|ix| {
                let row = self.row(ix);
                (
                    row.name.clone(),
                    row.folder.clone(),
                    row.modified,
                    row.current,
                )
            })
            .collect()
    }
}

/// `path`'s folder as a row shows it: from `root` with `/`s, empty at the
/// root itself, or whole outside it.
fn folder_label(path: &Path, root: Option<&Path>) -> String {
    let Some(parent) = path.parent() else {
        return String::new();
    };
    match root.and_then(|root| parent.strip_prefix(root).ok()) {
        Some(rest) => (rest.components())
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        None => parent.display().to_string(),
    }
}

/// The open editors (and viewers of a file) across `workbench`'s panes, in
/// the module docs' order.
fn rows(workbench: &Workbench, cx: &App) -> Vec<Row> {
    let active = workbench.pane().clone();
    let mut panes = vec![active.clone()];
    panes.extend(
        (workbench.panes(cx).into_iter()).filter(|pane| pane.entity_id() != active.entity_id()),
    );
    let current = active.read(cx).active_tab().map(|tab| tab.id());
    let root = workbench.root();
    let mut rows: Vec<Row> = Vec::new();
    for pane in &panes {
        let read = pane.read(cx);
        for id in read.tabs_by_recency() {
            let Some(tab) = read.tabs().iter().find(|tab| tab.id() == id) else {
                continue;
            };
            if tab.editor().is_none() && tab.path.is_none() {
                continue;
            }
            let path = tab.path.clone();
            if path.is_some() && rows.iter().any(|row| row.path == path) {
                continue;
            }
            let name = match &path {
                Some(path) => path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                None => tab.item().tab_title(cx).to_string(),
            };
            let folder = path
                .as_deref()
                .map_or_else(String::new, |path| folder_label(path, root));
            let matched = match folder.is_empty() {
                true => name.clone(),
                false => format!("{folder}/{name}"),
            };
            let name_start = matched.chars().count() - name.chars().count();
            rows.push(Row {
                pane: pane.downgrade(),
                tab: id,
                current: pane.entity_id() == active.entity_id() && Some(id) == current,
                modified: tab.item().is_dirty(cx),
                path,
                name,
                folder,
                matched,
                name_start,
            });
        }
    }
    rows
}

impl PickerDelegate for BufferPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The row's pane and tab.
    fn match_id(&self, ix: usize) -> SharedString {
        let row = self.row(ix);
        format!("{}:{:?}", row.pane.entity_id(), row.tab).into()
    }

    fn rows_fill_width(&self) -> bool {
        true
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let row = self.row(ix);
        let matched = &self.matches[ix].1;
        // The chars matched in the name, counted from its start.
        let in_name: Vec<usize> = (matched.iter())
            .filter_map(|&ix| ix.checked_sub(row.name_start))
            .collect();
        let label = HighlightedLabel::new(row.name.clone(), char_ranges(&row.name, &in_name), &ui);
        let icon = file_icon_ref(&row.name);
        let path = icon.path();
        let name = row.name.clone();
        let icon = div()
            .size(px(14.))
            .flex_none()
            .debug_selector(|| format!("buffer-icon:{name}:{path}"))
            .child(icon.element(px(14.), ui.icon));
        let modified = div()
            .w(px(10.))
            .flex_none()
            .text_color(ui.muted_text)
            .children(row.modified.then_some("●"));
        let muted = |text: String| {
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(ui.muted_text)
                .child(text)
        };
        let current = (row.current).then(|| muted("current".into()).ml_auto().flex_none());
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(modified)
            .child(icon)
            .child(div().flex_none().child(label))
            .child(muted(row.folder.clone()))
            .children(current)
            .into_any_element()
    }

    /// Activates the row's tab in the active pane, or opens its file there;
    /// an untitled buffer of another pane is activated in its own.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(workbench) = self.workbench.upgrade() else {
            return false;
        };
        let row = self.row(ix);
        let (tab, path, own) = (row.tab, row.path.clone(), row.pane.upgrade());
        let active = workbench.read(cx).pane().clone();
        self.previous_focus = None;
        let shown = |pane: &Entity<Pane>, cx: &App| pane.read(cx).pane().position(tab);
        if let Some(ix) = shown(&active, cx) {
            active.update(cx, |pane, cx| pane.activate_at(ix, window, cx));
        } else if let Some(path) = path {
            active.update(cx, |pane, cx| pane.open(path, window, cx));
        } else if let Some(pane) = own
            && let Some(ix) = shown(&pane, cx)
        {
            pane.update(cx, |pane, cx| pane.activate_at(ix, window, cx));
            // The pane becomes the workspace's active one, so commands and
            // the status bar follow it, as a click in it would make it.
            workbench.update(cx, |workbench, cx| workbench.activate(pane, window, cx));
        } else {
            // Closed meanwhile: nothing to show.
            return true;
        }
        workbench.update(cx, |_, cx| cx.notify());
        true
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        if self.rows.is_empty() {
            Some(tr!("workbench-navigation-buffers-none"))
        } else if self.matches.is_empty() {
            Some(tr!("workbench-navigation-buffers-no-matches"))
        } else {
            None
        }
    }
}

impl Workbench {
    /// The buffer picker, while it shows.
    #[cfg(test)]
    pub(crate) fn buffer_picker(&self, cx: &App) -> Option<Entity<Picker<BufferPicker>>> {
        self.shown_palette(Palette::Buffers, cx)
    }

    /// Shows the buffer picker, focused, over the open editors, in place of
    /// any other palette, or afresh if it shows.
    pub(crate) fn show_buffers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_palette(window, cx);
        let previous = window.focused(cx);
        let rows = rows(self, cx);
        let delegate = BufferPicker::new(cx.entity().downgrade(), rows, previous);
        let placeholder = text!("workbench-navigation-buffers-placeholder");
        let picker = cx.new(|cx| Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx));
        picker.update(cx, |picker, cx| picker.show(window, cx));
        self.show_palette(Palette::Buffers, picker, window, cx);
    }
}

#[cfg(test)]
#[path = "buffer_picker_tests.rs"]
mod buffer_picker_tests;
