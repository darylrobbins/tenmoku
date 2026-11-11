//! What the editor tells a screen reader on its own, through the window's
//! live region (kiln-ui's [`fe_ui::announce`], card A11Y-2), as sections 4
//! to 6 of `docs/decisions/qenw9xxdzk-accessibility-design.md` list:
//!
//! - **Vim's mode**, politely, when it changes: "Normal", "Insert" and so
//!   on, an operator waiting still normal mode. The command line takes focus
//!   and says its own name, so entering it says nothing.
//! - **Vim's message**, when one is set: assertively for an error (one vim
//!   failed the key with, coded `E486: …` or not, or the host's
//!   [`EditorView::set_vim_error`]), politely otherwise (`:s`'s report).
//!   A key ends the message, and each host setting is new, so the same text
//!   again is said again.
//! - **The diagnostics at the caret**, politely, "error: <message>", once
//!   the caret has rested in them for [`HOVER_DELAY`] (each move restarts
//!   the wait, and it's dropped unless the editor still has focus when it's
//!   up), once each a visit, two with the same text being two; at once
//!   after next or previous problem
//!   ([`EditorView::announce_caret_diagnostics`]).
//! - **Folds**: "Folded N lines" or "Unfolded" when lines are hidden or
//!   shown with the text unchanged.
//! - **Cursors**: "N cursors" when one becomes more, "One cursor" back.
//! - **A clipped selection**: "N lines selected" when the text node holds
//!   only part of the primary selection (see [`clips`]), once a selection.
//! - **Ghost text**: "Suggestion available" when a suggestion appears, not
//!   when it changes.
//! - **Completion**: "N suggestions, <label>" when the menu opens, then the
//!   selected label when the selection moves, the list refilters as the
//!   text changes or a new answer replaces it; nothing
//!   on accept (the text typed is echoed) or dismiss. The menu takes no
//!   focus: the text area stays the focused node throughout.
//! - **Editor: Announce Position**: "line N, column M of T"
//!   ([`EditorView::announce_position`]).
//!
//! All but the position and the problem commands are noticed as the view
//! renders, by comparing what it shows with what it showed, only while the
//! editor has focus; the first frame only takes note. Everything is polite
//! unless it says otherwise. The announcer drops what no screen reader
//! hears.

use std::ops::Range;

use fe_buffer::Version;
use fe_editor::diagnostics::Severity;
use fe_ui::{Politeness, announce};
use gpui::{App, Context, SharedString, Subscription, Task, Window, canvas};
use kiln_l10n::tr;

use crate::{
    EditorView,
    element::{A11yTextCell, a11y_text::A11yText},
    hover::HOVER_DELAY,
    pane_item::status,
};

/// What the view last told, or saw, for its announcements to compare with.
#[derive(Default)]
pub(crate) struct Announced {
    /// A frame has been looked at: the first only takes note.
    seen: bool,
    /// The vim mode as last said, or seen.
    vim_mode: Option<SharedString>,
    /// The vim message last said, and the host's setting of it if it set
    /// it, until the message goes or a key ends it.
    vim_message: Option<(String, u64)>,
    /// The message the host last set, which of its settings that was
    /// (counting from 1) and whether it's an error: the host can set the
    /// same text again, which is said again.
    host_message: Option<(String, u64, bool)>,
    host_settings: u64,
    cursors: usize,
    ghost_text: bool,
    /// The completion menu's list (a new answer or a refilter makes a new
    /// one) and selected item, by index and label, while it shows.
    completion: Option<(u64, Option<usize>, String)>,
    /// The lines folds hide, and the text's version then.
    folded: (usize, Option<Version>),
    caret: CaretDiagnostics,
    on_blur: OnBlur,
    /// The primary selection a clipped selection was last checked for.
    clipped: Option<Range<usize>>,
}

/// A diagnostic at the caret: its range, which tells two with the same
/// text apart, then what's told, its severity and message.
type Spoken = (Range<usize>, Severity, String);

/// The caret's visit to diagnostics.
#[derive(Default)]
struct CaretDiagnostics {
    /// Those the caret is in.
    under: Vec<Spoken>,
    /// Where the caret was: a move restarts the wait.
    at: Option<usize>,
    /// Those of them told this visit.
    said: Vec<Spoken>,
    /// Tells the rest once the caret has rested.
    _wait: Option<Task<()>>,
}

/// Ends the caret's visit when the editor loses focus, drawn or not.
#[derive(Default)]
struct OnBlur(Option<Subscription>);

impl Announced {
    /// The host set `message` as vim's message, an error or not: it's said
    /// even if it's the text there already.
    pub(crate) fn host_message(&mut self, message: String, error: bool) {
        self.host_settings += 1;
        self.host_message = Some((message, self.host_settings, error));
    }

    /// A key reached the editor, which ends vim's message, the host's too:
    /// one there after it is new, even with the same text, and vim's own
    /// unless the host sets it again.
    pub(crate) fn key(&mut self) {
        self.vim_message = None;
        self.host_message = None;
    }
}

impl EditorView {
    /// Tells a screen reader what changed since the last frame (see the
    /// module docs), for a frame about to be drawn.
    pub(crate) fn announce_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.focus.is_focused(window);
        if self.announced.on_blur.0.is_none() {
            let blur = cx.on_blur(&self.focus, window, |view, _, _| {
                view.announced.caret = CaretDiagnostics::default();
            });
            self.announced.on_blur.0 = Some(blur);
        }
        let first = !std::mem::replace(&mut self.announced.seen, true);
        let mut said = Said::default();
        // Only the focused editor of the active window speaks: another
        // window's changes would be said in this one's live region.
        let quiet = first || !focused || !window.is_window_active();

        let mode = (self.vim_mode()).map(|mode| (mode, status::vim_mode_spoken(mode)));
        match mode {
            // The command line says its own name; what it returns to is
            // compared with the mode before it.
            Some((_, None)) => {}
            Some((_, Some(spoken))) => {
                if self.announced.vim_mode.as_ref() != Some(&spoken) && !quiet {
                    said.polite(spoken.to_string());
                }
                self.announced.vim_mode = Some(spoken);
            }
            None => self.announced.vim_mode = None,
        }

        // The host's, if it set the text there (since the last key), an
        // error as it says or with nvim's code; else vim's own, an error if
        // it failed the key (`register / is read-only`) or has nvim's code.
        let message = (self.vim_state()).and_then(|state| {
            let message = state.message()?;
            let host = (self.announced.host_message.as_ref())
                .filter(|(text, _, _)| text == message)
                .map(|(_, setting, error)| (*setting, *error));
            let (setting, error) = host.unwrap_or((0, state.failed()));
            Some((message.to_owned(), setting, error || is_vim_error(message)))
        });
        if message.is_none() {
            // The message ended: what the host said of it goes with it.
            self.announced.host_message = None;
        }
        let now = message
            .as_ref()
            .map(|(text, setting, _)| (text.clone(), *setting));
        if now != self.announced.vim_message {
            if let Some((text, _, error)) = message
                && !quiet
            {
                match error {
                    true => said.assertive(text),
                    false => said.polite(text),
                }
            }
            self.announced.vim_message = now;
        }

        let cursors = self.model.selections().len();
        let was = std::mem::replace(&mut self.announced.cursors, cursors);
        if !quiet && was != 0 && (was == 1) != (cursors == 1) {
            said.polite(match cursors {
                1 => tr!("gpui-a11y-one-cursor").to_string(),
                n => tr!("gpui-a11y-cursors", count = n).to_string(),
            });
        }

        let ghost = self.ghost_text_shown();
        if ghost && !self.announced.ghost_text && !quiet {
            said.polite(tr!("gpui-a11y-suggestion-available").to_string());
        }
        self.announced.ghost_text = ghost;

        let completion = self.completion_spoken();
        let now = (completion.as_ref())
            .map(|spoken| (spoken.list, spoken.selected, spoken.label.to_owned()));
        if now != self.announced.completion && !quiet {
            match (&self.announced.completion, &completion) {
                (None, Some(spoken)) => said.polite(
                    tr!(
                        "gpui-a11y-suggestions",
                        count = spoken.count,
                        label = spoken.label
                    )
                    .to_string(),
                ),
                (Some(_), Some(spoken)) => said.polite(spoken.label.to_owned()),
                (_, None) => {}
            }
        }
        self.announced.completion = now;

        let version = self.model.snapshot().version();
        let hidden = hidden_lines(
            self.model
                .display()
                .folds()
                .iter()
                .map(|f| (f.start, f.end)),
        );
        let (was, at) = std::mem::replace(&mut self.announced.folded, (hidden, Some(version)));
        if !quiet && at == Some(version) && hidden != was {
            said.polite(match hidden > was {
                true => tr!("gpui-a11y-folded", count = hidden - was).to_string(),
                false => tr!("gpui-a11y-unfolded").to_string(),
            });
        }

        self.watch_caret_diagnostics(focused, window, cx);
        said.tell(cx);
    }

    /// Starts or ends the caret's visit to the diagnostics it's in: those
    /// not yet told this visit are told once the caret has rested
    /// [`HOVER_DELAY`], each move restarting the wait. Losing focus ends
    /// the visit.
    fn watch_caret_diagnostics(&mut self, focused: bool, window: &Window, cx: &mut Context<Self>) {
        let (under, at) = match focused {
            true => (
                self.caret_diagnostics(),
                Some(self.model.primary_selection().head()),
            ),
            false => (Vec::new(), None),
        };
        let caret = &mut self.announced.caret;
        if under == caret.under && at == caret.at {
            return;
        }
        caret.said.retain(|said| under.contains(said));
        let unsaid = under.iter().any(|spoken| !caret.said.contains(spoken));
        caret.under = under;
        caret.at = at;
        caret._wait = unsaid.then(|| {
            cx.spawn_in(window, async move |view, cx| {
                cx.background_executor().timer(HOVER_DELAY).await;
                (view.update_in(cx, |view, window, cx| {
                    // Still where it rested, in the focused editor of the
                    // active window: a tab switch may have left this one
                    // undrawn, with nothing to end the wait.
                    let caret = &view.announced.caret;
                    let head = view.model.primary_selection().head();
                    let stands = view.focus.is_focused(window)
                        && window.is_window_active()
                        && caret.at == Some(head)
                        && caret.under == view.caret_diagnostics();
                    match stands {
                        true => view.say_caret_diagnostics(false, cx),
                        // The visit is over: the next frame in focus starts
                        // another.
                        false => view.announced.caret = CaretDiagnostics::default(),
                    }
                }))
                .ok();
            })
        });
    }

    /// The diagnostics the caret is in, as the hover popover finds them
    /// under the pointer: those holding the character after it, and the
    /// empty ones at it.
    fn caret_diagnostics(&self) -> Vec<Spoken> {
        let caret = self.model.primary_selection().head();
        (self.model.diagnostics().iter())
            .filter(|entry| {
                entry.range.contains(&caret) || entry.range.is_empty() && entry.range.start == caret
            })
            .map(|entry| (entry.range.clone(), entry.severity, entry.message.clone()))
            .collect()
    }

    /// Tells the diagnostics at the caret not yet told this visit, or with
    /// `again` all of them.
    fn say_caret_diagnostics(&mut self, again: bool, cx: &mut App) {
        let under = self.caret_diagnostics();
        let caret = &mut self.announced.caret;
        if again {
            caret.said.clear();
        }
        let unsaid: Vec<Spoken> = (under.iter())
            .filter(|spoken| !caret.said.contains(spoken))
            .cloned()
            .collect();
        caret._wait = None;
        caret.under = under;
        caret.at = Some(self.model.primary_selection().head());
        if unsaid.is_empty() {
            return;
        }
        let text: Vec<String> = unsaid.iter().map(diagnostic_text).collect();
        caret.said.extend(unsaid);
        announce(cx, text.join("\n"), Politeness::Polite);
    }

    /// Tells the diagnostics at the caret at once, after a command put the
    /// caret on one (Go to Next Problem), even if this visit told them.
    pub fn announce_caret_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.say_caret_diagnostics(true, cx);
    }

    /// `editor.announce_position`: tells the caret's line and column, as the
    /// status bar counts them, and how many lines the text has.
    pub(crate) fn announce_position(&mut self, cx: &mut Context<Self>) {
        let text = self.model.text();
        let head = self.model.primary_selection().head();
        let row = text.offset_to_point(head).row;
        let column = text.offset_to_char(head) - text.offset_to_char(text.row_start(row)) + 1;
        let said = tr!(
            "gpui-a11y-position",
            line = row + 1,
            column = column,
            total = text.row_count()
        );
        announce(cx, said, Politeness::Polite);
    }

    /// `area`, the text area's element, made the focused node, its own
    /// active descendant, while the editor has focus: the completion menu
    /// and the other popovers take none. Noted in
    /// [`EditorView::text_area_active`] as it's applied.
    pub(crate) fn text_area_active_descendant<E: gpui::StatefulInteractiveElement>(
        &mut self,
        area: E,
        focused: bool,
    ) -> E {
        self.text_area_active = focused;
        match focused {
            true => area.aria_active_descendant(),
            false => area,
        }
    }

    /// An empty element for the text area, after the editor element, that
    /// tells a screen reader "N lines selected" once the frame's text node
    /// is built and holds only part of the primary selection: once for each
    /// selection.
    pub(crate) fn clipped_selection_watch(
        &self,
        cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement + use<> {
        use gpui::Styled as _;
        let (view, cell) = (cx.weak_entity(), self.a11y_text.clone());
        canvas(
            move |_, window, cx| {
                (view.update(cx, |view, cx| view.say_clipped_selection(&cell, window, cx))).ok();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }

    pub(super) fn say_clipped_selection(
        &mut self,
        cell: &A11yTextCell,
        window: &Window,
        cx: &mut App,
    ) {
        let cell = cell.borrow();
        let Some(text) = &cell.text else {
            return;
        };
        let selection = self.model.primary_selection().range;
        if self.announced.clipped.as_ref() == Some(&selection) {
            return;
        }
        self.announced.clipped = Some(selection.clone());
        // A split's other editor, or another window's, keeps quiet: its
        // selection is noted, so it isn't said once focus comes to it.
        if !self.focus.is_focused(window) || !window.is_window_active() {
            return;
        }
        let model_text = self.model.text();
        let row_end = |row: u32| model_text.row_start(row) + model_text.row_len(row) as usize;
        let display = self.model.display();
        let fold_header = |offset: usize| {
            let row = model_text.offset_to_point(offset).row;
            Some(display.hidden_rows(row)?.start - 1)
        };
        if !clips(text, selection.clone(), row_end, fold_header) {
            return;
        }
        let first = model_text.offset_to_point(selection.start).row;
        let end = model_text.offset_to_point(selection.end);
        // A selection ending at a row's start doesn't take that row.
        let last = match end.column == 0 && end.row > first {
            true => end.row - 1,
            false => end.row,
        };
        let lines = (last - first + 1) as usize;
        announce(
            cx,
            tr!("gpui-a11y-lines-selected", count = lines),
            Politeness::Polite,
        );
    }
}

/// Whether `message` is one of vim's errors: `E486: Pattern not found`.
fn is_vim_error(message: &str) -> bool {
    let Some(code) = message.strip_prefix('E') else {
        return false;
    };
    let digits = code.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && code[digits..].starts_with(':')
}

/// "error: <message>", by its severity.
fn diagnostic_text((_, severity, message): &Spoken) -> String {
    let said = match severity {
        Severity::Error => tr!("gpui-a11y-diagnostic-error", message = message),
        Severity::Warning => tr!("gpui-a11y-diagnostic-warning", message = message),
        Severity::Information => tr!("gpui-a11y-diagnostic-information", message = message),
        Severity::Hint => tr!("gpui-a11y-diagnostic-hint", message = message),
    };
    said.to_string()
}

/// How many buffer rows `folds` hide, each `(header, last)` hiding the rows
/// after its header up to its last, sorted by header: nested folds count
/// once.
pub(crate) fn hidden_lines(folds: impl IntoIterator<Item = (u32, u32)>) -> usize {
    let mut hidden = 0;
    // The last row counted, inclusive.
    let mut counted: Option<u32> = None;
    for (header, last) in folds {
        let first = match counted {
            Some(counted) => (header + 1).max(counted + 1),
            None => header + 1,
        };
        if first <= last {
            hidden += (last - first + 1) as usize;
        }
        counted = Some(counted.map_or(last, |counted| counted.max(last)));
    }
    hidden
}

/// Whether `text`, the text node's content, holds only part of the
/// non-empty `selection`: an end of it moved to the window's edge, being
/// above or below the rows the node holds, or to a truncated run's end,
/// being past where that row was cut. `row_end` gives a buffer row's end,
/// before its line break, and `fold_header` the header row of the fold an
/// offset is hidden by, if any. An end hidden by a fold whose header is a
/// run moves to that run's text end, but the node still holds the
/// selection's lines, so that doesn't count, even past the last run.
pub(crate) fn clips(
    text: &A11yText,
    selection: Range<usize>,
    row_end: impl Fn(u32) -> usize,
    fold_header: impl Fn(usize) -> Option<u32>,
) -> bool {
    if selection.is_empty() {
        return false;
    }
    let (Some(first), Some(last)) = (text.runs.first(), text.runs.last()) else {
        return false;
    };
    let folded = |offset: usize| {
        fold_header(offset)
            .is_some_and(|header| text.runs.iter().any(|run| run.key.buffer_row == header))
    };
    let cut = |offset: usize| {
        !folded(offset)
            && (offset < first.start
                || offset > last.start + last.text.len()
                || (text.runs.iter()).any(|run| {
                    run.truncated
                        && offset > run.start + run.text_len
                        && offset <= row_end(run.key.buffer_row)
                }))
    };
    cut(selection.start) || cut(selection.end)
}

/// What a frame has to tell, which goes as one message: the announcer says
/// one a frame.
#[derive(Default)]
struct Said {
    lines: Vec<String>,
    assertive: bool,
}

impl Said {
    fn polite(&mut self, text: String) {
        self.lines.push(text);
    }

    fn assertive(&mut self, text: String) {
        self.lines.push(text);
        self.assertive = true;
    }

    fn tell(self, cx: &mut App) {
        if self.lines.is_empty() {
            return;
        }
        let politeness = match self.assertive {
            true => Politeness::Assertive,
            false => Politeness::Polite,
        };
        announce(cx, self.lines.join("\n"), politeness);
    }
}
