//! The toolbar's picker: what a click on a crumb or the "…" button lists,
//! hosted by the workbench in the palette's place, as quick open is. It
//! picks for the pane it was opened from.
//!
//! - A directory's entries, read off the main thread, directories first,
//!   each sorted ignoring case. A file opens in the pane; a directory lists
//!   its own entries.
//! - A symbol's siblings in the outline. A pick is checked against the
//!   outline it was listed from: if the outline changed since, the list is
//!   rebuilt for the same crumb, by its chain of names, and the pick
//!   refused, so it never lands on a moved symbol.
//! - The pane commands, each dispatched as a key would, in the pane.
//!
//! Rows match the query by subsequence, ignoring case, in list order.

use std::{
    ffi::OsString,
    fs, io,
    ops::Range,
    path::{Path, PathBuf},
};

use fe_command::Source;
use fe_gpui::EditorView;
use fe_syntax::Outline;
use fe_ui::{HighlightedLabel, Icon, char_ranges, file_icon, folder_icon};
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, Focusable as _, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, Task, WeakEntity, Window, div, px, svg,
};

use crate::{
    OutlinePanel, Pane, PaneEvent, Workbench,
    pane_toolbar::{self, ToolbarRequest},
    picker::{Picker, PickerDelegate},
};

/// The pane commands "…" lists, as `(command, title)`.
const PANE_COMMANDS: [(&str, &str); 9] = [
    ("workbench.close_tab", "Close Tab"),
    ("pane.close", "Close Pane"),
    ("pane.split_right", "Split Right"),
    ("pane.split_down", "Split Down"),
    ("pane.move_tab_left", "Move Tab Left"),
    ("pane.move_tab_right", "Move Tab Right"),
    ("pane.move_tab_up", "Move Tab Up"),
    ("pane.move_tab_down", "Move Tab Down"),
    ("view.toggle_breadcrumbs", "Toggle Breadcrumbs"),
];

/// A directory entry: a symlink isn't followed, so it's a file.
struct Entry {
    name: OsString,
    label: SharedString,
    dir: bool,
}

/// A symbol: its index in the outline's items, its name and where its
/// name starts.
type Symbol = (u32, SharedString, usize);

enum List {
    Entries {
        dir: PathBuf,
        /// `None` while it's read.
        entries: Option<io::Result<Vec<Entry>>>,
    },
    Symbols {
        editor: WeakEntity<EditorView>,
        /// The clicked crumb's chain of names, from the top level.
        names: Vec<SharedString>,
        /// The outline's generation they're listed from: `None` if no
        /// outline showed for the editor then.
        generation: Option<u64>,
        symbols: Vec<Symbol>,
    },
    Commands(Vec<(&'static str, &'static str, Option<String>)>),
}

pub(crate) struct ToolbarPicker {
    pane: WeakEntity<Pane>,
    /// Records the caret before a pick jumps.
    workbench: WeakEntity<Workbench>,
    outline: WeakEntity<OutlinePanel>,
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
    /// Directory reads, and those of them that ran off the main thread.
    #[cfg(test)]
    reads: std::sync::Arc<std::sync::Mutex<(usize, usize)>>,
}

impl ToolbarPicker {
    pub(crate) fn new(outline: WeakEntity<OutlinePanel>) -> Self {
        Self {
            pane: WeakEntity::new_invalid(),
            workbench: WeakEntity::new_invalid(),
            outline,
            root: None,
            list: List::Commands(Vec::new()),
            select: None,
            query: String::new(),
            matches: Vec::new(),
            listing: 0,
            read: None,
            previous_focus: None,
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
            ToolbarRequest::Symbols {
                editor,
                generation,
                item,
                names,
            } => self.list_symbols(editor, names, Some((generation, item)), cx),
            ToolbarRequest::PaneCommands => {
                let rows = PANE_COMMANDS
                    .iter()
                    .map(|&(id, title)| (id, title, pane_toolbar::shortcut(id, cx)))
                    .collect();
                self.set_list(List::Commands(rows), None);
            }
        }
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

    /// Lists the siblings of the item at the end of the chain `names`, from
    /// the outline now shown for `editor`: of item `known.1` if that
    /// outline is still of generation `known.0`, else found by its names.
    fn list_symbols(
        &mut self,
        editor: WeakEntity<EditorView>,
        names: Vec<SharedString>,
        known: Option<(u64, u32)>,
        cx: &App,
    ) {
        let outline = self.outline.upgrade();
        let shown = outline
            .as_ref()
            .and_then(|outline| outline.read(cx).shown_for(editor.entity_id()).flatten());
        let (generation, item) = match shown {
            Some((outline, generation)) => {
                let item = match known {
                    Some((known, item)) if known == generation => Some(item as usize),
                    _ => find(outline, &names),
                };
                (Some(generation), item.map(|item| (outline, item)))
            }
            None => (None, None),
        };
        let symbols: Vec<Symbol> = item.map_or_else(Vec::new, |(outline, item)| {
            let parent = outline.items[item].parent;
            let items = outline.items.iter().enumerate();
            items
                .filter(|(_, sibling)| sibling.parent == parent)
                .map(|(ix, sibling)| {
                    let name = SharedString::from(sibling.name.clone());
                    (ix as u32, name, sibling.name_range.start)
                })
                .collect()
        });
        let select =
            item.and_then(|(_, item)| symbols.iter().position(|(ix, ..)| *ix as usize == item));
        let list = List::Symbols {
            editor,
            names,
            generation,
            symbols,
        };
        self.set_list(list, select);
    }

    /// The outline changed: a symbol list made while no outline showed for
    /// its editor, such as an inactive pane's crumb's, is made again.
    pub(crate) fn outline_changed(&mut self, cx: &mut Context<Picker<Self>>) {
        let List::Symbols {
            editor,
            names,
            generation: None,
            ..
        } = &self.list
        else {
            return;
        };
        let (editor, names) = (editor.clone(), names.clone());
        self.list_symbols(editor, names, None, cx);
        if let List::Symbols {
            generation: Some(_),
            ..
        } = self.list
        {
            self.refilter(true, cx);
        }
    }

    fn row_count(&self) -> usize {
        match &self.list {
            List::Entries {
                entries: Some(Ok(entries)),
                ..
            } => entries.len(),
            List::Entries { .. } => 0,
            List::Symbols { symbols, .. } => symbols.len(),
            List::Commands(rows) => rows.len(),
        }
    }

    fn label(&self, row: usize) -> &str {
        match &self.list {
            List::Entries {
                entries: Some(Ok(entries)),
                ..
            } => &entries[row].label,
            List::Entries { .. } => "",
            List::Symbols { symbols, .. } => &symbols[row].1,
            List::Commands(rows) => rows[row].1,
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

    /// Selects the symbol's name start and focuses its editor, unless the
    /// outline changed since it was listed, or is behind the editor's text
    /// (edited since, its outline still to land): then it's listed again
    /// and the pick refused.
    fn pick_symbol(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> bool {
        let List::Symbols {
            editor,
            names,
            generation,
            symbols,
        } = &self.list
        else {
            return false;
        };
        // Its editor closed meanwhile: nothing to select in.
        let Some(editor) = editor.upgrade() else {
            return true;
        };
        let version = editor.read(cx).model().snapshot().version();
        let current = self.outline.upgrade().and_then(|outline| {
            let shown = outline.read(cx).shown_for(editor.entity_id()).flatten();
            shown
                .filter(|(outline, _)| outline.version == version)
                .map(|(_, generation)| generation)
        });
        if current.is_none() || current != *generation {
            let (editor, names) = (editor.downgrade(), names.clone());
            self.list_symbols(editor, names, None, cx);
            self.refilter(true, cx);
            return false;
        }
        let start = symbols[row].2;
        self.push_history(cx);
        editor.update(cx, |editor, cx| editor.select_and_reveal(start..start, cx));
        window.focus(&editor.focus_handle(cx), cx);
        self.previous_focus = None;
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

/// The item at the end of the chain `names` from the top level.
fn find(outline: &Outline, names: &[SharedString]) -> Option<usize> {
    let mut parent = None;
    let mut found = None;
    for name in names {
        let ix = outline
            .items
            .iter()
            .position(|item| item.parent == parent && item.name == name.as_ref())?;
        found = Some(ix);
        parent = Some(ix as u32);
    }
    found
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
                entry if entry.dir => folder_icon(false),
                entry => file_icon(&entry.label),
            }),
            _ => None,
        };
        let shortcut = match &self.list {
            List::Commands(rows) => rows[*row].2.clone(),
            _ => None,
        };
        let icon = icon.map(|icon: Icon| {
            svg()
                .path(icon.path())
                .size(px(14.))
                .flex_none()
                .text_color(ui.icon)
        });
        let shortcut =
            shortcut.map(|shortcut| div().ml_auto().text_color(ui.muted_text).child(shortcut));
        div()
            .flex()
            .flex_1()
            .items_center()
            .gap_2()
            .children(icon)
            .child(label)
            .children(shortcut)
            .into_any_element()
    }

    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let Some(&(row, _)) = self.matches.get(ix) else {
            return false;
        };
        match self.list {
            List::Entries { .. } => self.pick_entry(row, window, cx),
            List::Symbols { .. } => self.pick_symbol(row, window, cx),
            List::Commands(_) => self.pick_command(row, window, cx),
        }
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.read = None;
        self.restore_focus(window, cx);
    }

    /// The directory listed, relative to the root, or why it couldn't be
    /// read; "No symbols" for a crumb no longer in the outline.
    fn status(&self) -> Option<SharedString> {
        match &self.list {
            List::Entries { dir, entries } => {
                if let Some(Err(error)) = entries {
                    return Some(format!("Cannot read {}: {error}", dir.display()).into());
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
            List::Symbols { symbols, .. } if symbols.is_empty() => Some("No symbols".into()),
            List::Symbols { .. } | List::Commands(_) => None,
        }
    }
}
