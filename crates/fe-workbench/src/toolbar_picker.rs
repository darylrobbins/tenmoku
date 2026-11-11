//! The toolbar's picker: what a click on a crumb, the tab list or the "…"
//! button lists, hosted by the workbench in the palette's place, as quick
//! open is. It picks for the pane it was opened from.
//!
//! - A directory's entries, read off the main thread, directories first,
//!   each sorted ignoring case. A file opens in the pane; a directory lists
//!   its own entries.
//! - The pane commands, each dispatched as a key would, in the pane, with
//!   the key that runs it where the pane's item has focus.
//! - The pane's tabs, in the strip's order; a pick shows one.
//!
//! Rows match the query by subsequence, ignoring case, in list order.

use std::{
    ffi::OsString,
    fs, io,
    ops::Range,
    path::{Path, PathBuf},
};

use fe_command::Source;
use fe_ui::{HighlightedLabel, IconRef, char_ranges, file_icon_ref, folder_icon_ref};
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, InteractiveElement as _,
    IntoElement as _, ParentElement as _, SharedString, Styled as _, Task, WeakEntity, Window, div,
    px,
};

use kiln_l10n::tr;
use kiln_shell::PaneView as _;

use crate::{
    Pane, PaneEvent, Workbench,
    pane::TabId,
    pane_toolbar::{self, ToolbarRequest},
    picker::{Picker, PickerDelegate},
};

/// The pane commands "…" lists, as `(command, title)`, the title a message:
/// the design's (`docs/design/design-system.md`, Pane controls), in its
/// order, then the rest. [`MAXIMIZE`] and [`PIN_TAB`] are in their places.
const PANE_COMMANDS: [(&str, &str); 16] = [
    ("pane.split_right", "workbench-pane-action-split-right"),
    ("pane.split_down", "workbench-pane-action-split-down"),
    MAXIMIZE,
    ("pane.exchange", "workbench-pane-action-exchange"),
    PIN_TAB,
    (
        "workbench.close_other_tabs",
        "workbench-pane-action-close-other-tabs",
    ),
    (
        "workbench.reopen_closed_editor",
        "workbench-pane-action-reopen-closed-tab",
    ),
    ("workbench.close_tab", "workbench-pane-action-close-tab"),
    ("pane.close", "workbench-pane-action-close-pane"),
    ("pane.move_tab_left", "workbench-pane-action-move-tab-left"),
    (
        "pane.move_tab_right",
        "workbench-pane-action-move-tab-right",
    ),
    ("pane.move_tab_up", "workbench-pane-action-move-tab-up"),
    ("pane.move_tab_down", "workbench-pane-action-move-tab-down"),
    (
        "view.toggle_breadcrumbs",
        "workbench-pane-action-toggle-breadcrumbs",
    ),
    // Listed only while the active tab is pinned, in [`PIN_TAB`]'s place.
    UNPIN_TAB,
    // Listed only while the pane is zoomed, in [`MAXIMIZE`]'s place.
    RESTORE,
];

/// Zooms the pane; while it's zoomed, [`RESTORE`] is listed instead, which
/// runs the same command.
const MAXIMIZE: (&str, &str) = ("pane.toggle_maximize", "workbench-pane-action-maximize");
const RESTORE: (&str, &str) = ("pane.toggle_maximize", "workbench-pane-action-restore");

/// Pins the active tab; while it's pinned, [`UNPIN_TAB`] is listed instead.
const PIN_TAB: (&str, &str) = ("workbench.pin_tab", "workbench-pane-action-pin-tab");
const UNPIN_TAB: (&str, &str) = ("workbench.unpin_tab", "workbench-pane-action-unpin-tab");

/// A pane command's row: its command, title and key.
type CommandRow = (&'static str, SharedString, Option<String>);

/// A tab's row: its tab, title, folder (when tabs share a title) and icon.
struct TabRow {
    id: TabId,
    title: SharedString,
    description: Option<SharedString>,
    icon: Option<IconRef>,
}

/// A directory entry: a symlink isn't followed, so it's a file.
struct Entry {
    name: OsString,
    label: SharedString,
    dir: bool,
}

enum List {
    Entries {
        dir: PathBuf,
        /// `None` while it's read.
        entries: Option<io::Result<Vec<Entry>>>,
    },
    Commands(Vec<CommandRow>),
    Tabs(Vec<TabRow>),
}

pub(crate) struct ToolbarPicker {
    pane: WeakEntity<Pane>,
    /// Records the caret before a pick jumps.
    workbench: WeakEntity<Workbench>,
    root: Option<PathBuf>,
    list: List,
    /// The row to select once the list is in, for an empty query.
    select: Option<usize>,
    query: String,
    /// The rows matching the query, with the byte ranges matched.
    matches: Vec<(usize, Vec<Range<usize>>)>,
    /// Moves on with each list, so a row pressed in one can't pick from
    /// the next.
    listing: u64,
    read: Option<Task<()>>,
    /// Focused before it opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    /// Whether the pane is zoomed, so its Maximize row reads Restore: set
    /// by the workbench as it opens the picker, while it can't be read.
    pub(crate) zoomed: bool,
    /// Directory reads, and those of them that ran off the main thread.
    #[cfg(test)]
    reads: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
}

impl ToolbarPicker {
    pub(crate) fn new() -> Self {
        Self {
            pane: WeakEntity::new_invalid(),
            workbench: WeakEntity::new_invalid(),
            root: None,
            list: List::Commands(Vec::new()),
            select: None,
            query: String::new(),
            matches: Vec::new(),
            listing: 0,
            read: None,
            previous_focus: None,
            zoomed: false,
            #[cfg(test)]
            reads: Default::default(),
        }
    }

    /// The picker is opening for `request`, from `pane`, with
    /// `previous_focus` focused until now.
    pub(crate) fn open(
        &mut self,
        request: ToolbarRequest,
        pane: WeakEntity<Pane>,
        workbench: WeakEntity<Workbench>,
        root: Option<PathBuf>,
        previous_focus: Option<FocusHandle>,
        cx: &mut Context<Picker<Self>>,
    ) {
        self.pane = pane;
        self.workbench = workbench;
        self.root = root;
        if previous_focus.is_some() {
            self.previous_focus = previous_focus;
        }
        self.query.clear();
        match request {
            ToolbarRequest::Entries { dir, select } => self.list_entries(dir, select, cx),
            ToolbarRequest::PaneCommands => {
                let rows = self.command_rows(cx);
                self.set_list(List::Commands(rows), None);
            }
            ToolbarRequest::Tabs => {
                let (rows, active) = self.tab_rows(cx);
                self.set_list(List::Tabs(rows), active);
            }
        }
    }

    /// The pane commands' rows, each with the key that runs it where the
    /// pane's item had focus; Pin Tab is Unpin Tab while the active tab is
    /// pinned, and Maximize is Restore while the pane is zoomed.
    fn command_rows(&self, cx: &App) -> Vec<CommandRow> {
        let pane = self.pane.upgrade();
        let pinned = pane.as_ref().is_some_and(|pane| {
            let state = pane.read(cx).pane();
            state.active().is_some_and(|ix| state.is_pinned(ix))
        });
        let stack = pane.map_or_else(pane_toolbar::editor_context, |pane| {
            pane.read(cx).menu_context()
        });
        (PANE_COMMANDS.iter())
            .filter(|&&row| row != UNPIN_TAB && row != RESTORE)
            .map(|&row| match row {
                PIN_TAB if pinned => UNPIN_TAB,
                MAXIMIZE if self.zoomed => RESTORE,
                row => row,
            })
            .map(|(id, title)| (id, tr!(title), pane_toolbar::menu_shortcut(id, &stack, cx)))
            .collect()
    }

    /// The pane's tabs as its strip draws them, and the active one's row.
    fn tab_rows(&self, cx: &App) -> (Vec<TabRow>, Option<usize>) {
        let Some(pane) = self.pane.upgrade() else {
            return (Vec::new(), None);
        };
        let state = pane.read(cx).pane();
        let rows = (state.tabs(cx).into_iter().zip(state.items()))
            .map(|(tab, item)| TabRow {
                id: item.id(),
                title: item.tab_title(cx),
                description: tab.description_text().cloned(),
                icon: tab.icon_ref().cloned(),
            })
            .collect();
        (rows, state.active())
    }

    fn set_list(&mut self, list: List, select: Option<usize>) {
        self.list = list;
        self.select = select;
        self.listing += 1;
        self.read = None;
        self.matches.clear();
    }

    /// Lists `dir`'s entries once read in the background, `select`ing the
    /// one so named.
    fn list_entries(
        &mut self,
        dir: PathBuf,
        select: Option<OsString>,
        cx: &mut Context<Picker<Self>>,
    ) {
        let list = List::Entries {
            dir: dir.clone(),
            entries: None,
        };
        self.set_list(list, None);
        cx.notify();
        let listing = self.listing;
        #[cfg(test)]
        let (reads, executor) = (self.reads.clone(), cx.background_executor().clone());
        let read = cx.background_spawn(async move {
            #[cfg(test)]
            {
                let mut reads = reads.lock().unwrap();
                reads.0 += 1;
                reads.1 += usize::from(!executor.is_main_thread());
            }
            read_entries(&dir)
        });
        self.read = Some(cx.spawn(async move |picker, cx| {
            let read = read.await;
            picker
                .update(cx, |picker, cx| {
                    let this = &mut picker.delegate;
                    let List::Entries { entries, .. } = &mut this.list else {
                        return;
                    };
                    if this.listing != listing {
                        return;
                    }
                    this.select = select.and_then(|select| {
                        let found = read.as_ref().ok()?;
                        found.iter().position(|entry| entry.name == select)
                    });
                    *entries = Some(read);
                    this.read = None;
                    this.refilter(true, cx);
                })
                .ok();
        }));
    }

    fn row_count(&self) -> usize {
        match &self.list {
            List::Entries {
                entries: Some(Ok(entries)),
                ..
            } => entries.len(),
            List::Entries { .. } => 0,
            List::Commands(rows) => rows.len(),
            List::Tabs(rows) => rows.len(),
        }
    }

    fn label(&self, row: usize) -> &str {
        match &self.list {
            List::Entries {
                entries: Some(Ok(entries)),
                ..
            } => &entries[row].label,
            List::Entries { .. } => "",
            List::Commands(rows) => &rows[row].1,
            List::Tabs(rows) => &rows[row].title,
        }
    }

    /// Matches the rows against the query, then selects the first match,
    /// or for an empty query the row to select.
    fn refilter(&mut self, new_query: bool, cx: &mut Context<Picker<Self>>) {
        let query: Vec<char> = self.query.chars().flat_map(char::to_lowercase).collect();
        self.matches = (0..self.row_count())
            .filter_map(|row| {
                let label = self.label(row);
                let indices = subsequence(&query, label)?;
                Some((row, char_ranges(label, &indices)))
            })
            .collect();
        let select = self
            .select
            .filter(|_| self.query.is_empty())
            .and_then(|select| self.matches.iter().position(|(row, _)| *row == select));
        cx.notify();
        let picker = cx.entity();
        // The picker is being updated now.
        cx.defer(move |cx| {
            picker.update(cx, |picker, cx| {
                picker.matches_updated(new_query, cx);
                if let Some(select) = select {
                    picker.select(select, cx);
                }
            })
        });
    }

    /// Focuses what was focused before it opened.
    fn restore_focus(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    /// A pick is a jump: back returns to the caret as it is now. A running
    /// go-to-definition is dropped first, as `nav_jump` does, so its answer
    /// can't land over the pick.
    fn push_history(&self, cx: &mut Context<Picker<Self>>) {
        if let Some(workbench) = self.workbench.upgrade() {
            workbench.update(cx, |workbench, cx| {
                workbench.nav_state.goto.next();
                workbench.push_history(cx)
            });
        }
    }

    /// Opens a file in the pane, active and focused, or lists a directory.
    fn pick_entry(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> bool {
        let List::Entries {
            dir,
            entries: Some(Ok(entries)),
        } = &self.list
        else {
            return false;
        };
        let path = dir.join(&entries[row].name);
        if entries[row].dir {
            self.query.clear();
            let picker = cx.entity();
            // The picker is being updated now.
            cx.defer(move |cx| {
                let input = picker.read(cx).input().clone();
                input.update(cx, |input, cx| input.set_text("", cx));
            });
            self.list_entries(path, None, cx);
            return false;
        }
        // Its pane closed meanwhile: nothing to open it in.
        let Some(pane) = self.pane.upgrade() else {
            return true;
        };
        self.previous_focus = None;
        self.push_history(cx);
        pane.update(cx, |pane, cx| {
            pane.open(path, window, cx);
            cx.emit(PaneEvent::Activated);
        });
        true
    }

    /// Activates the pane, then dispatches the command there as a key
    /// would.
    fn pick_command(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> bool {
        let List::Commands(rows) = &self.list else {
            return false;
        };
        let command = rows[row].0;
        // Its pane closed meanwhile: the command would run in another.
        let Some(pane) = self.pane.upgrade() else {
            return true;
        };
        self.previous_focus = None;
        let focus = pane.read(cx).focus_target(cx);
        window.focus(&focus, cx);
        pane.update(cx, |_, cx| cx.emit(PaneEvent::Activated));
        pane_toolbar::dispatch(command, Source::Palette, window, cx);
        true
    }

    /// Activates the pane and shows the tab, if it's still there.
    fn pick_tab(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> bool {
        let List::Tabs(rows) = &self.list else {
            return false;
        };
        let tab = rows[row].id;
        // Its pane closed meanwhile: nothing to show.
        let Some(pane) = self.pane.upgrade() else {
            return true;
        };
        self.previous_focus = None;
        pane.update(cx, |pane, cx| {
            match pane.pane().position(tab) {
                Some(ix) => pane.activate_at(ix, window, cx),
                None => window.focus(&pane.focus_target(cx), cx),
            }
            cx.emit(PaneEvent::Activated);
            cx.notify();
        });
        true
    }
}

/// The char indices of `text` matching `query`'s chars, lowercase, in
/// order, ignoring case; `None` if they don't all match.
pub(crate) fn subsequence(query: &[char], text: &str) -> Option<Vec<usize>> {
    let mut wanted = query.iter().peekable();
    let mut indices = Vec::new();
    // A char can lowercase to several, as `İ` does: each may match.
    for (ix, ch) in text.chars().enumerate() {
        for lower in ch.to_lowercase() {
            if wanted.next_if(|&&want| want == lower).is_some() && indices.last() != Some(&ix) {
                indices.push(ix);
            }
        }
    }
    wanted.peek().is_none().then_some(indices)
}

/// `dir`'s entries, directories first, each sorted ignoring case.
fn read_entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        entries.push(Entry {
            label: name.to_string_lossy().into_owned().into(),
            dir: entry.file_type()?.is_dir(),
            name,
        });
    }
    entries
        .sort_by_cached_key(|entry| (!entry.dir, entry.label.to_lowercase(), entry.name.clone()));
    Ok(entries)
}

#[cfg(test)]
impl ToolbarPicker {
    /// The matching rows' labels.
    pub(crate) fn labels(&self) -> Vec<String> {
        let rows = self.matches.iter();
        rows.map(|(row, _)| self.label(*row).to_owned()).collect()
    }

    /// The matching pane commands' IDs.
    pub(crate) fn commands(&self) -> Vec<&'static str> {
        let List::Commands(rows) = &self.list else {
            return Vec::new();
        };
        let matches = self.matches.iter();
        matches.map(|(row, _)| rows[*row].0).collect()
    }

    /// The matching pane commands' shortcuts.
    pub(crate) fn shortcuts(&self) -> Vec<Option<String>> {
        let List::Commands(rows) = &self.list else {
            return Vec::new();
        };
        let matches = self.matches.iter();
        matches.map(|(row, _)| rows[*row].2.clone()).collect()
    }

    /// Match `ix`'s highlighted byte ranges.
    pub(crate) fn highlights(&self, ix: usize) -> Vec<Range<usize>> {
        self.matches[ix].1.clone()
    }

    /// Directory reads, and those of them that ran off the main thread.
    pub(crate) fn reads(&self) -> (usize, usize) {
        *self.reads.lock().unwrap()
    }

    pub(crate) fn status_text(&self) -> Option<SharedString> {
        self.status()
    }
}

impl PickerDelegate for ToolbarPicker {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        self.query = query;
        self.refilter(true, cx);
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn match_id(&self, ix: usize) -> SharedString {
        format!("{}:{}", self.listing, self.matches[ix].0).into()
    }

    fn render_match(&self, ix: usize, _: bool, cx: &App) -> AnyElement {
        let ui = fe_ui::ui_theme(cx);
        let (row, highlights) = &self.matches[ix];
        let label = HighlightedLabel::new(self.label(*row).to_owned(), highlights.clone(), &ui);
        let icon = match &self.list {
            List::Entries {
                entries: Some(Ok(entries)),
                ..
            } => Some(match &entries[*row] {
                entry if entry.dir => (&entry.label, folder_icon_ref(false)),
                entry => (&entry.label, file_icon_ref(&entry.label)),
            }),
            List::Tabs(rows) => (rows[*row].icon.clone()).map(|icon| (&rows[*row].title, icon)),
            _ => None,
        };
        let description = match &self.list {
            List::Tabs(rows) => rows[*row].description.clone(),
            _ => None,
        };
        let shortcut = match &self.list {
            List::Commands(rows) => rows[*row].2.clone(),
            _ => None,
        };
        let icon = icon.map(|(name, icon)| {
            let path = icon.path();
            div()
                .size(px(14.))
                .flex_none()
                .debug_selector(|| format!("toolbar-picker-icon:{name}:{path}"))
                .child(icon.element(px(14.), ui.icon))
        });
        let shortcut =
            shortcut.map(|shortcut| div().ml_auto().text_color(ui.muted_text).child(shortcut));
        let description = description.map(|description| {
            div()
                .min_w_0()
                .truncate()
                .text_color(ui.muted_text)
                .child(description)
        });
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .children(icon)
            .child(label)
            .children(description)
            .children(shortcut)
            .into_any_element()
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return false;
        };
        match self.list {
            List::Entries { .. } => self.pick_entry(row, window, cx),
            List::Commands(_) => self.pick_command(row, window, cx),
            List::Tabs(_) => self.pick_tab(row, window, cx),
        }
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.read = None;
        self.restore_focus(window, cx);
    }

    /// The directory listed, relative to the root, or why it couldn't be
    /// read.
    fn status(&self) -> Option<SharedString> {
        match &self.list {
            List::Entries { dir, entries } => {
                if let Some(Err(error)) = entries {
                    let (dir, error) = (dir.display().to_string(), error.to_string());
                    return Some(tr!(
                        "workbench-pane-cannot-read-dir",
                        dir = dir,
                        error = error
                    ));
                }
                let root = self.root.as_deref();
                let relative = root.and_then(|root| dir.strip_prefix(root).ok());
                let shown = match relative {
                    Some(relative) if relative.as_os_str().is_empty() => {
                        crate::pane::file_name(root)
                    }
                    Some(relative) => relative.display().to_string(),
                    None => dir.display().to_string(),
                };
                Some(shown.into())
            }
            List::Commands(_) | List::Tabs(_) => None,
        }
    }
}
