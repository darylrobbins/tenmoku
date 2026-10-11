use super::*;

/// An upper bound on what [`put_block`] inserts for `payload`'s rows,
/// `width` wide, `count` times at display `column`, a split grapheme's
/// spaces and a new row's line break included: `None` on overflow.
pub(crate) fn put_size(
    payload: &str,
    (width, count): (usize, usize),
    column: usize,
    tab: usize,
) -> Option<usize> {
    payload.split('\n').try_fold(0usize, |size, piece| {
        let copies = piece.len().checked_add(width)?.checked_mul(count)?;
        size.checked_add(copies)?
            .checked_add(column)?
            .checked_add(split(tab))?
            .checked_add(1)
    })
}

/// The most spaces a grapheme split by a block's edge leaves: a tab's or
/// the widest character's columns.
fn split(tab: usize) -> usize {
    tab.max(motion::widest())
}

/// An upper bound on what [`put()`] inserts: `payload`, a register of
/// `kind`'s width if blockwise, `count` times over a selection of `kind` on
/// `rows` rows from display `column`, with the spaces deleting a block
/// leaves; `None` on overflow.
fn put_bound(
    payload: &str,
    (width, kind): (Option<usize>, VisualKind),
    (rows, column): (usize, usize),
    count: usize,
    tab: usize,
) -> Option<usize> {
    let deleted = match kind {
        VisualKind::Block => rows.checked_mul(split(tab).checked_mul(2)?)?,
        _ => 0,
    };
    let put = match (width, kind) {
        (Some(_), VisualKind::Line) => payload.len().checked_add(1)?.checked_mul(count)?,
        (Some(width), _) => put_size(payload, (width, count), column, tab)?,
        (None, VisualKind::Block) => payload
            .len()
            .checked_mul(count)?
            .checked_add(split(tab))?
            .checked_mul(rows)?,
        (None, VisualKind::Line) => payload.len().checked_add(1)?.checked_mul(count)?,
        (None, _) => payload.len().checked_mul(count)?.checked_add(1)?,
    };
    deleted.checked_add(put)
}

/// Puts a block's `rows`, `width` columns wide, `count` times side by side at
/// display `column` from row `first` on, as `p` and `P` put a blockwise
/// register: each copy padded to the width when text follows it, rows short
/// of the column padded with spaces, a grapheme straddling it split, and
/// rows past the end of the text added. Returns the edits, where the first
/// and the last row's text start once they're in, and how long the last
/// row's is.
pub(crate) fn put_block(
    text: &Text,
    (first, column): (u32, usize),
    rows: &[&str],
    (width, count): (usize, usize),
    tab: usize,
) -> (Vec<(Range<usize>, String)>, [usize; 3]) {
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let (mut shift, mut starts) = (0isize, [0; 3]);
    let mut added = String::new();
    for (ix, piece) in rows.iter().enumerate() {
        let row = first.saturating_add(ix as u32);
        let (range, mut with, at, follows) = match row < text.row_count() {
            true => {
                let (range, spaces, at) =
                    insertion(text, row, (Some(column), false), tab, true).expect("a padded row");
                let follows = at < spaces || range.end < row_end(text, row);
                (range, spaced("", (spaces, at)), at, follows)
            }
            false => {
                let new = format!("\n{}", " ".repeat(column));
                (text.len()..text.len(), new, column + 1, false)
            }
        };
        // Padded to the width, each tab in it counted as a whole tab stop, as
        // nvim measures it.
        let line = Line::of(0, piece.to_string());
        let wide: usize = (0..line.len()).map(|ix| line.width(ix, 0, tab)).sum();
        let pad = width.saturating_sub(wide);
        let mut copies = String::new();
        for copy in 0..count {
            copies.push_str(piece);
            if copy + 1 < count || follows {
                copies.extend(std::iter::repeat_n(' ', pad));
            }
        }
        with.insert_str(at, &copies);
        starts[2] = copies.len();
        let start = match row < text.row_count() {
            true => range.start,
            false => text.len() + added.len(),
        };
        starts[1] = start.checked_add_signed(shift).expect("sorted edits") + at;
        if ix == 0 {
            starts[0] = starts[1];
        }
        match row < text.row_count() {
            true => {
                shift += with.len() as isize - range.len() as isize;
                edits.push((range, with));
            }
            false => added.push_str(&with),
        }
    }
    if !added.is_empty() {
        edits.push((text.len()..text.len(), added));
    }
    (edits, starts)
}

/// `vim.put` in a visual mode (`:help v_p`): the selection replaced by the
/// register, `count` times, as one transaction. `p` leaves what it replaced
/// in the registers as a delete would; `P` leaves them alone. Lines put over
/// characters go on rows of their own, and over a block below it (`P`:
/// above); a one-row text goes on every row of a block reaching its left
/// column. `'<` and `'>` go to the start and the last character of what
/// went in (on a block's first row, for text put on every row). The put is
/// taken in the text the delete leaves, and both go in together, so nothing
/// edits the buffer between them; one too big is refused before anything
/// changes, the visual mode included.
pub(crate) fn put(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: PutArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let VimTarget {
        model,
        host,
        state,
        registers,
    } = target;
    let value = registers
        .read(args.register, &mut **host)
        .map_err(|error| CommandError::new(error.to_string()))?;
    // `_` holds empty characters, not nothing.
    let unset = value.is_none() && args.register != Some('_');
    // nvim's E353, once the selection is replaced.
    state.failed |= unset;
    let value = value.unwrap_or_default();
    model.sync_display();
    let Some(ends) = ends(model, state) else {
        return effect(false);
    };
    // An unset register puts nothing, however many times: the count bounds
    // and repeats nothing, and the put leaves what the uncounted one does.
    let count = match unset {
        true => 1,
        false => args.count.unwrap_or(1).max(1) as usize,
    };
    let mut payload = value.text.replace("\r\n", "\n").replace('\r', "\n");
    if value.linewise && !payload.ends_with('\n') {
        payload.push('\n');
    }
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let (first, last) = ends.rows(&text);
    let cursor_row = text.offset_to_point(ends.cursor).row;
    let block = Block::of(&text, &ends, tab);
    // All it may put in, rows, copies, padding and split tabs included, is
    // bounded before anything changes.
    let column = match ends.kind {
        VisualKind::Block => block.left,
        _ => column_of(&text, ends.lo(), tab),
    };
    let rows = (last - first + 1) as usize;
    let size = put_bound(
        &payload,
        (value.block_width, ends.kind),
        (rows, column),
        count,
        tab,
    );
    if size.is_none_or(|size| size > operator::MAX_PUT) {
        return Err(CommandError::new(format!(
            "putting {count} copies of {} bytes would insert more than {} bytes",
            payload.len(),
            operator::MAX_PUT
        )));
    }
    // What's replaced, as a delete takes it (for `p`, which keeps it), where
    // the put goes once it's deleted: the selection's start (a block's left
    // column on its first row), and where undo puts the caret back.
    type Removed = Vec<(Range<usize>, String)>;
    let (deleted, removed, at, begin): (Option<Register>, Removed, usize, _) = match ends.kind {
        VisualKind::Block => {
            let cuts = block.cuts(&text, tab);
            let first = &cuts[0];
            let at = first.range.start + first.before * usize::from(!first.range.is_empty());
            let removed = cuts
                .iter()
                .filter(|cut| !cut.range.is_empty())
                .map(|cut| (cut.range.clone(), " ".repeat(cut.before + cut.after)))
                .collect();
            let deleted = match args.before {
                true => None,
                false => Some(block.register(&text, &cuts, tab)?),
            };
            (
                deleted,
                removed,
                at,
                (block.first, at - text.row_start(block.first)),
            )
        }
        kind => {
            let range = ends.range(&text, kind);
            // The selection's start, whatever the delete takes before it;
            // over lines with the cursor on the first of several, the
            // cursor, whose column nvim's undo keeps.
            let begin = match kind {
                VisualKind::Line if first < last && cursor_row == first => {
                    history::pair(&text, ends.cursor)
                }
                _ => history::pair(&text, range.start),
            };
            let deleted = (!args.before).then(|| {
                let mut taken = text.slice_to_string(range.clone());
                if kind == VisualKind::Line {
                    taken.push('\n');
                }
                Register::new(taken, kind == VisualKind::Line)
            });
            // With nothing in the register, whole lines go as `d` takes
            // them, a line break with them.
            let range = match (kind, unset) {
                (VisualKind::Line, true) if last + 1 < text.row_count() => {
                    range.start..text.row_start(last + 1)
                }
                (VisualKind::Line, true) if first > 0 => row_end(&text, first - 1)..range.end,
                _ => range,
            };
            let at = range.start;
            (deleted, vec![(range, String::new())], at, begin)
        }
    };
    // The text the delete leaves, which the put goes into. The delete
    // changes nothing before the selection's start, and a block's none of
    // its rows' line breaks: `at`, the first row and the block's stay.
    let mut emptied = text.clone();
    for (range, with) in removed.iter().rev() {
        emptied.replace(range.clone(), with);
    }
    let repeated = payload.repeat(count);
    let lines = |text: &str| text.strip_suffix('\n').unwrap_or(text).to_string();
    // The edits, where the put text starts and its last character (once
    // they're in), and whether the caret goes to the first non-blank.
    let (edits, start, end, first_non_blank) = match (value.block_width, ends.kind) {
        (Some(_), VisualKind::Line) => {
            let rows = lines(&(payload.clone() + "\n").repeat(count));
            (vec![(at..at, rows.clone())], at, at + rows.len(), true)
        }
        (Some(width), _) => {
            let rows: Vec<&str> = payload.split('\n').collect();
            let column = column_of(&emptied, at, tab);
            let (edits, [_, last, len]) =
                put_block(&emptied, (first, column), &rows, (width, count), tab);
            (edits, at, last + len, false)
        }
        (None, kind) if value.linewise => {
            // A line break that goes in before the lines, ending the row
            // they follow (not their own empty first row).
            let body = lines(&repeated);
            let (at, put, ending) = match kind {
                VisualKind::Char => (at, format!("\n{repeated}"), 1),
                VisualKind::Line => (at, body.clone(), 0),
                _ if args.before => (emptied.row_start(first), repeated.clone(), 0),
                _ => (row_end(&emptied, cursor_row), format!("\n{body}"), 1),
            };
            let start = at + ending;
            (vec![(at..at, put)], start, start + body.len(), true)
        }
        (None, VisualKind::Block) if !payload.contains('\n') => {
            let edits = (first..=last)
                .filter_map(|row| {
                    let place = (Some(block.left), false);
                    let (range, spaces, at) = insertion(&emptied, row, place, tab, false)?;
                    Some((range, spaced(&repeated, (spaces, at))))
                })
                .collect();
            (edits, at, at + repeated.len(), false)
        }
        // Over lines, each copy of characters on rows of its own.
        (None, kind) => {
            let blank = kind == VisualKind::Line;
            let put = match blank {
                true => {
                    let mut rows = format!("{payload}\n").repeat(count);
                    rows.pop();
                    rows
                }
                false => repeated.clone(),
            };
            (vec![(at..at, put.clone())], at, at + put.len(), blank)
        }
    };
    let extent = extent(&text, ends, ends.kind == VisualKind::Line, tab);
    leave(model, state, ends);
    let begins = vec![begin];
    let (transaction, snapshot) = operator::apply_steps(model, [removed, edits], origin(actor));
    if let Some(id) = transaction {
        state.carets.record(id, begins);
    }
    let after = snapshot.text().clone();
    // The first and last characters put in (a line break ending it is at
    // the next row's start).
    let start = Cursor::at(&after, start).offset();
    let end = match after.slice_to_string(start..end).ends_with('\n') {
        true => end,
        false => Cursor::at(&after, end.saturating_sub(1).max(start)).offset(),
    };
    let land = match (first_non_blank, value.block_width) {
        (true, _) => {
            let mut cursor = Cursor::at(&after, start);
            cursor.ix = cursor.line.first_non_blank();
            cursor.offset()
        }
        (false, Some(_)) => start,
        (false, None) if repeated.contains('\n') => start,
        (false, None) => end,
    };
    operator::land(model, &snapshot, [land]);
    if let Some(marks) = state.last_visual.as_mut() {
        marks.start = Pos::at(&snapshot, start);
        marks.end = Pos::at(&snapshot, end);
    }
    // `p` leaves what it replaced in the registers, as a delete would; a
    // delete of nothing writes nothing.
    if let Some(deleted) = deleted.filter(|deleted| deleted.linewise || !deleted.text.is_empty()) {
        let small = !deleted.linewise && !deleted.text.contains('\n');
        let write = Write::Delete {
            small,
            numbered: !small,
        };
        registers
            .write(None, write, deleted, &mut **host)
            .map_err(|error| CommandError::new(error.to_string()))?;
    }
    // `.` deletes as much text again, as nvim's does.
    state.set_last_change(
        Change::Operate(OperateArgs {
            operator: Operator::Delete,
            target: Target::Selection,
            count: None,
            register: args.before.then_some('_'),
        })
        .into(),
    );
    state.visual_extent = Some(extent);
    state.goals = None;
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}
