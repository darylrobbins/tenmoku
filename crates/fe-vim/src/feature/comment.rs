//! Commenting (a feature module; see [`crate::feature`]): `gc{motion}`,
//! `gcc` and visual `gc` ([`Operator::Comment`]) toggle the line comment on
//! the rows they cover, and `gc` after an operator ([`TextObject::Comment`])
//! is the run of commented rows around the caret, as nvim 0.12's built-in
//! commenting does (`:help commenting`).
//!
//! The toggle is the editor's, `EditorModel::toggle_comment`, from the
//! host's [`EditorHost::comment_tokens`] (the buffer's language), over
//! exactly the rows covered: if every non-blank one is commented it
//! uncomments them, taking the token and one space after it; otherwise it
//! puts the token and a space at the smallest indentation among them. It's
//! one undo step, which `.` repeats, and `u` puts the caret back where the
//! target starts. Rows are whole, as `>` takes them: a charwise motion
//! takes the rows it spans, the last being the row before when an
//! exclusive motion ended at its column 0 (`:help exclusive`).
//! `{count}gcc` takes `count` rows, as `{count}gUU` does. The caret stays
//! where the target starts (nvim's `'[`), at that row and byte column once
//! toggled: `gcc` on the row's first non-blank, if the caret was past it; a
//! visual `gc` at the start of a charwise selection, at a block's top-left
//! corner, or for rows on the first row's start (on the cursor if it's
//! above the anchor). `'[` and `']` go to where the target starts and
//! ends: a charwise one's first and last characters; for rows from a
//! linewise motion, the caret and where the motion lands, in order; for an
//! object's rows, the first and last rows' starts; over a selection its
//! ends, a linewise one's anchor at its row's start, a block's at its
//! corners. They and the other marks keep their rows and byte columns
//! through the toggle, as nvim's `lockmarks` keeps them. An empty object
//! (`i"` in `""`) toggles nothing, as nvim's `gc` leaves it.
//!
//! The object selects its rows as nvim's does, `{first}GV{last}G`, and the
//! operator then acts on that selection as on any (`zf` folds it), so `'<`
//! and `'>` take it (and move as a visual change moves them); a row counts as commented if its text
//! starts with the line token, or with neither, starts and ends with the
//! block pair. `.` finds the run again. Not on a commented row, it does
//! nothing.
//!
//! `gc` starts the operator, so its target goes through all of
//! operator-pending: counts, `/` and `?`, and `omap`s. `c` with it pending
//! makes `gcc` (the reducer hands an operator key under another operator
//! to the features), and is read unmapped, as nvim maps `gcc` whole in
//! normal mode.
//!
//! With no tokens (no language, or one without comments) none of them does
//! anything and leaves no undo step, as the editor's command doesn't; a
//! visual `gc` still leaves the visual mode.
//!
//! Where the editor's toggle differs from nvim's, fastedit keeps its own,
//! and the oracle corpus (`tests/corpus/commentary.toml`) leaves the cases
//! out:
//!
//! - Blank rows: nvim comments a blank row among others as the trimmed
//!   token at the smallest indentation (rows `a`, an empty one and `b`
//!   become `# a`, `#` and `# b`), and leaves rows that are all blank as
//!   they are; fastedit leaves blank rows among others as they are, and
//!   when every row is blank puts the token and a space at the end of each,
//!   as VS Code does.
//! - Uncommenting a row with nothing after the token (`  #`): nvim empties
//!   the row; fastedit keeps its indentation.
//! - Mixed indentation: nvim puts the smallest indentation (the fewest
//!   bytes) on every row, in place of as many bytes of its own, then the
//!   token (`\tfoo` and four spaces then `bar` become `\t# foo` and
//!   `\t#    bar`); fastedit puts the token after as many characters of
//!   each row's own indentation as the smallest has (` #    bar`).
//! - Where the tokens come from: nvim reads 'commentstring' (a filetype's,
//!   or inside an injected tree-sitter language, that language's at the
//!   cursor); fastedit takes the buffer language's tokens, a line token if
//!   it has one, else its block pair.
//! - A language with only a block pair: fastedit wraps each row's text
//!   where that row's text starts (`/* x */`), where nvim, given such a
//!   'commentstring', puts the opener at the smallest indentation among the
//!   rows and wraps blank rows too (`/**/`).
//! - nvim with an empty 'commentstring' warns and rewrites the rows as they
//!   are (an undo step); fastedit does nothing.
//! - Several carets (fastedit's own): their rows toggle together, as one
//!   set, as the editor's command toggles the rows of every selection.

use fe_buffer::{BufferSnapshot, Origin, TransactionId};
use fe_command::{Actor, CommandError, Dispatched, Effect};
use fe_editor::{
    CommentTokens, EditorModel,
    commands::{EditorHost, origin},
};
use fe_text::{Bias, Text};

use kiln_l10n::tr;

use crate::{
    Change, Motion, OperateArgs, Operator, Pending, Step, Target, TextObject, VimKey, VimTarget,
    feature::Feature,
    history, marks,
    operator::{self, Span},
    visual,
};

pub(crate) const FEATURE: Feature = Feature {
    key,
    g,
    ..Feature::NONE
};

/// `c` after `g`: over the selection in a visual mode, the object after an
/// operator, or else starts the operator.
fn g(pending: &Pending, key: VimKey) -> Option<Step> {
    if key != VimKey::Char('c') {
        return None;
    }
    let pending = pending.clone();
    Some(match pending.operator() {
        _ if pending.is_visual() => pending.operate(Operator::Comment, Target::Selection),
        None => pending.with_operator(Operator::Comment),
        Some(operator) => pending.operate(operator, Target::TextObject(TextObject::Comment)),
    })
}

/// `c` with `gc` pending: `count` rows (`gcc`).
fn key(pending: &Pending, key: VimKey) -> Option<Step> {
    (key == VimKey::Char('c')
        && !pending.is_visual()
        && pending.operator() == Some(Operator::Comment))
    .then(|| {
        pending
            .clone()
            .operate(Operator::Comment, Target::Motion(Motion::Line))
    })
}

/// The host's comment tokens, if it has a line token or a block pair.
pub(crate) fn tokens(host: &dyn EditorHost) -> Option<CommentTokens> {
    host.comment_tokens()
        .filter(|tokens| tokens.line.is_some() || tokens.block.is_some())
}

/// Toggles the comment on `rows` (sorted, each once) as
/// `EditorModel::toggle_comment` toggles the rows selections touch, with
/// `origin`: the transaction, if it changed anything, and the snapshot
/// right after. With no tokens it changes nothing.
pub(crate) fn rows(
    model: &mut EditorModel,
    host: &dyn EditorHost,
    rows: &[u32],
    origin: Origin,
) -> (Option<TransactionId>, BufferSnapshot) {
    let Some(tokens) = tokens(host) else {
        return (None, model.snapshot().clone());
    };
    // A caret at the start of each row, which touches it even when empty.
    let text = model.text().clone();
    model.set_selections(
        rows.iter()
            .filter(|&&row| row < text.row_count())
            .map(|&row| operator::caret(text.row_start(row))),
    );
    let (snapshot, transactions) = model.run_as(origin, |model| {
        model.toggle_comment(tokens);
        model.snapshot().clone()
    });
    (transactions.last().copied(), snapshot)
}

/// Where `gc` over `span` starts and ends, as nvim's `'[` and `']`, for
/// the caret at `head`: a charwise span's first and last characters; for
/// rows from a linewise motion, the caret and where the motion `reached`,
/// in order; for an object's rows (nothing `reached`), the first and last
/// rows' starts. The caret stays at the start.
pub(crate) fn ends(
    text: &Text,
    span: &Span,
    head: usize,
    reached: Option<usize>,
) -> Option<(usize, usize)> {
    match span {
        Span::Chars(range) => {
            let last = text.clip_offset(range.end.max(range.start + 1) - 1, Bias::Left);
            Some((range.start, last.max(range.start)))
        }
        Span::Lines { rows, from, .. } => {
            let last = text.row_start(*rows.end());
            Some(match (from, reached) {
                (Some(from), Some(reached)) => (*from, reached.max(head)),
                (Some(from), None) => (*from, last),
                (None, Some(reached)) => (reached.min(head), reached.max(head)),
                (None, None) => (text.row_start(*rows.start()), last),
            })
        }
        Span::Move(_) | Span::Failed => None,
    }
}

/// `gc` over a visual selection: its rows, leaving the visual mode with the
/// caret where an operator over it starts, which then stays at that row and
/// byte column.
pub(crate) fn selection(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    target.model.sync_display();
    let Some(area) = visual::live_area(target.model, target.state) else {
        return Err(CommandError::new(tr!("vim-no-visual-selection-to-comment")));
    };
    let (Some(first), Some(last)) = (area.rows.first(), area.rows.last()) else {
        return Err(CommandError::new(tr!("vim-no-visual-selection-to-comment")));
    };
    let shifted: Vec<u32> = (first.0..=last.0).collect();
    let marked = visual::operator_ends(target.model, target.state);
    visual::leave_for_ex(target);
    let VimTarget {
        model, host, state, ..
    } = target;
    if tokens(&**host).is_none() {
        return Ok(Dispatched::new(Effect::Changed));
    }
    let text = model.text().clone();
    let begin = history::pair(&text, model.primary_selection().head());
    let (transaction, snapshot) = rows(model, &**host, &shifted, origin(actor));
    let after = snapshot.text();
    let (row, column) = begin;
    let at = after.row_start(row) + column.min(after.row_len(row) as usize);
    operator::land(model, &snapshot, [at]);
    state.goals = None;
    if let Some(id) = transaction {
        state.carets.record(id, vec![begin]);
    }
    if let Some((start, end)) = marked {
        let version = transaction.map(|_| snapshot.version());
        let ends = (history::pair(&text, start), history::pair(&text, end));
        marks::recased(state, model, version, ends.0, ends.1);
    }
    state.set_last_change(Change::Operate(args).into());
    if !state.replaying {
        state.visual_extent = Some(area.extent);
    }
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// The run of commented rows around `row`, if it's commented.
fn run(text: &Text, row: u32, tokens: CommentTokens) -> Option<(u32, u32)> {
    let commented = |row: u32| commented(&text.row_to_string(row), tokens);
    if !commented(row) {
        return None;
    }
    let mut first = row;
    while first > 0 && commented(first - 1) {
        first -= 1;
    }
    let mut last = row;
    while last + 1 < text.row_count() && commented(last + 1) {
        last += 1;
    }
    Some((first, last))
}

/// Whether a row's text is commented: it starts with the line token, or
/// with none, starts and ends with the block pair, past blanks.
fn commented(row: &str, tokens: CommentTokens) -> bool {
    let code = row.trim();
    match (tokens.line, tokens.block) {
        (Some(token), _) => code.starts_with(token),
        (None, Some((open, close))) => {
            code.len() >= open.len() + close.len()
                && code.starts_with(open)
                && code.ends_with(close)
        }
        (None, None) => false,
    }
}

/// `{operator}gc`: selects the run of commented rows around the primary
/// caret as nvim's object does and applies the operator over that
/// selection, `.` finding the run again. Off a commented row, or with no
/// tokens, it does nothing.
pub(crate) fn operate_object(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let Some(tokens) = tokens(&*target.host) else {
        return Ok(Dispatched::new(Effect::Nothing));
    };
    target.model.sync_display();
    let text = target.model.text().clone();
    let row = text
        .offset_to_point(target.model.primary_selection().head())
        .row;
    let Some((first, last)) = run(&text, row, tokens) else {
        return Ok(Dispatched::new(Effect::Nothing));
    };
    let extent = target.state.visual_extent;
    let changes = target.state.changes;
    visual::select_rows(target.model, target.state, first, last);
    let selected = OperateArgs {
        target: Target::Selection,
        ..args
    };
    // As any operator over a selection: `zf` folds it.
    let dispatched = operator::operate(target, actor, selected);
    if dispatched.is_err() {
        visual::exit(target)?;
    }
    // `.` repeats the operator over the object, not over as many rows.
    let state = &mut *target.state;
    if let Some(session) = state.session.as_mut() {
        session.change = Change::Operate(args);
    }
    if state.changes != changes
        && let Some(last) = state.last_change.as_mut()
    {
        last.change = Change::Operate(args);
    }
    state.visual_extent = extent;
    dispatched
}
