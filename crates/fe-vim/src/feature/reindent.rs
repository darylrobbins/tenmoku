//! The `=` operator (a feature module; see [`crate::feature`]): re-indents
//! the rows a motion, text object or visual selection covers
//! ([`Operator::Reindent`]) as the host suggests for the buffer's language
//! (`EditorHost::suggest_indent`), as VSCodeVim and IdeaVim map it to their
//! editor's reindent and nvim runs a filetype's 'indentexpr'.
//!
//! `=` starts the operator, or in a visual mode acts on the selection's
//! rows (a block's too). Doubled (`==`) it takes `count` rows. It acts on
//! whole rows as `>` does: those the target's ends are on, the last being
//! the row before when an exclusive motion ended at its column 0.
//!
//! Each row, top to bottom, takes the indentation the host suggests for it
//! in the text as the rows above have just been re-indented, made as the
//! editor's line break makes it (`EditorModel::newline_in`): a level is the
//! buffer's indent unit, and a level out is that many fewer columns, in
//! tabs as far as they go if the buffer indents with tabs. A row the host
//! has no suggestion for keeps its indentation: fastedit has no C-indenting
//! fallback, as nvim has with no 'indentexpr' (a known difference). A row
//! that's empty or holds only blanks is left empty, as in nvim.
//!
//! The host is asked about every row in the text as `=` found it, so the
//! language parses it once, and a suggestion's basis row is read as it has
//! just been re-indented: what asking after each row would give, as a
//! suggestion is relative to its basis row's indentation.
//!
//! One undo step, none when nothing changes; the caret goes to the first
//! non-blank of the first row, as Vim's does with 'startofline' (nvim's,
//! with its default 'nostartofline', keeps the goal column there: a known
//! difference); `.` repeats it over as many rows; marks stay on their rows,
//! and `'[` and `']` span them as after `>`.

use std::collections::HashMap;

use fe_buffer::{BufferSnapshot, Origin, TransactionId};
use fe_editor::{EditorModel, IndentStyle, IndentSuggestion, commands::EditorHost};
use fe_text::Text;

use crate::{
    Motion, Operator, Pending, Step, Target, VimKey,
    feature::Feature,
    operator::{self, indent_fill, indent_width},
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    ..Feature::NONE
};

/// `=`: starts the operator, or with it pending takes `count` rows (`==`);
/// in a visual mode it acts on the selection. With another operator
/// pending (`d=`) it's not this feature's, and cancels.
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    if key != VimKey::Char('=') {
        return None;
    }
    let pending = pending.clone();
    Some(match pending.operator() {
        _ if pending.is_visual() => pending.operate(Operator::Reindent, Target::Selection),
        None => pending.with_operator(Operator::Reindent),
        Some(Operator::Reindent) => {
            pending.operate(Operator::Reindent, Target::Motion(Motion::Line))
        }
        Some(_) => return None,
    })
}

/// Re-indents `rows` (sorted, each once) as `host` suggests, in one
/// transaction with `origin`: the transaction, if anything changed, and the
/// snapshot right after (see `operator::apply`).
pub(crate) fn rows(
    model: &mut EditorModel,
    host: &dyn EditorHost,
    rows: &[u32],
    origin: Origin,
) -> (Option<TransactionId>, BufferSnapshot) {
    let levels = Levels::of(model);
    operator::apply_plan(model, origin, |snapshot| {
        let text = snapshot.text();
        // Each row's indentation as re-indented so far.
        let mut planned: HashMap<u32, String> = HashMap::new();
        let mut edits = Vec::new();
        for &row in rows.iter().filter(|&&row| row < text.row_count()) {
            let line = text.row_to_string(row);
            let blanks = blank_len(&line);
            let indent = match blanks == line.len() {
                true => String::new(),
                false => match host.suggest_indent(snapshot, row) {
                    Some(suggestion) => levels.indent(text, suggestion, &planned),
                    None => continue,
                },
            };
            if line[..blanks] != indent {
                let start = text.row_start(row);
                edits.push((start..start + blanks, indent.clone()));
            }
            planned.insert(row, indent);
        }
        edits
    })
}

/// How the buffer indents: a level's text (`EditorModel::indent_unit`), a
/// level's columns, the tab size, and whether it indents with tabs.
struct Levels {
    unit: String,
    width: usize,
    tab: usize,
    tabs: bool,
}

impl Levels {
    fn of(model: &EditorModel) -> Self {
        let tab = model.tab_size().max(1);
        let style = model.indent_style();
        Self {
            unit: model.indent_unit(),
            width: match style {
                Some(IndentStyle::Spaces(n)) => usize::from(n),
                Some(IndentStyle::Tabs) | None => tab,
            },
            tab,
            tabs: style == Some(IndentStyle::Tabs),
        }
    }

    /// The indentation `suggestion` gives in `text`: a level deeper is the
    /// basis row's indentation and the indent unit; a level out is that
    /// many fewer columns. The basis row's indentation is as `planned`, if
    /// there.
    fn indent(
        &self,
        text: &Text,
        suggestion: IndentSuggestion,
        planned: &HashMap<u32, String>,
    ) -> String {
        let IndentSuggestion::Relative { basis_row, delta } = suggestion else {
            return String::new();
        };
        let basis = match planned.get(&basis_row) {
            Some(indent) => indent.clone(),
            None if basis_row < text.row_count() => {
                let line = text.row_to_string(basis_row);
                line[..blank_len(&line)].to_owned()
            }
            None => String::new(),
        };
        let count = delta.unsigned_abs() as usize;
        if delta >= 0 {
            return basis + &self.unit.repeat(count);
        }
        let columns = indent_width(&basis, self.tab).saturating_sub(self.width * count);
        indent_fill(columns, self.tab, self.tabs)
    }
}

/// The byte length of `line`'s leading blanks.
fn blank_len(line: &str) -> usize {
    line.bytes()
        .take_while(|&byte| byte == b' ' || byte == b'\t')
        .count()
}
