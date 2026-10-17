//! Vi copy mode: a block cursor, apart from the shell's, that moves over the
//! grid and its scrollback with vim's motions, selects and finds text, while
//! the view stays on what it shows as output arrives (alacritty's model, with
//! tmux's exit: leaving drops the selection and goes back to the bottom).

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vi_mode::ViMotion;

use crate::pty_loop::lock;
use crate::search::{self, QueryError, TerminalMatch, TerminalQuery};
use crate::session::{SelectionKind, Terminal};
use crate::shell::ShellState;

/// Where [`Terminal::vi_motion`] moves the vi cursor, by vim's key for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViMove {
    /// `h`
    Left,
    /// `j`
    Down,
    /// `k`
    Up,
    /// `l`
    Right,
    /// `w`: the start of the next word, split on whitespace and shell
    /// punctuation.
    WordNext,
    /// `b`: the start of this word or the one before.
    WordPrevious,
    /// `e`: the end of this word or the next.
    WordEnd,
    /// `W`: the start of the next whitespace-separated WORD.
    BigWordNext,
    /// `B`
    BigWordPrevious,
    /// `E`
    BigWordEnd,
    /// `0`: the line's first column.
    LineStart,
    /// `^`: the line's first non-blank cell.
    FirstNonBlank,
    /// `$`: the line's last non-blank cell.
    LineEnd,
    /// `H`: the view's top row.
    High,
    /// `M`: the view's middle row.
    Middle,
    /// `L`: the view's bottom row.
    Low,
    /// `%`: the bracket matching the one under the cursor.
    Bracket,
    /// `{`: the blank line above the paragraph.
    ParagraphUp,
    /// `}`: the blank line below the paragraph.
    ParagraphDown,
    /// `gg`: the first column of the oldest history line.
    Top,
    /// `G`: the first column of the screen's last line.
    Bottom,
}

impl ViMove {
    /// alacritty's motion for this one, `None` for those that go to a point.
    fn motion(self) -> Option<ViMotion> {
        Some(match self {
            ViMove::Left => ViMotion::Left,
            ViMove::Down => ViMotion::Down,
            ViMove::Up => ViMotion::Up,
            ViMove::Right => ViMotion::Right,
            ViMove::WordNext => ViMotion::SemanticRight,
            ViMove::WordPrevious => ViMotion::SemanticLeft,
            ViMove::WordEnd => ViMotion::SemanticRightEnd,
            ViMove::BigWordNext => ViMotion::WordRight,
            ViMove::BigWordPrevious => ViMotion::WordLeft,
            ViMove::BigWordEnd => ViMotion::WordRightEnd,
            ViMove::LineStart => ViMotion::First,
            ViMove::FirstNonBlank => ViMotion::FirstOccupied,
            ViMove::LineEnd => ViMotion::Last,
            ViMove::High => ViMotion::High,
            ViMove::Middle => ViMotion::Middle,
            ViMove::Low => ViMotion::Low,
            ViMove::Bracket => ViMotion::Bracket,
            ViMove::ParagraphUp => ViMotion::ParagraphUp,
            ViMove::ParagraphDown => ViMotion::ParagraphDown,
            ViMove::Top | ViMove::Bottom => return None,
        })
    }
}

impl Terminal {
    /// Whether vi copy mode is on.
    pub fn is_vi_mode(&self) -> bool {
        self.term.lock().mode().contains(TermMode::VI)
    }

    /// Turns vi copy mode on or off. Entering puts the vi cursor on the
    /// shell's cursor (on the view's top-left cell when that's out of view)
    /// and drops any selection; leaving drops the selection and scrolls to
    /// the bottom.
    pub fn set_vi_mode(&self, on: bool) {
        let mut term = self.term.lock();
        if term.mode().contains(TermMode::VI) == on {
            return;
        }
        term.toggle_vi_mode();
        term.selection = None;
        if !on {
            if term.grid().display_offset() != 0 {
                term.scroll_display(Scroll::Bottom);
            }
            lock(&self.shell).forget();
        }
    }

    /// Moves the vi cursor, scrolling the view to keep it in sight; a
    /// selection's moving end follows it. Nothing outside vi mode.
    pub fn vi_motion(&self, to: ViMove) {
        let mut term = self.term.lock();
        if !term.mode().contains(TermMode::VI) {
            return;
        }
        let offset = term.grid().display_offset();
        // alacritty's middle of a one-row view is the row above it.
        let to = match to {
            ViMove::Middle if term.screen_lines() < 2 => ViMove::High,
            to => to,
        };
        match to.motion() {
            Some(motion) => term.vi_motion(motion),
            None => {
                let line = match to {
                    ViMove::Top => term.topmost_line(),
                    _ => term.bottommost_line(),
                };
                term.vi_goto_point(Point::new(line, Column(0)));
            }
        }
        lock(&self.shell).view_moved(offset, &term);
    }

    /// Scrolls the view by `lines` (negative back into history, positive
    /// toward the bottom) with the vi cursor keeping its row on screen, as
    /// vim's ctrl-e, ctrl-u and ctrl-b and the others do. Where the view
    /// can't scroll that far the cursor moves the rest of the way, within
    /// the view. Nothing outside vi mode.
    pub fn vi_scroll(&self, lines: i32) {
        let mut term = self.term.lock();
        if !term.mode().contains(TermMode::VI) {
            return;
        }
        let offset = term.grid().display_offset();
        let point = term.vi_mode_cursor.point;
        term.scroll_display(Scroll::Delta(lines.saturating_neg()));
        let top = -(term.grid().display_offset() as i32);
        let bottom = top + term.screen_lines() as i32 - 1;
        let line = point.line.0.saturating_add(lines).clamp(top, bottom);
        term.vi_goto_point(Point::new(Line(line), point.column));
        lock(&self.shell).view_moved(offset, &term);
    }

    /// Scrolls the view by `pages` pages (negative back into history,
    /// positive toward the bottom) of two rows less than its height, as
    /// vim's ctrl-f and ctrl-b do, as far as it can, then puts the vi cursor,
    /// keeping its column, on the view's top row after paging forward or its
    /// bottom row after paging back. Where the view can't move at all, paging
    /// forward puts the cursor on the bottom row and paging back leaves it.
    /// Nothing outside vi mode.
    pub fn vi_scroll_page(&self, pages: i32) {
        let mut term = self.term.lock();
        if !term.mode().contains(TermMode::VI) {
            return;
        }
        let offset = term.grid().display_offset();
        let point = term.vi_mode_cursor.point;
        let page = (term.screen_lines() as i32 - 2).max(1);
        term.scroll_display(Scroll::Delta(pages.saturating_mul(page).saturating_neg()));
        let top = -(term.grid().display_offset() as i32);
        let bottom = top + term.screen_lines() as i32 - 1;
        let moved = term.grid().display_offset() != offset;
        let line = match (pages > 0, moved) {
            (true, true) => top,
            (true, false) => bottom,
            (false, true) => bottom,
            (false, false) => point.line.0,
        };
        term.vi_goto_point(Point::new(Line(line), point.column));
        lock(&self.shell).view_moved(offset, &term);
    }

    /// Scrolls the view by `lines` (negative back into history) with the vi
    /// cursor staying on its line, as vim's ctrl-e and ctrl-y do; it moves
    /// only to the view's top or bottom row when its line would leave the
    /// view. Nothing outside vi mode.
    pub fn vi_scroll_view(&self, lines: i32) {
        let mut term = self.term.lock();
        if !term.mode().contains(TermMode::VI) {
            return;
        }
        let offset = term.grid().display_offset();
        let point = term.vi_mode_cursor.point;
        term.scroll_display(Scroll::Delta(lines.saturating_neg()));
        let top = -(term.grid().display_offset() as i32);
        let bottom = top + term.screen_lines() as i32 - 1;
        let line = point.line.0.clamp(top, bottom);
        term.vi_goto_point(Point::new(Line(line), point.column));
        lock(&self.shell).view_moved(offset, &term);
    }

    /// Starts a selection of `kind` at the vi cursor, as v, V and ctrl-v
    /// do: the same kind again clears it, and another changes its kind,
    /// keeping where it started. `None` clears it. Motions move its end.
    pub fn vi_select(&self, kind: Option<SelectionKind>) {
        let mut term = self.term.lock();
        let Some(kind) = kind else {
            term.selection = None;
            return;
        };
        let ty = kind.into();
        match &mut term.selection {
            Some(selection) if !selection.is_empty() && selection.ty == ty => {
                term.selection = None;
            }
            Some(selection) if !selection.is_empty() => {
                // Each kind takes its ends' sides its own way.
                selection.ty = ty;
                selection.include_all();
            }
            _ => {
                let point = term.vi_mode_cursor.point;
                let mut selection = Selection::new(ty, point, Side::Left);
                // The cell under the cursor is selected from the start.
                selection.include_all();
                term.selection = Some(selection);
            }
        }
    }

    /// Moves the vi cursor to the start of `query`'s next match after it
    /// (or before it, when not `forward`), wrapping through the whole
    /// history once, and scrolls the view to it; a selection extends to it.
    /// With no match nothing moves. Nothing outside vi mode.
    pub fn vi_search(
        &self,
        query: &TerminalQuery,
        forward: bool,
    ) -> Result<Option<TerminalMatch>, QueryError> {
        let mut term = self.term.lock();
        if !term.mode().contains(TermMode::VI) {
            search::compile(query)?;
            return Ok(None);
        }
        let cursor = term.vi_mode_cursor.point;
        // Strictly past the cursor, so the match it's on isn't found again;
        // back, from the first cell of a wide character it's on the spacer
        // of, which is where it's drawn.
        let origin = if forward {
            cursor.add(&*term, Boundary::None, 1)
        } else {
            let cursor = term.expand_wide(cursor, Direction::Left);
            cursor.sub(&*term, Boundary::None, 1)
        };
        let Some(found) = search::find_from(&term, query, origin, forward, Side::Left)? else {
            return Ok(None);
        };
        let offset = term.grid().display_offset();
        term.vi_goto_point(*found.start());
        lock(&self.shell).view_moved(offset, &term);
        Ok(Some(search::to_match(&found)))
    }

    /// Moves the vi cursor to the first column of the previous or next
    /// (`forward`) prompt the shell marked (OSC 133;A) above or below its
    /// line, as `[[` and `]]` do. Whether there was one.
    pub fn vi_prompt(&self, forward: bool) -> bool {
        let mut term = self.term.lock();
        if !term.mode().contains(TermMode::VI) {
            return false;
        }
        let mut shell = lock(&self.shell);
        let lines = shell.prompt_lines(&term);
        let line = term.vi_mode_cursor.point.line.0;
        let target = if forward {
            lines.iter().find(|&&mark| mark > line)
        } else {
            lines.iter().rev().find(|&&mark| mark < line)
        };
        let Some(&target) = target else {
            return false;
        };
        let offset = term.grid().display_offset();
        term.vi_goto_point(Point::new(Line(target), Column(0)));
        shell.view_moved(offset, &term);
        true
    }
}

/// What a parser call needs to keep vi mode's view on its text: taken
/// before the call, while the view is at the bottom. Once the view is
/// scrolled back alacritty keeps it where it is itself.
pub(crate) struct Hold {
    /// The rows scrolled into history before the call: the absolute row of
    /// grid line 0, as every parser call and resize leaves it on the
    /// primary screen.
    scrolled: u64,
    cursor: Point,
}

impl Hold {
    /// What to hold for the parser call about to run, if anything: only in
    /// vi mode, on the primary screen, with the view at the bottom. A call
    /// on the alternate screen ends with the sequence that leaves it (the
    /// loop splits there), so the primary output after it is held from the
    /// next call, once that one has brought `scrolled` in step with any
    /// resize made while the primary was hidden.
    pub(crate) fn before<T>(term: &Term<T>, shell: &ShellState) -> Option<Hold> {
        let mode = term.mode();
        let held = mode.contains(TermMode::VI)
            && !mode.contains(TermMode::ALT_SCREEN)
            && term.grid().display_offset() == 0;
        held.then_some(Hold {
            scrolled: shell.scrolled,
            cursor: term.vi_mode_cursor.point,
        })
    }

    /// Scrolls the view back by the rows the call scrolled into history,
    /// so it shows what it did, and puts the vi cursor back on its text,
    /// which alacritty held at the view's top.
    pub(crate) fn after<T: EventListener>(self, term: &mut Term<T>, shell: &ShellState) {
        let mode = term.mode();
        if !mode.contains(TermMode::VI)
            || mode.contains(TermMode::ALT_SCREEN)
            || term.grid().display_offset() != 0
        {
            return;
        }
        let scrolled = shell.scrolled.saturating_sub(self.scrolled);
        if scrolled == 0 {
            return;
        }
        let scrolled = i32::try_from(scrolled).unwrap_or(i32::MAX);
        let line = self.cursor.line.0.saturating_sub(scrolled);
        // The cursor first, as scrolling moves the selection's end to it.
        term.vi_mode_cursor.point.line = Line(line).grid_clamp(term, Boundary::Grid);
        term.scroll_display(Scroll::Delta(scrolled));
    }
}

/// Puts the vi cursor back in the view if output or a resize left it out
/// of it, or off the grid: a screen switch, or history trimmed from under
/// it. A selection's end follows it. Nothing outside vi mode, or with the
/// cursor in view.
pub(crate) fn keep_in_view<T: EventListener>(term: &mut Term<T>) {
    if !term.mode().contains(TermMode::VI) {
        return;
    }
    let point = term.vi_mode_cursor.point;
    let top = -(term.grid().display_offset() as i32);
    let bottom = top + term.screen_lines() as i32 - 1;
    let line = Line(point.line.0.clamp(top, bottom));
    let column = point.column.min(term.last_column());
    if (line, column) != (point.line, point.column) {
        term.vi_goto_point(Point::new(line, column));
    }
}
