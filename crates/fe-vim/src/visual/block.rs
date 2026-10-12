use super::*;

/// A block: display columns `left` to `right` (`None` after `$`: to each
/// row's end) on rows `first` to `last`.
#[derive(Copy, Clone, Debug)]
pub(super) struct Block {
    pub(super) first: u32,
    pub(super) last: u32,
    pub(super) left: usize,
    pub(super) right: Option<usize>,
}

/// What a block takes on one row.
#[derive(Clone, Debug, Default)]
pub(super) struct Cut {
    /// The bytes of the graphemes it overlaps; empty at the row's end if
    /// none do.
    pub(super) range: Range<usize>,
    /// How many bytes a yank takes: those graphemes, but a grapheme partly
    /// inside it as spaces for its columns inside; on a row ending before
    /// its left edge, spaces as wide as the block (see [`Block::cuts`]).
    pub(super) taken: usize,
    /// The display columns it covers.
    pub(super) width: usize,
    /// The columns of graphemes partly inside it that lie before and after
    /// it, which a delete leaves as spaces.
    pub(super) before: usize,
    pub(super) after: usize,
    /// Whether the row reaches its right edge.
    pub(super) beyond: bool,
    /// Whether the row ends before its left edge.
    pub(super) short: bool,
}

impl Block {
    /// The block between `ends`: each end takes its grapheme's columns (a
    /// row's end is one column).
    pub(super) fn of(text: &Text, ends: &Ends, tab: usize) -> Self {
        let (first, last) = ends.rows(text);
        let columns = |offset| {
            let at = cursor_at(text, offset);
            let start = at.line.start_column(at.ix, tab);
            let width = match at.ix < at.line.len() {
                true => at.line.width(at.ix, start, tab).max(1),
                false => 1,
            };
            (start, start + width - 1)
        };
        let (anchor, cursor) = (columns(ends.anchor), columns(ends.cursor));
        Self {
            first,
            last,
            left: anchor.0.min(cursor.0),
            right: (ends.goal != Some(usize::MAX)).then_some(anchor.1.max(cursor.1)),
        }
    }

    /// What it takes on each of its rows. A row ending before its left edge
    /// takes spaces, as nvim's yank does: as wide as the block, or after `$`
    /// one wider.
    pub(super) fn cuts(&self, text: &Text, tab: usize) -> Vec<Cut> {
        let mut cuts: Vec<Cut> = (self.first..=self.last)
            .map(|row| self.cut(text, row, tab, None))
            .collect();
        if self.right.is_none() {
            let spaces = self.width(&cuts) + 1;
            for cut in cuts.iter_mut().filter(|cut| cut.short) {
                cut.taken = spaces;
            }
        }
        cuts
    }

    /// What it takes on `row`, pushing what a yank takes there onto `taken`
    /// if given (but a short row's spaces).
    pub(super) fn cut(
        &self,
        text: &Text,
        row: u32,
        tab: usize,
        mut taken: Option<&mut String>,
    ) -> Cut {
        let line = Line::new(text, row);
        let end = self.right.map_or(usize::MAX, |right| right + 1);
        let row_end = line.start + line.text.len();
        let mut cut = Cut {
            range: row_end..row_end,
            ..Cut::default()
        };
        let mut column = 0;
        for ix in 0..line.len() {
            let start = column;
            column += line.width(ix, start, tab);
            if start >= end {
                cut.beyond = true;
                break;
            }
            if column <= self.left {
                continue;
            }
            let (offset, grapheme) = (line.start + line.starts[ix], line.grapheme(ix));
            if cut.range.start == row_end {
                cut.range.start = offset;
            }
            cut.range.end = offset + grapheme.len();
            let inside = column.min(end) - start.max(self.left);
            cut.width += inside;
            let partly = start < self.left || column > end;
            if partly {
                cut.before += self.left.saturating_sub(start);
                cut.after += column.saturating_sub(end);
            }
            cut.taken += if partly { inside } else { grapheme.len() };
            if let Some(taken) = taken.as_deref_mut() {
                match partly {
                    true => taken.extend(std::iter::repeat_n(' ', inside)),
                    false => taken.push_str(grapheme),
                }
            }
        }
        cut.short = column < self.left;
        if let Some(right) = self.right.filter(|_| cut.short) {
            cut.taken = right - self.left + 1;
        }
        cut.beyond |= end <= column;
        cut
    }

    /// Its width: after `$`, the widest row's, at least one.
    fn width(&self, cuts: &[Cut]) -> usize {
        match self.right {
            Some(right) => right - self.left + 1,
            None => cuts.iter().map(|cut| cut.width).max().unwrap_or(0).max(1),
        }
    }

    /// What a yank of it writes, its rows joined, built only once its size
    /// is known: as wide as the block on short rows, it can be far larger
    /// than the text. More than [`operator::MAX_PUT`] bytes, which no put
    /// could put back, are refused.
    pub(super) fn register(
        &self,
        text: &Text,
        cuts: &[Cut],
        tab: usize,
    ) -> Result<Register, CommandError> {
        let size = cuts.iter().try_fold(0usize, |size, cut| {
            size.checked_add(cut.taken)?.checked_add(1)
        });
        if size.is_none_or(|size| size > operator::MAX_PUT) {
            return Err(CommandError::new(format!(
                "a block of {} rows would take more than {} bytes",
                cuts.len(),
                operator::MAX_PUT
            )));
        }
        let rows: Vec<String> = (self.first..)
            .zip(cuts)
            .map(|(row, cut)| {
                let mut taken = String::with_capacity(cut.taken);
                match cut.short {
                    true => taken.extend(std::iter::repeat_n(' ', cut.taken)),
                    false => _ = self.cut(text, row, tab, Some(&mut taken)),
                }
                taken
            })
            .collect();
        Ok(Register::block(&rows, self.width(cuts)))
    }

    /// The position at its top-left corner in `text`: the grapheme there,
    /// or the row's end.
    fn corner(&self, text: &Text, tab: usize) -> usize {
        match self.left < width_from(&text.row_to_string(self.first), 0, tab) {
            true => at_column(text, self.first, self.left, tab),
            false => row_end(text, self.first),
        }
    }
}

/// The normal-mode caret on `row` at display `column`: the grapheme covering
/// it, or the row's last.
pub(super) fn at_column(text: &Text, row: u32, column: usize, tab: usize) -> usize {
    let mut cursor = Cursor::at(text, text.row_start(row));
    cursor.vertical(row, Some(column), tab);
    cursor.offset()
}

/// The display column `offset` starts at on its row.
pub(super) fn column_of(text: &Text, offset: usize, tab: usize) -> usize {
    let at = cursor_at(text, offset);
    at.line.start_column(at.ix, tab)
}

/// How wide `text` is from display `column`.
pub(crate) fn width_from(text: &str, column: usize, tab: usize) -> usize {
    let line = Line::of(0, text.to_string());
    (0..line.len()).fold(column, |at, ix| at + line.width(ix, at, tab)) - column
}

/// Where text goes in at display `column` on `row` (`None`: at its end): the
/// bytes to replace, how many spaces replace them, and after how many of
/// those the text goes. A grapheme straddling the column takes the text
/// after it if `pass` (`A` on a block's first row, where nvim's cursor can't
/// stop inside it); otherwise a tab becomes spaces either side of it, as
/// nvim splits it, and before a wide character go spaces up to the column,
/// and the text. A row short of the column is padded with spaces if `pad`,
/// and otherwise has no place for it.
pub(crate) fn insertion(
    text: &Text,
    row: u32,
    (column, pass): (Option<usize>, bool),
    tab: usize,
    pad: bool,
) -> Option<(Range<usize>, usize, usize)> {
    let line = Line::new(text, row);
    let end = line.start + line.text.len();
    let Some(column) = column else {
        return Some((end..end, 0, 0));
    };
    let mut start = 0;
    for ix in 0..line.len() {
        let offset = line.start + line.starts[ix];
        let width = line.width(ix, start, tab);
        if start == column {
            return Some((offset..offset, 0, 0));
        }
        let end = offset + line.grapheme(ix).len();
        match line.grapheme(ix) {
            _ if start + width <= column => {}
            _ if pass => return Some((end..end, 0, 0)),
            "\t" => return Some((offset..end, width, column - start)),
            _ => return Some((offset..offset, column - start, column - start)),
        }
        start += width;
    }
    match start == column || pad {
        true => Some((end..end, column - start, column - start)),
        false => None,
    }
}

/// `typed` in `spaces` spaces, after `at` of them.
pub(crate) fn spaced(typed: &str, (spaces, at): (usize, usize)) -> String {
    let mut with = String::with_capacity(spaces + typed.len());
    with.extend(std::iter::repeat_n(' ', at));
    with.push_str(typed);
    with.extend(std::iter::repeat_n(' ', spaces - at));
    with
}

/// `>` or `<` on `row` of a block from display column `left`, by `by`
/// columns (`:help v_b_>`): `>` rewrites the blanks around `left` (those
/// before it, and a tab straddling it, included) as spaces `by` wider; `<`
/// removes up to `by` columns of the blanks from `left` on, keeping those
/// that still fit whole and a straddling character's columns before `left`
/// as spaces.
/// Empty rows and rows ending before `left` are left alone.
fn shift_block(
    text: &Text,
    row: u32,
    (left, by): (usize, usize),
    indent: bool,
    tab: usize,
) -> Option<(Range<usize>, String)> {
    let line = Line::new(text, row);
    let width = |ix: usize, column: usize| line.width(ix, column, tab);
    let byte = |ix: usize| line.start + line.starts.get(ix).copied().unwrap_or(line.text.len());
    let blank = |ix: usize| matches!(line.grapheme(ix), " " | "\t");
    // To the grapheme covering `left`, and the blanks from there.
    let mut columns = vec![0];
    let mut ix = 0;
    while ix < line.len() && columns[ix] + width(ix, columns[ix]) <= left {
        columns.push(columns[ix] + width(ix, columns[ix]));
        ix += 1;
    }
    if line.len() == 0 || (ix == line.len() && columns[ix] < left) {
        return None;
    }
    // A wide character straddling `left` is blanks to `<`, as to nvim: it
    // stays only if it still fits.
    let (mut blanks, mut end) = (ix, columns[ix]);
    if !indent && ix < line.len() && columns[ix] < left && !blank(ix) {
        end += width(ix, end);
        blanks += 1;
    }
    while blanks < line.len() && blank(blanks) {
        end += width(blanks, end);
        blanks += 1;
    }
    if indent {
        let mut from = ix;
        while from > 0 && blank(from - 1) {
            from -= 1;
        }
        let spaces = " ".repeat(end - columns[from] + by);
        return Some((byte(from)..byte(blanks), spaces));
    }
    let to = end - end.saturating_sub(left.max(columns[ix])).min(by);
    let (mut kept, mut at) = (ix, columns[ix]);
    while kept < blanks && at + width(kept, at) <= to {
        at += width(kept, at);
        kept += 1;
    }
    let before = match kept == ix && at < left && blanks > ix {
        true => left - std::mem::replace(&mut at, left),
        false => 0,
    };
    let range = byte(kept)..byte(blanks);
    let spaces = " ".repeat(before + to - at);
    (!range.is_empty() || !spaces.is_empty()).then_some((range, spaces))
}

/// `vim.operate` over a block, but for `S`, `R` and `J`, which take its rows
/// as the other kinds do: each operator acts on what the block takes on
/// each row ([`Block::cut`], `:help blockwise-operators`), `D` and `C` to
/// the rows' ends, and leaves the caret on its top-left corner; `c`, `I` and
/// `A` open an insert session on its first row, whose text `Esc` types on
/// the others.
pub(super) fn operate_block(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    ends: Ends,
    columns: Option<(usize, Option<usize>)>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    if let Some(name) = args.register {
        Registers::check(name).map_err(|error| CommandError::new(error.to_string()))?;
    }
    let VimTarget {
        model,
        host,
        state,
        registers,
    } = target;
    let text = model.text().clone();
    let tab = model.tab_size().max(1);
    let mut block = Block::of(&text, &ends, tab);
    if let Some((left, width)) = columns {
        block.left = left;
        block.right = width.map(|width| left + width - 1);
    }
    if args.target == Target::SelectedToEnd {
        block.right = None;
    }
    let cuts = block.cuts(&text, tab);
    let rows = block.last - block.first + 1;
    let extent = Extent::Block {
        rows,
        columns: block.right.map(|right| right - block.left + 1),
    };
    let empty = rows == 1 && cuts[0].taken == 0;
    let write = match args.operator {
        Operator::Yank => Some(Write::Yank),
        // A delete of nothing writes nothing.
        Operator::Delete if empty => None,
        Operator::Delete | Operator::Change => Some(Write::Delete {
            small: rows == 1,
            numbered: rows > 1,
        }),
        _ => None,
    };
    // What it writes, before anything changes (`_` writes nothing).
    let write = match write {
        Some(write) if args.register != Some('_') => {
            Some((write, block.register(&text, &cuts, tab)?))
        }
        _ => None,
    };
    leave(model, state, ends);
    state.goals = None;
    if let Some((write, value)) = write {
        registers
            .write(args.register, write, value, &mut **host)
            .map_err(|error| CommandError::new(error.to_string()))?;
    }
    let cut_rows = || cuts.iter().filter(|cut| !cut.range.is_empty());
    let deletes = || {
        cut_rows()
            .map(|cut| (cut.range.clone(), " ".repeat(cut.before + cut.after)))
            .collect::<Vec<_>>()
    };
    let edits: Vec<(Range<usize>, String)> = match args.operator {
        Operator::Delete => deletes(),
        // Each column becomes `ch`, as many as fit of a wide one, the rest
        // spaces where the row goes on past the block.
        Operator::Replace(ch) => cut_rows()
            .map(|cut| {
                // A tab fills one column, as nvim's `r<Tab>` does.
                let wide = match ch {
                    '\t' => 1,
                    ch => width_from(ch.encode_utf8(&mut [0; 4]), 0, tab).max(1),
                };
                let rest = match cut.beyond {
                    true => cut.width % wide,
                    false => 0,
                };
                let mut with = " ".repeat(cut.before);
                with.extend(std::iter::repeat_n(ch, cut.width / wide));
                with.extend(std::iter::repeat_n(' ', rest + cut.after));
                (cut.range.clone(), with)
            })
            .collect(),
        Operator::ToggleCase | Operator::Lowercase | Operator::Uppercase | Operator::Rot13 => {
            cut_rows()
                .filter_map(|cut| {
                    let old = text.slice_to_string(cut.range.clone());
                    let new = recased_text(&old, args.operator);
                    (new != old).then(|| (cut.range.clone(), new))
                })
                .collect()
        }
        Operator::Indent | Operator::Outdent => {
            let by = tab.saturating_mul(args.count.unwrap_or(1).max(1) as usize);
            let indent = args.operator == Operator::Indent;
            (block.first..=block.last)
                .filter_map(|row| shift_block(&text, row, (block.left, by), indent, tab))
                .collect()
        }
        Operator::Change | Operator::Insert | Operator::Append => {
            // `c` deletes the block and types at its left column on the rows
            // reaching it; `I` types there, `A` after the block (padding
            // short rows), each making room on the first row.
            let (column, pad) = match args.operator {
                Operator::Append => (block.right.map(|right| right + 1), true),
                _ => (Some(block.left), false),
            };
            let opening = match (args.operator, &cuts[0]) {
                (Operator::Change, first) if !first.range.is_empty() => {
                    (deletes(), first.range.start + first.before)
                }
                (Operator::Change, first) => (deletes(), first.range.start),
                _ => match insertion(&text, block.first, (column, pad), tab, pad) {
                    Some((range, spaces, at)) => {
                        let start = range.start;
                        (vec![(range, spaced("", (spaces, at)))], start + at)
                    }
                    None => (Vec::new(), row_end(&text, block.first)),
                },
            };
            // A tab or wide character straddling `I`'s column on the first
            // row takes the text before it, and nvim then types it on no
            // other row. `A` on the first row goes after a grapheme straddling
            // its column, and nvim types that on the others too.
            let prefix = match (&opening.0[..], column) {
                ([(range, with)], Some(column))
                    if with.is_empty() && column_of(&text, range.start, tab) > column =>
                {
                    let passed = Cursor::at(&text, range.start - 1);
                    passed.line.grapheme(passed.ix).to_string()
                }
                _ => String::new(),
            };
            let straddled = match (args.operator, &opening.0[..]) {
                (Operator::Insert, [(range, with)]) => {
                    !with.is_empty() && range.start < row_end(&text, block.first)
                }
                _ => false,
            };
            let (opening, others) = match straddled {
                true => (
                    (Vec::new(), opening.0[0].0.start),
                    block.first + 1..=block.first,
                ),
                false => (opening, block.first + 1..=block.last),
            };
            let corner = (args.operator != Operator::Change && !straddled).then_some(block.left);
            let edge = match (args.operator, column) {
                (Operator::Change, _) => insert::Edge::Fixed,
                (Operator::Insert, _) => insert::Edge::Left,
                (_, Some(_)) => insert::Edge::Right,
                (_, None) => insert::Edge::End(
                    (block.first..=block.last)
                        .map(|row| width_from(&text.row_to_string(row), 0, tab))
                        .max()
                        .unwrap_or(0),
                ),
            };
            // The other rows' places, in the text the opening edits leave.
            let rows = move |opened: &BufferSnapshot| insert::BlockRows {
                rows: others
                    .filter_map(|row| block_row(opened, row, (column, pad), tab))
                    .collect(),
                prefix,
                corner,
                text: opened.text().clone(),
                edge,
                tab,
            };
            // `c` types once, `I` and `A` as many times as their count.
            let count = match args.operator {
                Operator::Change => 1,
                _ => args.count.unwrap_or(1).max(1),
            };
            let change = (Change::Operate(args), count);
            insert::block(model, state, origin(actor), change, opening, rows);
            if !state.replaying {
                state.visual_extent = Some(extent);
            }
            return Ok(Dispatched::new(Effect::Changed));
        }
        Operator::Yank | Operator::Join => Vec::new(),
        Operator::Format | Operator::FormatKeep => unreachable!("g_operators' own"),
    };
    let corner = block.corner(&text, tab);
    let begins = vec![history::pair(&text, corner)];
    // `u U ~ g?` make an undo step even when nothing changes, as in nvim.
    let (transaction, snapshot) = match args.operator {
        Operator::ToggleCase | Operator::Lowercase | Operator::Uppercase | Operator::Rot13 => {
            crate::g_operators::apply(model, edits, actor)
        }
        _ => operator::apply(model, edits, origin(actor)),
    };
    if let Some(id) = transaction {
        state.carets.record(id, begins);
    }
    // The caret goes to the corner's byte column as it was, as nvim's does
    // (after `>` and `<` even past the row's new end), but after a delete to
    // where the block's left column now is.
    let after = snapshot.text();
    let column = match (args.operator, &cuts[0]) {
        (Operator::Delete, first) if !first.range.is_empty() => {
            first.range.start + first.before - text.row_start(block.first)
        }
        _ => corner - text.row_start(block.first),
    };
    let at = after.row_start(block.first) + column.min(after.row_len(block.first) as usize);
    match args.operator {
        Operator::Indent | Operator::Outdent => {
            let at = snapshot.anchor_after(at);
            let head = model.snapshot().resolve(&at);
            model.set_selections([operator::caret(head)]);
        }
        _ => operator::land(model, &snapshot, [at]),
    }
    if args.operator != Operator::Yank {
        state.set_last_change(Change::Operate(args).into());
        if !state.replaying {
            state.visual_extent = Some(extent);
        }
    }
    Ok(Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect()))
}

/// Where `Esc` types on `row` of a block: as [`insertion`] places `column`
/// in `snapshot`, the text a session's opening edits leave, `pad`ding a
/// short row (see [`insert::BlockRow`]).
fn block_row(
    snapshot: &BufferSnapshot,
    row: u32,
    (column, pad): (Option<usize>, bool),
    tab: usize,
) -> Option<insert::BlockRow> {
    let text = snapshot.text();
    let (range, spaces, at) = insertion(text, row, (column, false), tab, pad)?;
    let (start, end) = (text.row_start(row), row_end(text, row));
    let ending = (row + 1 < text.row_count())
        .then(|| snapshot.anchor_after(end)..snapshot.anchor_before(end + 1));
    Some(insert::BlockRow {
        ends: snapshot.anchor_after(start)..snapshot.anchor_before(end),
        ending,
        text: start..end,
        place: range.start - start..range.end - start,
        spaces: (spaces, at),
    })
}

/// `text` with each grapheme's first character as `~`, `u`, `U` or `g?` leave it.
pub(super) fn recased_text(text: &str, operator: Operator) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    text.graphemes(true)
        .flat_map(|grapheme| {
            let mut chars = grapheme.chars();
            let first = chars.next().map(|ch| recased(ch, operator));
            first.into_iter().chain(chars)
        })
        .collect()
}
