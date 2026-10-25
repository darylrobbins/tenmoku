//! The outline's keys: `outline.show` focuses the panel with the symbol
//! containing the active editor's caret selected (the first row if none
//! does), and the panel's commands move the selection and jump to it, as a
//! click on its row does. Up, down and Enter always; with vim on, the
//! panel's key context has the `vim` flag, for `k`, `j`, `gg`, `G` and `o`,
//! and a count typed first (`3j`, `5G`) moves that many rows, or to that
//! row.
//!
//! The selection is a row of the outline shown: dropped with it when the
//! panel follows another editor, so a key pressed then doesn't land in
//! either, and kept on the same symbol when the outline is found again,
//! its position mapped through the edits made since.

use fe_buffer::{Bias, BufferSnapshot, Version};
use fe_syntax::{Outline, OutlineItem};
use gpui::{App, Context, KeyContext, KeyDownEvent, ScrollStrategy, WeakEntity, Window};
use kiln::Cx;

use super::OutlinePanel;

/// The key context the panel's element sets.
pub(crate) const KEY_CONTEXT: &str = "outline_panel";

/// The flag the panel's key context has while vim is on.
const VIM_FLAG: &str = "vim";

/// The highest count the panel takes: more rows than an outline has.
const MAX_COUNT: usize = 1_000_000;

/// The selected row, and what moves it.
#[derive(Default)]
pub(super) struct Selection {
    /// The row selected, of the outline shown.
    pub(super) row: Option<usize>,
    /// A vim count typed before the next command.
    count: Option<usize>,
    /// Whether `outline.show` asked for the caret's symbol and no outline
    /// of the text as it was then has shown: it's selected again as each
    /// lands, until a key moves the selection.
    follow_caret: bool,
}

#[kiln::command(context = "outline_panel")]
impl OutlinePanel {
    /// Select Previous Symbol
    #[command(id = "outline.select_previous")]
    fn select_previous_symbol(&mut self, cx: &mut Cx<Self>) {
        self.move_selection(-1, cx.cx);
    }

    /// Select Next Symbol
    #[command(id = "outline.select_next")]
    fn select_next_symbol(&mut self, cx: &mut Cx<Self>) {
        self.move_selection(1, cx.cx);
    }

    /// Select First Symbol
    ///
    /// With a count, selects that row, as vim's `gg` goes to that line.
    #[command(id = "outline.select_first")]
    fn select_first_symbol(&mut self, cx: &mut Cx<Self>) {
        let row = self.selection.count.take().map_or(0, |count| count - 1);
        self.select_clamped(row, cx.cx);
    }

    /// Select Last Symbol
    ///
    /// With a count, selects that row, as vim's `G` goes to that line.
    #[command(id = "outline.select_last")]
    fn select_last_symbol(&mut self, cx: &mut Cx<Self>) {
        let row = (self.selection.count.take()).map_or(usize::MAX, |count| count - 1);
        self.select_clamped(row, cx.cx);
    }

    /// Go to Selected Symbol
    ///
    /// Puts the editor's caret on the selected symbol's name and focuses it.
    #[command(id = "outline.open_selected")]
    fn open_selected_symbol(&mut self, cx: &mut Cx<Self>) {
        self.selection.count = None;
        if let Some(row) = self.selection.row {
            let generation = self.generation;
            self.jump(generation, row, cx.window, cx.cx);
        }
    }
}

impl OutlinePanel {
    /// `outline.show`'s part: focuses the panel and selects the symbol
    /// containing the editor's caret, else the first row; once an outline of
    /// the editor's text as it is lands, if the one shown isn't.
    pub(crate) fn focus_from_show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selection.count = None;
        self.selection.follow_caret = true;
        self.follow_caret(cx);
    }

    /// Selects the symbol containing the editor's caret, else the first
    /// row, if an outline shows. The caret is an offset in the text as it
    /// is: until the outline shown is of that version, it may point at
    /// another symbol, so the request stays and is answered again as each
    /// outline lands.
    fn follow_caret(&mut self, cx: &mut Context<Self>) {
        if self.items().is_empty() {
            return;
        }
        let Some(editor) = self.editor.as_ref().and_then(WeakEntity::upgrade) else {
            return;
        };
        let model = editor.read(cx).model();
        let (caret, version) = (model.primary_selection().head(), model.snapshot().version());
        let Some((outline, _)) = &self.outline else {
            return;
        };
        let current = outline.version == version;
        let row = outline.item_at(caret).unwrap_or(0);
        self.show_row(row, cx);
        self.selection.follow_caret = !current;
    }

    /// Moves the selection `delta` rows times the count, stopping at the
    /// first and the last; with none selected, from before the first, or
    /// past the last.
    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let steps = self.selection.count.take().unwrap_or(1) as isize * delta;
        let len = self.items().len() as isize;
        let from = match self.selection.row {
            Some(row) => row as isize,
            None if delta > 0 => -1,
            None => len,
        };
        self.select_clamped((from + steps).max(0) as usize, cx);
    }

    /// Selects row `row`, or the last if there are fewer, if any show.
    fn select_clamped(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(last) = self.items().len().checked_sub(1) else {
            return;
        };
        self.select(row.min(last), cx);
    }

    /// Selects row `row` for a key, which answers any request for the
    /// caret's symbol.
    fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        self.selection.follow_caret = false;
        self.show_row(row, cx);
    }

    fn show_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.selection.row = Some(row);
        self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Drops the selection, and a request for the caret's symbol, as the
    /// panel follows another editor.
    pub(super) fn clear_selection(&mut self) {
        self.selection = Selection::default();
    }

    /// Keeps the selection on its symbol now `old` is replaced by another
    /// outline of the same editor: the item of the same name, kind and depth
    /// whose name starts nearest where the selected one's does, both mapped
    /// through the edits since to the text as it is; else the row it was on,
    /// or the last. A request for the caret's symbol is answered instead.
    pub(super) fn keep_selection(&mut self, old: Option<&Outline>, cx: &mut Context<Self>) {
        if self.selection.follow_caret {
            return self.follow_caret(cx);
        }
        let Some(row) = self.selection.row else {
            return;
        };
        let Some(last) = self.items().len().checked_sub(1) else {
            self.selection.row = None;
            return;
        };
        let editor = self.editor.as_ref().and_then(WeakEntity::upgrade);
        let same = (old.and_then(|old| Some((old, old.items.get(row)?))))
            .zip(editor)
            .and_then(|((old, was), editor)| {
                let snapshot = editor.read(cx).model().snapshot();
                self.same_symbol(was, old.version, snapshot)
            });
        self.selection.row = Some(same.unwrap_or(row.min(last)));
    }

    /// The row of the outline shown that is `was`, an item of an outline of
    /// version `from`: of its name, kind and depth, its name starting
    /// nearest where `was`'s does in `snapshot`. None if there's none, or if
    /// either outline is older than `snapshot` maps from.
    fn same_symbol(
        &self,
        was: &OutlineItem,
        from: Version,
        snapshot: &BufferSnapshot,
    ) -> Option<usize> {
        let (outline, _) = self.outline.as_ref()?;
        let floor = snapshot.oldest_version();
        if from < floor || outline.version < floor {
            return None;
        }
        let at = snapshot.map_offset(was.name_range.start, Bias::Left, from);
        (self.items().iter().enumerate())
            .filter(|(_, item)| {
                item.name == was.name && item.kind == was.kind && item.depth == was.depth
            })
            .min_by_key(|(_, item)| {
                let start = item.name_range.start;
                snapshot
                    .map_offset(start, Bias::Left, outline.version)
                    .abs_diff(at)
            })
            .map(|(ix, _)| ix)
    }

    /// A digit typed with vim on, unbound, adds to the count; any other
    /// key that reaches the panel unbound drops it. A leading `0` isn't a
    /// count.
    pub(super) fn count_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if !fe_gpui::vim_enabled(cx) {
            return;
        }
        let keystroke = &event.keystroke;
        let digit = (keystroke.modifiers.number_of_modifiers() == 0)
            .then(|| keystroke.key.parse::<u8>().ok().filter(|digit| *digit <= 9))
            .flatten()
            .filter(|digit| *digit > 0 || self.selection.count.is_some());
        match digit {
            Some(digit) => {
                let count = self.selection.count.unwrap_or(0) * 10 + usize::from(digit);
                self.selection.count = Some(count.min(MAX_COUNT));
                cx.stop_propagation();
            }
            None => self.selection.count = None,
        }
    }

    /// Its element's key context: [`KEY_CONTEXT`], with the `vim` flag
    /// while vim is on.
    pub(super) fn key_context(&self, cx: &App) -> KeyContext {
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        if fe_gpui::vim_enabled(cx) {
            context.add(VIM_FLAG);
        }
        context
    }
}

#[cfg(test)]
impl OutlinePanel {
    /// The row selected.
    pub(crate) fn selected(&self) -> Option<usize> {
        self.selection.row
    }

    /// The vim count typed so far.
    pub(crate) fn count(&self) -> Option<usize> {
        self.selection.count
    }
}
