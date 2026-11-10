//! 'inccommand' in the view: while a `:s` line is typed, its matches are
//! highlighted and each row it changes shows, after its end, as it would
//! read ([`EditorView::preview_substitute`]).

use std::{cell::RefCell, ops::Range, rc::Rc, sync::Arc};

use fe_buffer::Text;
use fe_editor::decorations::{Decoration, DecorationKind, DecorationSet, Tint};
use fe_vim::{CommandLineKind, Registers, SubstitutePreview, VimState};
use gpui::Context;

use super::{SearchPreview, vim_registers};
use crate::EditorView;

/// The most characters a row's preview after its end shows.
const ROW_PREVIEW_CHARS: usize = 120;

impl EditorView {
    /// While vim has a `:` line open, shows what submitting it as `line`
    /// would replace now ([`VimState::substitute_preview`], 'inccommand'):
    /// a highlight over each match, and after the end of each row it
    /// changes, that row as it would read (`→ …`, rows a match joins read
    /// as one), cut at [`ROW_PREVIEW_CHARS`]. A match from the caret's row
    /// on (or the first) is revealed as a search preview reveals its own,
    /// a fold hiding it opened, from where the view was before the line's
    /// first preview; the caret stays. A line that previews nothing (not a
    /// `:s`, still typed, invalid, not found, 'inccommand' off) puts the
    /// view, the folds and the decorations back. The host's line calls it
    /// after each edit; cancelling the line puts them back too, and
    /// submitting drops the decorations, the `:s` editing the text.
    pub fn preview_substitute(&mut self, line: &str, cx: &mut Context<Self>) {
        self.sync_vim(cx);
        if !matches!(
            self.vim.as_ref().map(VimState::mode),
            Some(fe_vim::Mode::CmdLine(CommandLineKind::Ex))
        ) {
            return;
        }
        self.undo_search_preview();
        self.model.sync_display();
        let Some(state) = &self.vim else {
            return;
        };
        let unset = Registers::default();
        let registers = vim_registers(cx).unwrap_or(&unset);
        let Some(preview) = state.substitute_preview(&self.model, registers, line) else {
            cx.notify();
            return;
        };
        if self.search_preview.is_none() {
            self.search_preview = Some(SearchPreview::from(self.place()));
        }
        let snapshot = self.model.snapshot().clone();
        let version = snapshot.version();
        let mut set = DecorationSet::new();
        set.apply(&snapshot, version, decorations(snapshot.text(), &preview));
        let provider = self
            .model
            .add_decoration_provider(Rc::new(RefCell::new(set)));
        let text = snapshot.text();
        let caret_row = text
            .offset_to_point(self.model.primary_selection().head())
            .row;
        let from = text.row_start(caret_row);
        let reveal = (preview.matches.iter())
            .find(|(range, _)| range.start >= from)
            .or(preview.matches.first())
            .map_or(0, |(range, _)| range.start);
        let before = text.clone();
        let events = self.model.buffer_mut().subscribe();
        let opened = self.open_folds_at(reveal);
        if let Some(shown) = &mut self.search_preview {
            shown.opened = opened;
            shown.opened_in = Some((before, events));
            shown.reveal = Some(reveal);
            shown.substitute = Some((provider, version));
            // The frame scrolls to it as it would to the caret, clear of
            // sticky scroll's band.
            self.autoscroll = true;
            self.band_spares_caret = false;
        }
        cx.notify();
    }

    /// Takes a `:s` line's preview decorations off the model, if any.
    pub(super) fn end_substitute_preview(&mut self) {
        let provider = (self.search_preview.as_mut()).and_then(|shown| shown.substitute.take());
        if let Some((provider, _)) = provider {
            self.model.remove_decoration_provider(provider);
        }
    }

    /// Takes a `:s` line's preview down if the buffer changed since it was
    /// made (through another view of it): what it shows is of the text
    /// before. The view and the folds go back as for a line that previews
    /// nothing; the line's next edit previews afresh.
    pub(crate) fn sync_substitute_preview(&mut self) {
        let version = self.model.snapshot().version();
        let stale = (self.search_preview.as_ref())
            .and_then(|shown| shown.substitute)
            .is_some_and(|(_, made)| made != version);
        if stale {
            self.undo_search_preview();
        }
    }

    /// 'inccommand' went off: an open `:` line's preview comes down at once,
    /// the view and the folds put back, whatever is left of it.
    pub(super) fn drop_substitute_preview(&mut self, cx: &mut Context<Self>) {
        let colon = matches!(
            self.vim.as_ref().map(VimState::mode),
            Some(fe_vim::Mode::CmdLine(CommandLineKind::Ex))
        );
        if colon && self.search_preview.is_some() {
            self.undo_search_preview();
            cx.notify();
        }
    }

    /// The decorations a `:s` line's preview paints, on the model now:
    /// each match's highlight and each changed row's text after its end.
    #[cfg(test)]
    pub(crate) fn substitute_preview_decorations(&self) -> Vec<Decoration> {
        let len = self.model.text().len();
        let mut found = self.model.decorations(0..len);
        found.retain(|decoration| {
            decoration.tint == Tint::Added
                && (matches!(decoration.kind, DecorationKind::AfterLine(_))
                    || decoration.kind == DecorationKind::Highlight)
        });
        found
    }
}

/// A highlight over each of `preview`'s matches in `text` (one of line
/// breaks alone, which the rows' text doesn't show, as an empty one where
/// they start, a space wide at its row's end), and after the end of each
/// row they change the row as it would read.
fn decorations(text: &Text, preview: &SubstitutePreview) -> Vec<Decoration> {
    let mut out: Vec<Decoration> = (preview.matches.iter())
        .map(|(range, _)| {
            let breaks_alone = !range.is_empty()
                && (text.chunks_in_range(range.clone()))
                    .all(|chunk| chunk.bytes().all(|byte| byte == b'\n'));
            let range = match breaks_alone {
                true => range.start..range.start,
                false => range.clone(),
            };
            Decoration::new(range, DecorationKind::Highlight, Tint::Added)
        })
        .collect();
    for (end, row) in rows_after(text, &preview.matches) {
        out.push(Decoration::new(
            end..end,
            DecorationKind::AfterLine(Arc::from(
                kiln_l10n::tr!("gpui-substitute-preview-row", row = row).as_ref(),
            )),
            Tint::Added,
        ));
    }
    out
}

/// Text cut at [`ROW_PREVIEW_CHARS`], with `…` where it was cut and `⏎` for
/// a line break, written no further than that.
#[derive(Default)]
struct Cut {
    shown: String,
    chars: usize,
    cut: bool,
}

impl Cut {
    fn push(&mut self, text: &str) {
        for ch in text.chars() {
            if self.chars == ROW_PREVIEW_CHARS {
                self.cut = true;
                return;
            }
            self.shown.push(if ch == '\n' { '⏎' } else { ch });
            self.chars += 1;
        }
    }

    fn finish(mut self) -> String {
        if self.cut {
            self.shown.push('…');
        }
        self.shown
    }
}

/// Each run of rows of `text` that `matches` change, rows a match joins
/// taken together: where its last row ends, and the rows as they would
/// read, cut ([`Cut`]). Rows the replacements leave as they were are left
/// out, by reading the rows with them against the rows as they are, as
/// they go.
fn rows_after(text: &Text, matches: &[(Range<usize>, String)]) -> Vec<(usize, String)> {
    let row = |at: usize| text.offset_to_point(at).row;
    let row_end = |row: u32| text.row_start(row) + text.row_len(row) as usize;
    let mut out = Vec::new();
    let mut each = matches.iter().peekable();
    while let Some(first) = each.next() {
        // The run: rows from the first match's through those its matches
        // reach.
        let mut run = vec![first];
        let mut last = row(first.0.end);
        while let Some(next) = each.next_if(|(range, _)| row(range.start) <= last) {
            last = last.max(row(next.0.end));
            run.push(next);
        }
        let (start, end) = (text.row_start(row(first.0.start)), row_end(last));
        let longer =
            |(range, new): &&(Range<usize>, String)| new.len() as isize - range.len() as isize;
        let same = run.iter().map(longer).sum::<isize>() == 0
            && (replaced(text, start..end, &run).flat_map(str::bytes))
                .eq(text.chunks_in_range(start..end).flat_map(str::bytes));
        if same {
            continue;
        }
        let mut shown = Cut::default();
        for piece in replaced(text, start..end, &run) {
            if shown.cut {
                break;
            }
            shown.push(piece);
        }
        out.push((end, shown.finish()));
    }
    out
}

/// `rows` of `text` with `run`'s matches (in order, within them) replaced,
/// as the pieces it reads, nothing copied.
fn replaced<'a>(
    text: &'a Text,
    rows: Range<usize>,
    run: &'a [&'a (Range<usize>, String)],
) -> impl Iterator<Item = &'a str> + 'a {
    let starts = std::iter::once(rows.start).chain(run.iter().map(|(range, _)| range.end));
    let ends = run.iter().map(|(range, _)| range.start).chain([rows.end]);
    let news = run.iter().map(|(_, new)| Some(new.as_str())).chain([None]);
    starts
        .zip(ends)
        .zip(news)
        .flat_map(move |((start, end), new)| text.chunks_in_range(start..end).chain(new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_after_joins_and_skips_unchanged_rows() {
        let text = Text::new("one two\nthree\nfour\n");
        let m = |range: Range<usize>, new: &str| (range, new.to_string());
        // Two on a row, and a row left as it was.
        let matches = [m(0..1, "O"), m(4..5, "T"), m(8..9, "t")];
        assert_eq!(rows_after(&text, &matches), [(7, "One Two".to_string())]);
        // Changes that cancel out: `aa`, the first `a` doubled and the
        // second gone.
        let aa = Text::new("aa\n");
        assert_eq!(rows_after(&aa, &[m(0..1, "aa"), m(1..2, "")]), []);
        assert_eq!(
            rows_after(&aa, &[m(0..1, "ab"), m(1..2, "")]),
            [(2, "ab".to_string())]
        );
        // A match joining rows, and one on the row it joins.
        let matches = [m(8..9, "T"), m(13..15, "_"), m(16..17, "U")];
        assert_eq!(rows_after(&text, &matches), [(18, "Three_oUr".to_string())]);
        // A line break put in, and a long row cut.
        let long = Text::new(&format!("{}\n", "a".repeat(200)));
        let previewed = decorations(
            &long,
            &SubstitutePreview {
                matches: vec![m(0..1, "b\nc")],
                truncated: false,
            },
        );
        let DecorationKind::AfterLine(after) = &previewed[1].kind else {
            panic!("{previewed:?}");
        };
        assert!(after.starts_with("→ b⏎c"), "{after}");
        assert!(after.ends_with("a…"), "{after}");
        assert_eq!(after.chars().count(), 2 + ROW_PREVIEW_CHARS + 1);
    }
}
