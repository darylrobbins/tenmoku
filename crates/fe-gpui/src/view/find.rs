//! In-file find: the pattern a host's find bar sets, highlighted in the
//! editor ([`crate::element`] paints it), stepped through, and counted off
//! the main thread.

use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::{Text, Version};
use fe_vim::search::{Pattern, matches_in};
use gpui::{AppContext as _, Context, Task};

use crate::EditorView;

/// How many rows a count searches between checks for having gone stale.
const COUNT_CHUNK_ROWS: u32 = 4_096;

/// The most matches a count keeps.
pub(crate) const MAX_COUNTED: usize = 10_000;

/// A view's find state: see [`EditorView::set_find`].
#[derive(Default)]
pub(crate) struct Find {
    pattern: Option<Pattern>,
    /// The last count to land, of the text at its version.
    counted: Option<Counted>,
    /// The count running, if any.
    counting: Option<Counting>,
}

struct Counted {
    version: Version,
    ranges: Vec<Range<usize>>,
    capped: bool,
}

struct Counting {
    /// The buffer version it counts.
    version: Version,
    /// Set to stop it at its next chunk.
    stop: Arc<AtomicBool>,
    task: Task<()>,
}

impl Find {
    /// Stops the count running, if any: it lands nothing.
    fn stop(&mut self) {
        if let Some(counting) = self.counting.take() {
            counting.stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for Find {
    fn drop(&mut self) {
        self.stop();
    }
}

/// How many matches the find pattern has: see [`EditorView::find_count`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindCount {
    /// The matches counted: all of them, or the first 10,000 if `capped`.
    pub total: usize,
    /// There are more than 10,000, which weren't counted.
    pub capped: bool,
    /// Which of the counted matches, from 1, is selected: see
    /// [`EditorView::find_count`].
    pub current: Option<usize>,
}

impl EditorView {
    /// Sets the pattern to highlight, step through and count, or with `None`
    /// clears it: then nothing is found, painted or counted. While it's set
    /// it's painted instead of vim's search highlight.
    pub fn set_find(&mut self, pattern: Option<Pattern>, cx: &mut Context<Self>) {
        self.find.stop();
        self.find.counted = None;
        self.find.pattern = pattern;
        self.sync_find(cx);
        cx.notify();
    }

    /// Whether a count is running.
    #[cfg(test)]
    pub(crate) fn is_counting(&self) -> bool {
        self.find.counting.is_some()
    }

    /// The pattern [`EditorView::set_find`] set, if any.
    pub(crate) fn find_pattern(&self) -> Option<&Pattern> {
        self.find.pattern.as_ref()
    }

    /// Selects the next match after the primary selection's start, wrapping
    /// past the end, so a selected match is stepped past, and reveals it.
    /// With vim on the caret goes to its start, as `/` leaves it. Returns
    /// `false`, changing nothing, if there's no match or no pattern.
    pub fn find_next(&mut self, cx: &mut Context<Self>) -> bool {
        self.step_find(true, cx)
    }

    /// [`EditorView::find_next`] going back: the last match before the
    /// primary selection's start, wrapping past the start.
    pub fn find_previous(&mut self, cx: &mut Context<Self>) -> bool {
        self.step_find(false, cx)
    }

    /// Selects the first match starting at or after `offset`, wrapping past
    /// the end, as a find bar does while its query is typed, and reveals it
    /// as [`EditorView::find_next`] does.
    pub fn find_from(&mut self, offset: usize, cx: &mut Context<Self>) -> bool {
        let Some(pattern) = &self.find.pattern else {
            return false;
        };
        let Some(found) = pattern.find_from(self.model.text(), offset) else {
            return false;
        };
        self.select_match(found, cx);
        true
    }

    fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let Some(pattern) = &self.find.pattern else {
            return false;
        };
        let from = self.model.primary_selection().range.start;
        let Some(found) = pattern.find(self.model.text(), from, forward) else {
            return false;
        };
        self.select_match(found, cx);
        true
    }

    /// Selects `found` and reveals it; with vim on, puts the caret at its
    /// start instead.
    fn select_match(&mut self, found: Range<usize>, cx: &mut Context<Self>) {
        self.sync_vim(cx);
        let range = match self.vim_mode() {
            Some(_) => found.start..found.start,
            None => found,
        };
        self.select_and_reveal(range, cx);
    }

    /// How many matches the find pattern has in the text as it is now, and
    /// which is selected: the one equal to the primary selection, or with
    /// vim on the one starting at the caret. `None` with no pattern, or
    /// while the matches are counted in the background after
    /// [`EditorView::set_find`] or an edit; the view notifies when the
    /// count lands.
    pub fn find_count(&self) -> Option<FindCount> {
        self.find.pattern.as_ref()?;
        let counted = self
            .find
            .counted
            .as_ref()
            .filter(|counted| counted.version == self.model.snapshot().version())?;
        let selection = self.model.primary_selection();
        let starting_at = |start: usize| {
            counted
                .ranges
                .binary_search_by_key(&start, |range| range.start)
                .ok()
        };
        let current = match self.vim {
            Some(_) => starting_at(selection.head()),
            None => starting_at(selection.range.start)
                .filter(|&ix| counted.ranges[ix] == selection.range),
        };
        Some(FindCount {
            total: counted.ranges.len(),
            capped: counted.capped,
            current: current.map(|ix| ix + 1),
        })
    }

    /// Starts counting the find pattern's matches in the background, unless
    /// a count of the text as it is now has landed or is running. A count of
    /// an older version stops at its next chunk and lands nothing. Each
    /// edit command and each frame do it, so an edit through
    /// [`EditorView::model_mut`] starts a count by the next frame.
    pub(crate) fn sync_find(&mut self, cx: &mut Context<Self>) {
        let Some(pattern) = self.find.pattern.clone() else {
            return;
        };
        let version = self.model.snapshot().version();
        let known = match &self.find.counting {
            Some(counting) => Some(counting.version),
            None => self.find.counted.as_ref().map(|counted| counted.version),
        };
        if known == Some(version) {
            return;
        }
        self.find.stop();
        self.find.counted = None;
        let stop = Arc::new(AtomicBool::new(false));
        let text = self.model.text().clone();
        let task = cx.spawn({
            let stop = stop.clone();
            async move |this, cx| {
                let counted = cx
                    .background_spawn(async move { count(&text, &pattern, &stop) })
                    .await;
                this.update(cx, |view, cx| {
                    // This task; dropping it here would cancel it mid-update.
                    if let Some(counting) = view.find.counting.take() {
                        counting.task.detach();
                    }
                    if let Some((ranges, capped)) = counted
                        && view.model.snapshot().version() == version
                    {
                        view.find.counted = Some(Counted {
                            version,
                            ranges,
                            capped,
                        });
                    }
                    // Stale, edited since it started: count again.
                    view.sync_find(cx);
                    cx.notify();
                })
                .ok();
            }
        });
        self.find.counting = Some(Counting {
            version,
            stop,
            task,
        });
    }
}

/// `pattern`'s matches in `text`, at most [`MAX_COUNTED`] of them, and
/// whether there are more; `None` if `stop` is set between chunks.
fn count(text: &Text, pattern: &Pattern, stop: &AtomicBool) -> Option<(Vec<Range<usize>>, bool)> {
    let rows = text.row_count();
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < rows {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        let end = start.saturating_add(COUNT_CHUNK_ROWS).min(rows);
        // One past the cap says whether there are more.
        let room = MAX_COUNTED + 1 - ranges.len();
        ranges.extend(matches_in(text, start..end, pattern, room));
        if ranges.len() > MAX_COUNTED {
            ranges.truncate(MAX_COUNTED);
            return Some((ranges, true));
        }
        start = end;
    }
    Some((ranges, false))
}
