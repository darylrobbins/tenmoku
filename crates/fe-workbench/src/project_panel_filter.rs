//! Filtering the project tree by name, vim's `/` in the tree as
//! neo-tree's `/` and nvim-tree's live filter `f` do (shift-f clears it,
//! nvim-tree's `F`).
//!
//! A box above the rows takes typing, and on each key the list shows only
//! the rows whose name holds what's typed, ignoring case, with the folders
//! above them, which it expands. Only listed folders are searched: one
//! never expanded isn't listed to look in it, as the no-match row says.
//! Enter, or the box losing focus, keeps the filter and gives the keys
//! back to the tree, whose j and k (and gg and G) then step through the
//! matching rows only; the tree's other keys act on the selected row as
//! they do unfiltered. Escape in the box, or shift-f, clears the filter,
//! restoring the folders expanded and the row selected before it. Opening
//! the tree again (`H`) clears it too.
//!
//! The box is the name field's kiln-ui [`TextField`], in the same
//! `text_input` key context, and the panel drops its `vim` flag while the
//! box has focus, so vim's tree keys don't take what's typed.
//!
//! The filter is a view over the tree's rows: tree row indices stay what
//! the panel's selection and operations use, and the list's positions are
//! mapped to them ([`ProjectPanel::list_pos`]).

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::OsStr,
    ops::Range,
    rc::Rc,
};

use fe_gpui::commands::RunCommand;
use fe_project::{EntryKind, FileTree, NodeId};
use fe_ui::{TextField, TextFieldEvent, text_field};
use gpui::{
    AnyElement, AppContext as _, Context, Entity, EntityId, Focusable as _,
    InteractiveElement as _, IntoElement as _, KeyDownEvent, ParentElement as _, ScrollStrategy,
    Styled as _, Subscription, Window, div, px,
};

use super::{PanelOp, ProjectPanel, ROW_HEIGHT, State, op};
use crate::picker::{self, PaletteOp};

/// The key context the box sits in, the name field's, for the
/// `text_input.*` commands' keys.
const FIELD_KEY_CONTEXT: &str = "text_input";

/// The row the list shows when nothing matches.
pub(super) const NO_MATCHES: &str = "No matches in listed folders";

/// The filter, set.
pub(super) struct Filter {
    /// What's typed.
    query: String,
    /// `query` in lower case, which names are matched against.
    lower: String,
    /// The box, while it takes typing.
    typing: Option<Typing>,
    /// The folders expanded before the filter, to expand again once it's
    /// cleared.
    before_expanded: HashSet<NodeId>,
    /// The folders waiting for a listing to expand before the filter: any
    /// queued since are dropped once it's cleared.
    before_expanding: HashSet<NodeId>,
    /// The row selected before the filter, to select again once it's
    /// cleared.
    before_selected: Option<NodeId>,
    /// The folders the filter expanded to show matches below them.
    opened: HashSet<NodeId>,
    /// The matches whose folders it expanded: a match the tree gains (a
    /// watcher batch, a listing) has its folders expanded too.
    seen: HashSet<NodeId>,
    /// The name of a row selected that the filter hides (a reveal's, or a
    /// rename's), which the line above the rows mentions.
    hidden: Option<String>,
    /// The rows shown, as of a tree version.
    view: RefCell<Option<(u64, Rc<[Shown]>)>>,
}

/// The filter's box, open.
struct Typing {
    input: Entity<TextField>,
    _subscriptions: [Subscription; 2],
}

/// A tree row the filter shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Shown {
    /// Its index among the tree's rows.
    pub(super) ix: usize,
    /// Whether its name matches, rather than a row below it.
    pub(super) matched: bool,
}

/// How a node under the root bears on the filter: whether its name
/// matches, whether one below it does, and whether one below it does
/// that isn't in [`Filter::seen`].
#[derive(Clone, Copy)]
struct Kept {
    matched: bool,
    below: bool,
    below_new: bool,
}

impl ProjectPanel {
    /// `/`: opens the filter's box, focused, with the filter's text if one
    /// is kept, noting the folders expanded and the row selected to restore
    /// once it's cleared.
    pub(super) fn start_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tree().is_none() {
            return;
        }
        let filter = self.filter.get_or_insert_with(|| Filter {
            query: String::new(),
            lower: String::new(),
            typing: None,
            before_expanded: self.expanded.clone(),
            before_expanding: self.expanding.clone(),
            before_selected: self.selected,
            opened: HashSet::new(),
            seen: HashSet::new(),
            hidden: None,
            view: RefCell::new(None),
        });
        let query = filter.query.clone();
        let input = cx.new(|cx| {
            TextField::new(window, cx)
                .placeholder("Filter by name")
                .text(&query)
        });
        let focus = input.focus_handle(cx);
        let changed = cx.subscribe_in(&input, window, |panel, _, event, window, cx| match event {
            TextFieldEvent::Changed => panel.filter_changed(cx),
            TextFieldEvent::Submitted => panel.keep_filter(window, cx),
            TextFieldEvent::Focused | TextFieldEvent::Blurred => {}
        });
        let id = input.entity_id();
        let blur = cx.on_blur(&focus, window, move |panel, window, cx| {
            if panel.filter_box() == Some(id) {
                panel.keep_filter(window, cx);
            }
        });
        filter.typing = Some(Typing {
            input,
            _subscriptions: [changed, blur],
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    /// The filter's box's entity, while it's open.
    fn filter_box(&self) -> Option<EntityId> {
        let typing = self.filter.as_ref()?.typing.as_ref()?;
        Some(typing.input.entity_id())
    }

    /// Whether the filter's box is open, taking typing.
    pub(super) fn filter_typing(&self) -> bool {
        self.filter_box().is_some()
    }

    /// Whether the filter's box has focus.
    pub(super) fn filter_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        let typing = self
            .filter
            .as_ref()
            .and_then(|filter| filter.typing.as_ref());
        typing.is_some_and(|typing| typing.input.focus_handle(cx).is_focused(window))
    }

    /// Enter in the box, or the box losing focus: closes the box, keeping
    /// the filter, and gives the tree its keys back. An empty filter is
    /// cleared instead.
    fn keep_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(filter) = self.filter.as_mut() else {
            return;
        };
        if filter.query.is_empty() {
            return self.clear_filter(window, cx);
        }
        let focused = self.filter_focused(window, cx);
        if let Some(filter) = self.filter.as_mut() {
            filter.typing = None;
        }
        if focused {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// Escape in the box, or shift-f: clears the filter, collapsing the
    /// folders it expanded (and any expanded since, or waiting for a
    /// listing to) and expanding again those collapsed since, then
    /// selecting the row selected before it: none if none was, and the row
    /// selected now if it went.
    pub(super) fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.filter_focused(window, cx);
        let Some(filter) = self.filter.take() else {
            return;
        };
        if focused {
            window.focus(&self.focus, cx);
        }
        let State::Open(tree) = &mut self.state else {
            return cx.notify();
        };
        // An expand all under way would go on expanding past what's
        // restored.
        self.expansion = None;
        let root = tree.root();
        for dir in self.expanded.clone() {
            if dir != root && !filter.before_expanded.contains(&dir) {
                tree.collapse(dir);
                self.expanded.remove(&dir);
            }
        }
        self.expanding
            .retain(|dir| filter.before_expanding.contains(dir));
        for &dir in &filter.before_expanded {
            if !self.expanded.contains(&dir) && tree.row(dir).is_some() {
                self.expanding.insert(dir);
            }
        }
        // Those listed expand here; the rest once listed.
        self.reconcile(cx);
        match filter.before_selected {
            None => self.selected = None,
            Some(id) if self.tree().is_some_and(|tree| tree.row(id).is_some()) => {
                self.selected = Some(id)
            }
            Some(_) => {}
        }
        if let Some(ix) = self.selected_index() {
            self.select_row(ix, cx);
        }
        cx.notify();
    }

    /// Takes the box's text in: expands the folders above each match that
    /// aren't, collapses those the filter expanded for a match that's gone,
    /// and selects the first match unless a match is selected.
    fn filter_changed(&mut self, cx: &mut Context<Self>) {
        let Some(filter) = self.filter.as_mut() else {
            return;
        };
        let Some(typing) = filter.typing.as_ref() else {
            return;
        };
        let query = typing.input.read(cx).value().to_owned();
        if query == filter.query {
            return;
        }
        filter.lower = query.to_lowercase();
        filter.query = query;
        filter.view.replace(None);
        filter.seen.clear();
        filter.hidden = None;
        self.open_matches(true, cx);
        if let Some(view) = self.filtered() {
            let selected = self.selected_index();
            let on_match = (view.iter()).any(|shown| shown.matched && Some(shown.ix) == selected);
            let first = view.iter().find(|shown| shown.matched);
            if let Some(first) = first.filter(|_| !on_match) {
                self.select_row(first.ix, cx);
            }
        }
        cx.notify();
    }

    /// Expands the folders above the matches the tree gained since the
    /// filter last looked (a watcher batch, a listing): only those, so a
    /// folder collapsed since stays so unless a new match is under it.
    /// Unlisted folders still aren't listed to search them.
    pub(super) fn refilter(&mut self, cx: &mut Context<Self>) {
        if self.filtered().is_some() {
            self.open_matches(false, cx);
        }
    }

    /// Expands the folders above the matches, each one that isn't: above
    /// every match if `all` (the text changed), collapsing those the
    /// filter expanded that lead to none now; otherwise above those not
    /// seen before.
    fn open_matches(&mut self, all: bool, cx: &mut Context<Self>) {
        let Some(filter) = self.filter.as_mut() else {
            return;
        };
        let State::Open(tree) = &mut self.state else {
            return;
        };
        let mut kept = HashMap::new();
        if !filter.lower.is_empty() {
            search(tree, tree.root(), &filter.lower, &filter.seen, &mut kept);
        }
        if all {
            let leads = |dir: &NodeId| kept.get(dir).is_some_and(|kept: &Kept| kept.below);
            for dir in std::mem::take(&mut filter.opened) {
                if leads(&dir) {
                    filter.opened.insert(dir);
                } else {
                    tree.collapse(dir);
                    self.expanded.remove(&dir);
                    self.expanding.remove(&dir);
                }
            }
        }
        let mut queued = false;
        for (&dir, kept) in &kept {
            if !(kept.below_new || all && kept.below) {
                continue;
            }
            let shown = tree.row(dir).is_some_and(|row| row.expanded);
            if !shown && self.expanding.insert(dir) {
                filter.opened.insert(dir);
                queued = true;
            }
        }
        filter.seen = (kept.iter())
            .filter(|(_, kept)| kept.matched)
            .map(|(&id, _)| id)
            .collect();
        if queued {
            // Those listed expand here; the rest once listed.
            self.reconcile(cx);
            cx.notify();
        }
    }

    /// Notes that a reveal selected `id`, which the filter hides: it's
    /// selected, but nothing acts on it while it's hidden.
    pub(super) fn revealed_hidden(&mut self, id: NodeId) {
        let name = (self.tree().and_then(|tree| tree.row(id)))
            .map(|row| row.name.to_string_lossy().into_owned());
        if let Some(filter) = self.filter.as_mut() {
            filter.hidden = name;
        }
    }

    /// Runs a command while the filter's box has focus, as the name field
    /// does ([`ProjectPanel::run_in_field`]): Enter keeps the filter, the
    /// `text_input` edits and the panel's left and right edit the box and
    /// move its caret, the panel's cut, copy and paste are the box's, and
    /// the panel's other keys do nothing. Others go on to the workbench.
    pub(super) fn run_in_filter(
        &mut self,
        run: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if run.0.command.as_str() == "text_input.submit" {
            return self.keep_filter(window, cx);
        }
        let action: Box<dyn gpui::Action> = match op(run, cx) {
            Some(PanelOp::Open) => return self.keep_filter(window, cx),
            Some(PanelOp::CollapseOrParent) => Box::new(text_field::Left),
            Some(PanelOp::ExpandOrFirstChild) => Box::new(text_field::Right),
            Some(PanelOp::Cut) => Box::new(text_field::Cut),
            Some(PanelOp::Copy) => Box::new(text_field::Copy),
            Some(PanelOp::Paste) => Box::new(text_field::Paste),
            Some(_) => return,
            None => match picker::op(run, cx) {
                Some(PaletteOp::Backspace) => Box::new(text_field::Backspace),
                Some(PaletteOp::DeleteWordLeft) => Box::new(text_field::DeleteWordLeft),
                Some(PaletteOp::Paste) => Box::new(text_field::Paste),
                Some(PaletteOp::Clear) => {
                    let typing = self
                        .filter
                        .as_ref()
                        .and_then(|filter| filter.typing.as_ref());
                    if let Some(typing) = typing {
                        typing.input.update(cx, |input, cx| input.set_value("", cx));
                    }
                    return self.filter_changed(cx);
                }
                _ => return cx.propagate(),
            },
        };
        // To the box, which has focus.
        window.dispatch_action(action, cx);
    }

    /// Escape in the box clears the filter.
    fn on_filter_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            self.clear_filter(window, cx);
            cx.stop_propagation();
        }
    }

    /// The tree rows the filter shows, in order, while it's set to some
    /// text; `None` while every row shows.
    pub(super) fn filtered(&self) -> Option<Rc<[Shown]>> {
        let filter = self
            .filter
            .as_ref()
            .filter(|filter| !filter.lower.is_empty())?;
        let tree = self.tree()?;
        let mut view = filter.view.borrow_mut();
        if let Some((version, shown)) = view.as_ref()
            && *version == tree.version()
        {
            return Some(shown.clone());
        }
        let shown: Rc<[Shown]> = shown_rows(tree, &filter.lower).into();
        *view = Some((tree.version(), shown.clone()));
        Some(shown)
    }

    /// Whether the list shows tree row `ix`: unless the filter hides it.
    pub(super) fn shows(&self, ix: usize) -> bool {
        self.filtered()
            .is_none_or(|view| view.binary_search_by_key(&ix, |shown| shown.ix).is_ok())
    }

    /// The number of rows the list shows, not counting a new entry's field.
    pub(super) fn list_count(&self) -> usize {
        match self.filtered() {
            Some(view) => view.len(),
            None => self.tree().map_or(0, FileTree::row_count),
        }
    }

    /// Where a new entry's field sits in the list, and its depth, as
    /// [`ProjectPanel::field_row`], under its folder's row in a filtered
    /// list.
    pub(super) fn list_field(&self) -> Option<(usize, usize)> {
        let (at, depth) = self.field_row()?;
        let Some(view) = self.filtered() else {
            return Some((at, depth));
        };
        if at == 0 {
            return Some((0, depth));
        }
        // The folder's row is just above it.
        let folder = at - 1;
        let pos = match view.binary_search_by_key(&folder, |shown| shown.ix) {
            Ok(pos) => pos + 1,
            Err(pos) => pos,
        };
        Some((pos, depth))
    }

    /// Where tree row `ix` is in the list: where it would be, if the
    /// filter hides it.
    pub(super) fn list_pos(&self, ix: usize) -> usize {
        let Some(view) = self.filtered() else {
            return self.list_ix(ix);
        };
        let pos = view
            .binary_search_by_key(&ix, |shown| shown.ix)
            .unwrap_or_else(|pos| pos);
        match self.list_field() {
            Some((at, _)) if pos >= at => pos + 1,
            _ => pos,
        }
    }

    /// The tree rows of the list's rows in `range`, which holds no new
    /// entry's field.
    pub(super) fn list_rows(&self, range: Range<usize>) -> Vec<usize> {
        match self.filtered() {
            Some(view) => {
                let end = range.end.min(view.len());
                let start = range.start.min(end);
                view[start..end].iter().map(|shown| shown.ix).collect()
            }
            None => range.collect(),
        }
    }

    /// The matching row `delta` matches on from the selected row, j and k
    /// in a filtered list: the next or the previous, or, past the last or
    /// the first, the last or the first (unless that's selected); the first
    /// with none selected. `None` unless the list is filtered.
    pub(super) fn step_match(&self, delta: isize) -> Option<Option<usize>> {
        let view = self.filtered()?;
        let mut matches = view
            .iter()
            .filter(|shown| shown.matched)
            .map(|shown| shown.ix);
        let Some(at) = self.selected_index() else {
            return Some(matches.next());
        };
        let matches: Vec<usize> = matches.collect();
        let pos = matches.partition_point(|&ix| ix < at);
        let on = matches.get(pos) == Some(&at);
        let target = if delta > 0 {
            let next = if on { pos + 1 } else { pos };
            matches.get(next).or(matches.last().filter(|_| !on))
        } else {
            let prev = pos.checked_sub(1);
            prev.and_then(|prev| matches.get(prev))
                .or(matches.first().filter(|_| !on))
        };
        Some(target.copied())
    }

    /// The first matching row, or the last, gg and G in a filtered list:
    /// `None` unless the list is filtered.
    pub(super) fn end_match(&self, last: bool) -> Option<Option<usize>> {
        let view = self.filtered()?;
        let mut matches = view.iter().filter(|shown| shown.matched);
        let found = if last {
            matches.next_back()
        } else {
            matches.next()
        };
        Some(found.map(|shown| shown.ix))
    }

    /// The tree row the list shows after tree row `ix`, if any.
    pub(super) fn next_shown(&self, ix: usize) -> Option<usize> {
        let Some(view) = self.filtered() else {
            let count = self.tree()?.row_count();
            return (ix + 1 < count).then_some(ix + 1);
        };
        let pos = view.partition_point(|shown| shown.ix <= ix);
        view.get(pos).map(|shown| shown.ix)
    }

    /// The row to select in place of tree row `ix`, whose node went: in a
    /// filtered list the first match from it on, or else the last before
    /// it, and none with no match left; `ix` unfiltered.
    pub(super) fn nearest_match(&self, ix: usize) -> Option<usize> {
        let Some(view) = self.filtered() else {
            return Some(ix);
        };
        let mut matches = view
            .iter()
            .filter(|shown| shown.matched)
            .map(|shown| shown.ix);
        let after = matches.clone().find(|&shown| shown >= ix);
        after.or(matches.rfind(|&shown| shown < ix))
    }

    /// Scrolls the list to a new entry's field, or the selected row, as
    /// the name field opens in a filtered list.
    pub(super) fn scroll_to_field(&self) {
        if self.filtered().is_none() {
            return;
        }
        let at = match self.list_field() {
            Some((at, _)) => Some(at),
            None => self.selected_index().map(|ix| self.list_pos(ix)),
        };
        if let Some(at) = at {
            self.scroll.scroll_to_item(at, ScrollStrategy::Nearest);
        }
    }

    /// What goes above the rows while the filter's set: its box while it
    /// takes typing, and otherwise a line saying what it's set to.
    pub(super) fn render_filter(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let filter = self.filter.as_ref()?;
        let Some(typing) = &filter.typing else {
            let text = match &filter.hidden {
                Some(name) => format!("Filter: {} (hides “{name}”)", filter.query),
                None => format!("Filter: {}", filter.query),
            };
            return Some(self.message(text, cx));
        };
        Some(
            div()
                .id("filter-box")
                .flex_none()
                .h(ROW_HEIGHT)
                .mx(px(6.))
                .mb_1()
                .flex()
                .items_center()
                .on_key_down(cx.listener(Self::on_filter_key))
                .child(
                    div()
                        .key_context(FIELD_KEY_CONTEXT)
                        .flex_1()
                        .min_w_0()
                        .h(ROW_HEIGHT - px(2.))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .child(typing.input.clone()),
                )
                .into_any_element(),
        )
    }
}

/// Whether `name` holds `lower`, ignoring case.
fn matches(name: &OsStr, lower: &str) -> bool {
    name.to_string_lossy().to_lowercase().contains(lower)
}

/// Notes in `kept` each node under listed directory `dir` whose name holds
/// `lower` or with one below it that does, looking only in listed
/// directories. Whether there's any, and any not in `seen`.
fn search(
    tree: &FileTree,
    dir: NodeId,
    lower: &str,
    seen: &HashSet<NodeId>,
    kept: &mut HashMap<NodeId, Kept>,
) -> (bool, bool) {
    let Some(children) = tree.children(dir) else {
        return (false, false);
    };
    let (mut any, mut any_new) = (false, false);
    for &child in children {
        let Some(row) = tree.row(child) else {
            continue;
        };
        let matched = matches(row.name, lower);
        let (below, below_new) = match row.kind {
            EntryKind::Dir => search(tree, child, lower, seen, kept),
            EntryKind::File | EntryKind::Symlink => (false, false),
        };
        if matched || below {
            kept.insert(
                child,
                Kept {
                    matched,
                    below,
                    below_new,
                },
            );
            any = true;
            any_new |= below_new || matched && !seen.contains(&child);
        }
    }
    (any, any_new)
}

/// The tree's rows that match `lower` or have a match below them.
fn shown_rows(tree: &FileTree, lower: &str) -> Vec<Shown> {
    let mut kept = HashMap::new();
    search(tree, tree.root(), lower, &HashSet::new(), &mut kept);
    (tree.rows(0..tree.row_count()).enumerate())
        .filter_map(|(ix, row)| {
            let kept = kept.get(&row.id)?;
            Some(Shown {
                ix,
                matched: kept.matched,
            })
        })
        .collect()
}

#[cfg(test)]
impl ProjectPanel {
    /// The filter's text, while it's set.
    pub(crate) fn filter_text(&self) -> Option<&str> {
        Some(self.filter.as_ref()?.query.as_str())
    }

    /// The rows the list shows, as `(depth, name)`, a new entry's field
    /// not counted.
    pub(crate) fn list_names(&self) -> Vec<(usize, String)> {
        let Some(tree) = self.tree() else {
            return Vec::new();
        };
        let rows = self.list_rows(0..self.list_count());
        (rows.into_iter())
            .filter_map(|ix| tree.rows(ix..ix + 1).next())
            .map(|row| (row.depth, row.name.to_string_lossy().into_owned()))
            .collect()
    }

    /// The message the list shows in place of its rows, if any.
    pub(crate) fn list_message(&self) -> Option<&'static str> {
        (self.filtered().is_some_and(|view| view.is_empty()) && self.field_row().is_none())
            .then_some(NO_MATCHES)
    }

    /// The hidden row the line above the rows mentions, if any.
    pub(crate) fn filter_hides(&self) -> Option<&str> {
        self.filter.as_ref()?.hidden.as_deref()
    }

    /// Whether the filter's box has focus.
    pub(crate) fn filter_box_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.filter_focused(window, cx)
    }
}
