//! In-file find: the pattern a host's find bar sets, highlighted in the
//! editor ([`crate::element`] paints it), stepped through, and counted off
//! the main thread.
//!
//! A find can have a scope ([`EditorView::set_find_scoped`], the find bar's
//! Find in Selection): only matches wholly inside it are found, counted,
//! painted and marked on the minimap, and it's drawn as a faint band. They
//! are the row's matches filtered, searched from the row's start as ever, so
//! the options read the context they do without a scope, and a match that
//! starts before the scope and runs into it hides one starting inside it.

use std::{
    ops::{ControlFlow, Range},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::{Anchor, Text, Version};
use fe_vim::search::{Deadline, Interrupted, Pattern, matches_in_until, visit_reaching};
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
    /// What limits the find: see [`EditorView::find_scope`]. It stays with
    /// no pattern, so its band shows before there's a query.
    scope: Option<Vec<Range<Anchor>>>,
    /// The band's rects the last frame painted.
    #[cfg(test)]
    pub(crate) painted_scope: Vec<gpui::Bounds<gpui::Pixels>>,
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
        self.set_find_scoped(pattern, None, cx);
    }

    /// [`EditorView::set_find`], with only the matches wholly inside `scope`
    /// found, counted and painted, and `scope` drawn, with no pattern too.
    /// It comes from this view's [`EditorView::selection_scope`].
    pub fn set_find_scoped(
        &mut self,
        pattern: Option<Pattern>,
        scope: Option<Vec<Range<Anchor>>>,
        cx: &mut Context<Self>,
    ) {
        self.find.stop();
        self.find.counted = None;
        self.find.pattern = pattern;
        // What's searched changes, so a frame that ran out of time says
        // nothing about the next.
        if self.find.scope != scope
            && let Some(layout) = &mut self.layout
        {
            layout.forget_highlight_ran_out();
        }
        self.find.scope = scope;
        self.sync_find(cx);
        cx.notify();
    }

    /// The non-empty selections as a scope for
    /// [`EditorView::set_find_scoped`], anchored so that text inserted inside
    /// it or at its ends joins it, but for an end at a row's start, where
    /// what's inserted is on the row after what was selected. `None` with no
    /// non-empty selection.
    pub fn selection_scope(&mut self) -> Option<Vec<Range<Anchor>>> {
        self.model.sync_display();
        let text = self.model.text();
        let snapshot = self.model.snapshot();
        let scope: Vec<_> = (self.model.selections().into_iter())
            .filter(|selection| !selection.range.is_empty())
            .map(|selection| {
                let Range { start, end } = selection.range;
                let end = match text.offset_to_point(end).column {
                    0 => snapshot.anchor_before(end),
                    _ => snapshot.anchor_after(end),
                };
                snapshot.anchor_before(start)..end
            })
            .collect();
        (!scope.is_empty()).then_some(scope)
    }

    /// The find's scope in the text as it is, sorted, apart and with nothing
    /// empty: a selection all deleted is gone from it, as is one whose
    /// anchors can't be read any more, so a find finds less, never more.
    /// `None` with no scope.
    pub fn find_scope(&self) -> Option<Vec<Range<usize>>> {
        let scope = self.find.scope.as_ref()?;
        let snapshot = self.model.snapshot();
        let versions = snapshot.oldest_version()..=snapshot.version();
        let ends: Vec<Anchor> = (scope.iter())
            .filter(|range| versions.contains(&range.start.version()))
            .flat_map(|range| [range.start, range.end])
            .collect();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for &[start, end] in snapshot.resolve_many(&ends).as_chunks::<2>().0 {
            match ranges.last_mut() {
                _ if start >= end => {}
                Some(last) if start <= last.end => last.end = last.end.max(end),
                _ => ranges.push(start..end),
            }
        }
        Some(ranges)
    }

    /// Whether `found`, a match in `text`, lies wholly inside `scope`, which
    /// is as [`EditorView::find_scope`] gives it. An empty one at a range's
    /// end doesn't if that's a row's start: a range ending there took in the
    /// row before's line break, not the row, as with whole rows selected,
    /// so `^` doesn't match on the row after them.
    pub fn in_find_scope(text: &Text, scope: &[Range<usize>], found: &Range<usize>) -> bool {
        let at = scope.partition_point(|range| range.end < found.end);
        let Some(range) = scope.get(at).filter(|range| range.start <= found.start) else {
            return false;
        };
        !(found.is_empty() && found.start == range.end && {
            found.start > range.start && text.offset_to_point(found.start).column == 0
        })
    }

    /// The part of `window` from the first of `scope`'s ranges reaching it to
    /// the last: all a match inside `scope` that reaches `window` has to
    /// reach. `None` with none reaching it, when nothing need be searched,
    /// as when the only range touching it ends at its start, a row's start.
    pub(crate) fn find_scope_window(
        text: &Text,
        scope: &[Range<usize>],
        window: &Range<usize>,
    ) -> Option<Range<usize>> {
        let first = scope.partition_point(|range| {
            range.end < window.start || (range.end == window.start && ends_before_row(text, range))
        });
        let last = scope.partition_point(|range| range.start <= window.end);
        (first < last)
            .then(|| window.start.max(scope[first].start)..window.end.min(scope[last - 1].end))
    }

    /// The rows `scope` (as [`EditorView::find_scope`] gives it) has any of,
    /// each once, in order, with the part of each that a match inside it has
    /// to reach.
    pub fn find_scope_rows<'a>(
        text: &'a Text,
        scope: &'a [Range<usize>],
    ) -> impl Iterator<Item = (u32, Range<usize>)> + 'a {
        let mut last = None;
        (scope.iter())
            .flat_map(|range| rows_of(text, range))
            .filter(move |&row| last.replace(row) != Some(row))
            .filter_map(|row| {
                let start = text.row_start(row);
                let whole = start..start + text.row_len(row) as usize;
                Some((row, Self::find_scope_window(text, scope, &whole)?))
            })
    }

    /// The matches on `row` that reach `window` and are inside `scope`, in
    /// order, at most `cap` of them, stepping through the row no further than
    /// those.
    pub(crate) fn scoped_on_row(
        text: &Text,
        row: u32,
        window: Range<usize>,
        pattern: &Pattern,
        scope: &[Range<usize>],
        cap: usize,
        deadline: &mut Deadline,
    ) -> Vec<Range<usize>> {
        kept_on_row(text, row, window, pattern, cap, deadline, |found| {
            Self::in_find_scope(text, scope, found)
        })
    }

    /// In `text`, the first match inside `scope` (see
    /// [`EditorView::in_find_scope`]) from `from` on that's `wanted`, else,
    /// wrapping, the first inside it. Each row is stepped through only as far
    /// as that match. `Err` if `deadline` passed by then, as unscoped: what
    /// it found isn't to be relied on past that.
    pub fn next_in_find_scope(
        text: &Text,
        pattern: &Pattern,
        scope: &[Range<usize>],
        from: usize,
        wanted: impl Fn(&Range<usize>) -> bool,
        deadline: &mut Deadline,
    ) -> Result<Option<Range<usize>>, Interrupted> {
        let inside = |found: &Range<usize>| Self::in_find_scope(text, scope, found);
        let first = scope.partition_point(|range| range.end < from);
        for (ix, range) in scope[first..].iter().enumerate() {
            let start = if ix == 0 {
                range.start.max(from)
            } else {
                range.start
            };
            for row in rows_of(text, &(start..range.end)) {
                let window = row_part(text, row, &(start..range.end));
                let found = kept_on_row(text, row, window, pattern, 1, deadline, |found| {
                    inside(found) && wanted(found)
                });
                if deadline.expired() {
                    return Err(Interrupted);
                }
                if let Some(found) = found.into_iter().next() {
                    return Ok(Some(found));
                }
            }
        }
        for range in scope {
            for row in rows_of(text, range) {
                let window = row_part(text, row, range);
                let found = kept_on_row(text, row, window, pattern, 1, deadline, inside);
                if deadline.expired() {
                    return Err(Interrupted);
                }
                if let Some(found) = found.into_iter().next() {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
    }

    /// In `text`, the last match inside `scope` starting before `from`,
    /// else, wrapping, the last inside it. As going back unscoped does, it
    /// steps through a row's matches up to `from`, keeping only the latest.
    /// `Err` if `deadline` passed first.
    fn previous_in_find_scope(
        text: &Text,
        pattern: &Pattern,
        scope: &[Range<usize>],
        from: usize,
        deadline: &mut Deadline,
    ) -> Result<Option<Range<usize>>, Interrupted> {
        // The last match on `row` starting before `before`. Cut short, a later
        // one may not have been reached, so that's not an answer.
        let last_on_row =
            |row: u32, range: &Range<usize>, before: usize, deadline: &mut Deadline| {
                let mut last = None;
                let window = row_part(text, row, range);
                visit_reaching(text, row, window, pattern, deadline, |found| {
                    if found.start < before && Self::in_find_scope(text, scope, &found) {
                        last = Some(found);
                    }
                    ControlFlow::Continue(())
                });
                match deadline.expired() {
                    true => Err(Interrupted),
                    false => Ok(last),
                }
            };
        let last = scope.partition_point(|range| range.start < from);
        for range in scope[..last].iter().rev() {
            let before = range.start..range.end.min(from);
            for row in rows_of(text, &before).rev() {
                if let Some(found) = last_on_row(row, &before, from, deadline)? {
                    return Ok(Some(found));
                }
            }
        }
        for range in scope.iter().rev() {
            for row in rows_of(text, range).rev() {
                if let Some(found) = last_on_row(row, range, usize::MAX, deadline)? {
                    return Ok(Some(found));
                }
            }
        }
        Ok(None)
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

    /// The matches counted, by start, the first [`MAX_COUNTED`]: `None` with
    /// no pattern, or while the text has changed since they were counted.
    pub(crate) fn find_counted(&self) -> Option<&[Range<usize>]> {
        self.find.pattern.as_ref()?;
        let counted = self.find.counted.as_ref()?;
        (counted.version == self.model.snapshot().version()).then_some(&counted.ranges[..])
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
        self.catch_up_find(cx);
        let Some(pattern) = &self.find.pattern else {
            return false;
        };
        let text = self.model.text();
        let found = match self.find_scope() {
            Some(scope) => Self::next_in_find_scope(
                text,
                pattern,
                &scope,
                offset,
                |found| found.start >= offset,
                &mut Deadline::new(),
            )
            .ok()
            .flatten(),
            None => pattern.find_from(text, offset),
        };
        let Some(found) = found else {
            return false;
        };
        self.select_match(found, cx);
        true
    }

    /// Catches up with edits made through another view of the buffer, as
    /// prepaint does, so a search reads the text and selection as they are.
    fn catch_up_find(&mut self, cx: &mut Context<Self>) {
        self.model.sync_display();
        self.sync_find(cx);
    }

    fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        self.catch_up_find(cx);
        let Some(pattern) = &self.find.pattern else {
            return false;
        };
        let from = self.model.primary_selection().range.start;
        let text = self.model.text();
        let found = match self.find_scope() {
            Some(scope) if forward => Self::next_in_find_scope(
                text,
                pattern,
                &scope,
                from,
                |found| found.start > from,
                &mut Deadline::new(),
            )
            .ok()
            .flatten(),
            Some(scope) => {
                Self::previous_in_find_scope(text, pattern, &scope, from, &mut Deadline::new())
                    .ok()
                    .flatten()
            }
            None => pattern.find(text, from, forward),
        };
        let Some(found) = found else {
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
        let scope = self.find_scope();
        let task = cx.spawn({
            let stop = stop.clone();
            async move |this, cx| {
                let counted = cx
                    .background_spawn(async move {
                        match scope {
                            Some(scope) => count_in_scope(&text, &pattern, &scope, {
                                let stop = stop.clone();
                                move || stop.load(Ordering::Relaxed)
                            }),
                            None => count(&text, &pattern, &stop),
                        }
                    })
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
        ranges.extend(matches_in_until(
            text,
            start..end,
            pattern,
            room,
            &mut Deadline::never(),
        ));
        if ranges.len() > MAX_COUNTED {
            ranges.truncate(MAX_COUNTED);
            return Some((ranges, true));
        }
        start = end;
    }
    Some((ranges, false))
}

/// [`count`] of the matches inside `scope` (see
/// [`EditorView::in_find_scope`]), searching only its rows, no further than
/// the cap needs. `None` once `stop` says so, which is asked before every
/// regex call.
fn count_in_scope(
    text: &Text,
    pattern: &Pattern,
    scope: &[Range<usize>],
    stop: impl Fn() -> bool + Send + 'static,
) -> Option<(Vec<Range<usize>>, bool)> {
    let mut deadline = Deadline::cancelled_by(stop);
    let mut ranges = Vec::new();
    for (row, window) in EditorView::find_scope_rows(text, scope) {
        // A row with nothing to search asks too.
        if deadline.expired() {
            return None;
        }
        visit_reaching(text, row, window, pattern, &mut deadline, |found| {
            if EditorView::in_find_scope(text, scope, &found) {
                ranges.push(found);
            }
            // One past the cap says whether there are more.
            match ranges.len() > MAX_COUNTED {
                true => ControlFlow::Break(()),
                false => ControlFlow::Continue(()),
            }
        });
        if ranges.len() > MAX_COUNTED {
            ranges.truncate(MAX_COUNTED);
            return Some((ranges, true));
        }
        if deadline.expired() {
            return None;
        }
    }
    Some((ranges, false))
}

/// Whether `range`'s end is a row's start, after the line break it took in,
/// so that nothing on that row is in it.
fn ends_before_row(text: &Text, range: &Range<usize>) -> bool {
    range.start < range.end && text.offset_to_point(range.end).column == 0
}

/// The rows `range` has any of: not one it only ends at the start of.
fn rows_of(text: &Text, range: &Range<usize>) -> std::ops::RangeInclusive<u32> {
    let end = text.offset_to_point(range.end);
    let last = match end.column == 0 && end.row > 0 {
        true => end.row - 1,
        false => end.row,
    };
    text.offset_to_point(range.start).row..=last
}

/// The part of `range` on `row`.
fn row_part(text: &Text, row: u32, range: &Range<usize>) -> Range<usize> {
    let start = text.row_start(row);
    range.start.max(start)..range.end.min(start + text.row_len(row) as usize)
}

/// The matches on `row` that reach `window` and that `keep` takes, in order,
/// at most `cap`, searching no further than the last of them. Ask the deadline
/// afterwards whether it passed.
fn kept_on_row(
    text: &Text,
    row: u32,
    window: Range<usize>,
    pattern: &Pattern,
    cap: usize,
    deadline: &mut Deadline,
    keep: impl Fn(&Range<usize>) -> bool,
) -> Vec<Range<usize>> {
    let mut kept = Vec::new();
    if cap == 0 {
        return kept;
    }
    visit_reaching(text, row, window, pattern, deadline, |found| {
        if keep(&found) {
            kept.push(found);
        }
        match kept.len() < cap {
            true => ControlFlow::Continue(()),
            false => ControlFlow::Break(()),
        }
    });
    kept
}

#[cfg(test)]
#[path = "find_scope_tests.rs"]
mod find_scope_tests;
