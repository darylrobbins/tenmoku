use super::*;

/// What a change over `ends` took, for `.`.
pub(super) fn extent(text: &Text, ends: Ends, lines: bool, tab: usize) -> Extent {
    let (first, last) = ends.rows(text);
    let rows = last - first + 1;
    if lines {
        return Extent::Lines(rows);
    }
    if ends.kind == VisualKind::Block {
        let block = Block::of(text, &ends, tab);
        let columns = block.right.map(|right| right - block.left + 1);
        return Extent::Block { rows, columns };
    }
    let (lo, hi) = (cursor_at(text, ends.lo()), cursor_at(text, ends.hi()));
    let column = |cursor: &Cursor| cursor.line.start_column(cursor.ix, tab);
    // Past the end of what's at the end: a row's end is one column.
    let end = match hi.ix < hi.line.len() {
        true => hi.line.start_column(hi.ix + 1, tab),
        false => column(&hi) + 1,
    };
    let last = match (ends.goal == Some(usize::MAX), rows) {
        (true, _) => None,
        (false, 1) => Some(end - column(&lo)),
        (false, _) => Some(end),
    };
    Extent::Chars { rows, last }
}

/// The ends `extent` takes from a normal-mode caret at `head`, whose goal
/// column is `goal`.
fn extent_from(text: &Text, head: usize, goal: Option<usize>, extent: &Extent, tab: usize) -> Ends {
    let mut cursor = cursor_at(text, head);
    let last_row = text.row_count() - 1;
    let (kind, goal) = match *extent {
        // At the caret's goal column, which a linewise delete goes back to.
        Extent::Lines(rows) => {
            let row = cursor.row.saturating_add(rows - 1).min(last_row);
            let goal = cursor.vertical(row, goal, tab);
            (VisualKind::Line, goal)
        }
        // After a one-row `$` change, nvim 0.12.4's `.` goes to the row's
        // end from display column 0 or 1, and from further along (the
        // caret's column; on a tab, its last) takes the row from its start to
        // the caret.
        Extent::Chars {
            rows: 1,
            last: None,
        } if cursor.line.goal_column(cursor.ix, tab) >= 2 => {
            let row_start = text.row_start(cursor.row);
            return Ends {
                kind: VisualKind::Char,
                anchor: head,
                cursor: row_start,
                goal: None,
            };
        }
        // To the grapheme covering the last column, past the row's text on
        // its end. Rows past the end of the text stop at the last.
        // The same columns from the caret's, or after `$` to each row's end.
        // From a caret whose goal is the row's end, to each row's end.
        Extent::Block { rows, columns } => {
            let columns = columns.filter(|_| goal != Some(usize::MAX));
            let row = cursor.row.saturating_add(rows - 1).min(last_row);
            let from = cursor.line.start_column(cursor.ix, tab);
            let column = columns.map_or(usize::MAX, |columns| from + columns - 1);
            cursor.vertical(row, Some(column), tab);
            (VisualKind::Block, columns.is_none().then_some(usize::MAX))
        }
        Extent::Chars { rows, last } => {
            let from = match rows {
                1 => cursor.line.start_column(cursor.ix, tab),
                _ => 0,
            };
            let row = cursor.row.saturating_add(rows - 1).min(last_row);
            let column = last.map(|columns| from + columns - 1);
            cursor.vertical(row, Some(column.unwrap_or(usize::MAX)), tab);
            if column
                .is_none_or(|column| column >= cursor.line.start_column(cursor.line.len(), tab))
            {
                cursor.ix = cursor.line.len();
            }
            (VisualKind::Char, last.is_none().then_some(usize::MAX))
        }
    };
    Ends {
        kind,
        anchor: head,
        cursor: cursor.offset(),
        goal,
    }
}

/// What a visual selection covers, for a change that reads the text of each
/// row.
pub(crate) struct Area {
    /// The bytes it covers on each row it spans, as offsets (empty where it
    /// covers none).
    pub(crate) rows: Vec<(u32, Range<usize>)>,
    /// Where nvim leaves the caret after a change: a linewise selection's
    /// first row start, a block's top-left corner, a charwise one's start.
    pub(crate) start: usize,
    /// What `.` repeats it by.
    pub(crate) extent: Extent,
}

/// The area `ends` select. Charwise, the last row ends after the grapheme at
/// the end (or at the row's end after `$`); a block's rows are its columns.
fn area(text: &Text, ends: Ends, tab: usize) -> Area {
    let (first, last) = ends.rows(text);
    let block = (ends.kind == VisualKind::Block).then(|| Block::of(text, &ends, tab));
    let hi = cursor_at(text, ends.hi());
    // `$` takes the end of the row its cursor is on, if that is the last.
    let to_end =
        (ends.goal == Some(usize::MAX) && ends.cursor >= ends.anchor) || hi.ix >= hi.line.len();
    let hi_end = hi.line.starts.get(hi.ix + 1).copied();
    let rows = (first..=last)
        .map(|row| {
            let start = text.row_start(row);
            let end = start + text.row_len(row) as usize;
            let range = match (&block, ends.kind) {
                (Some(block), _) => block.cut(text, row, tab, None).range,
                (None, VisualKind::Line) => start..end,
                (None, _) => {
                    let from = if row == first { ends.lo() } else { start };
                    let to = match (row == last, to_end, hi_end) {
                        (true, false, Some(next)) => hi.line.start + next,
                        _ => end,
                    };
                    from..to.max(from)
                }
            };
            (row, range)
        })
        .collect();
    let start = match (&block, ends.kind) {
        (Some(block), _) => at_column(text, first, block.left, tab),
        (None, VisualKind::Line) => text.row_start(first),
        (None, _) => ends.lo(),
    };
    Area {
        rows,
        start,
        extent: extent(text, ends, ends.kind == VisualKind::Line, tab),
    }
}

/// The area of the live visual mode, if one is on.
pub(crate) fn live_area(model: &EditorModel, state: &VimState) -> Option<Area> {
    let ends = ends(model, state)?;
    Some(area(model.text(), ends, model.tab_size().max(1)))
}

/// The area `extent` takes from the primary caret, as `.` lays it.
pub(crate) fn replay_area(model: &EditorModel, state: &VimState, extent: &Extent) -> Area {
    let text = model.text();
    let tab = model.tab_size().max(1);
    let head = Cursor::at(text, model.primary_selection().head()).offset();
    let goal = held_goal(model, state);
    // Unlike a delete's, a one-row `$` change from display column 2 on
    // takes its whole row (nvim 0.12.4).
    let cursor = cursor_at(text, head);
    if matches!(
        extent,
        Extent::Chars {
            rows: 1,
            last: None
        }
    ) && cursor.line.goal_column(cursor.ix, tab) >= 2
    {
        let start = text.row_start(cursor.row);
        let end = start + text.row_len(cursor.row) as usize;
        return Area {
            rows: vec![(cursor.row, start..end)],
            start,
            extent: *extent,
        };
    }
    // A block keeps the columns it had, from the caret's, whatever the
    // rows its corners land on: built here, not from their clamped ends.
    if let Extent::Block { rows, columns } = *extent {
        let columns = columns.filter(|_| goal != Some(usize::MAX));
        let left = column_of(text, head, tab);
        let first = cursor.row;
        let block = Block {
            first,
            last: first.saturating_add(rows - 1).min(text.row_count() - 1),
            left,
            right: columns.map(|columns| left.saturating_add(columns - 1)),
        };
        let rows = (block.first..=block.last)
            .map(|row| (row, block.cut(text, row, tab, None).range))
            .collect();
        return Area {
            rows,
            start: at_column(text, first, left, tab),
            extent: *extent,
        };
    }
    area(text, extent_from(text, head, goal, extent, tab), tab)
}

/// `.` after a change over a selection: the same operator over as much
/// text from the primary caret (`:help visual-repeat`). As in nvim, `'<`
/// and `'>` stay where they were.
pub(crate) fn repeat(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    let Some(extent) = target.state.visual_extent else {
        return effect(false);
    };
    target.model.sync_display();
    let text = target.model.text().clone();
    let tab = target.model.tab_size().max(1);
    let head = Cursor::at(&text, target.model.primary_selection().head()).offset();
    let goal = held_goal(target.model, target.state);
    let ends = extent_from(&text, head, goal, &extent, tab);
    select(target.model, target.state, ends);
    // A block is as wide as it was (after `$`, to each row's end),
    // whatever its last row's length, from the column the caret starts at.
    let columns = match extent {
        Extent::Block { columns, .. } => {
            let columns = columns.filter(|_| goal != Some(usize::MAX));
            Some((column_of(&text, head, tab), columns))
        }
        _ => None,
    };
    let repeated = operate_as_wide(target, actor, args, columns);
    // A change that failed (a shift too large) leaves normal mode as it was.
    if repeated.is_err() {
        target.state.mode = Mode::Normal;
        target.state.visual = None;
        target.model.set_selections([operator::caret(head)]);
    }
    repeated
}

/// `vim.operate` over the selection: `Target::Selection` takes it as its
/// kind does, `Target::SelectedLines` its rows. The mode becomes normal
/// (insert for `c`), keeping the extent for `gv`, `'<` and `'>`. Each change
/// is one transaction with the actor's origin.
pub(crate) fn operate(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
) -> Result<Dispatched<TransactionId>, CommandError> {
    operate_as_wide(target, actor, args, None)
}

/// [`operate()`], over a block of these display columns (its left one and how
/// many, `None` to each row's end) if given.
fn operate_as_wide(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    args: OperateArgs,
    columns: Option<(usize, Option<usize>)>,
) -> Result<Dispatched<TransactionId>, CommandError> {
    if matches!(args.operator, Operator::Format | Operator::FormatKeep) {
        return crate::g_operators::format_selection(target, actor, args);
    }
    target.model.sync_display();
    let Some(ends) = ends(target.model, target.state) else {
        return Err(CommandError::new("no visual selection to operate on"));
    };
    // `I` and `A` act only over a block.
    if matches!(args.operator, Operator::Insert | Operator::Append)
        && ends.kind != VisualKind::Block
    {
        return effect(false);
    }
    let text = target.model.text().clone();
    let lines = ends.kind != VisualKind::Char || args.target != Target::Selection;
    let (first, last) = ends.rows(&text);
    // `>` puts in a count of 'shiftwidth's on every row: bounded as a put is.
    if args.operator == Operator::Indent {
        let by = target
            .model
            .tab_size()
            .max(1)
            .checked_mul(args.count.unwrap_or(1) as usize);
        let size = by.and_then(|by| by.checked_mul((last - first + 1) as usize));
        if size.is_none_or(|size| size > operator::MAX_PUT) {
            return Err(CommandError::new(format!(
                "shifting {} rows {} times would insert more than {} bytes",
                last - first + 1,
                args.count.unwrap_or(1),
                operator::MAX_PUT
            )));
        }
    }
    // `S`, `R` and `J` take a block's rows; the rest act on the block.
    let rows_of_block = matches!(
        (args.operator, args.target),
        (Operator::Change, Target::SelectedLines) | (Operator::Join | Operator::JoinRaw, _)
    );
    if ends.kind == VisualKind::Block && !rows_of_block {
        return operate_block(target, actor, args, ends, columns);
    }
    let anchor_row = text.offset_to_point(ends.anchor).row;
    // Where a yank, `~`, `u` or `U` leaves the caret: the start, which for
    // rows is the first's, unless the cursor is above the anchor.
    let start = match lines {
        true if text.offset_to_point(ends.cursor).row < anchor_row => ends.cursor,
        true => text.row_start(first),
        false => ends.lo(),
    };
    let span = match lines {
        true => Span::Lines {
            rows: first..=last,
            first_non_blank: false,
            from: Some(start),
        },
        false => Span::Chars(ends.lo()..end_of(&text, ends.hi())),
    };
    let extent = extent(&text, ends, lines, target.model.tab_size().max(1));
    leave(target.model, target.state, ends);
    // One caret at the vim cursor, with its goal column, as the operators
    // read them.
    let tab = target.model.tab_size().max(1);
    let goal = ends
        .goal
        .unwrap_or_else(|| goal_of(&cursor_at(&text, ends.cursor), tab));
    to_normal(
        target.model,
        target.state,
        Ends {
            goal: Some(goal),
            ..ends
        },
    );
    // nvim moves the marks (this extent's, or through `.` an earlier
    // one's) on rows a change joins or replaces: on a row joined to the
    // first, to where its text lands there; on rows `c` replaced, to the row
    // after the first; on rows deleted at the end of the text, to the first
    // of them, past the new end.
    let snapshot = target.model.snapshot();
    let at_end = last + 1 == text.row_count();
    let first_row = snapshot.anchor_after(text.row_start(first));
    let marks = target.state.last_visual.clone().expect("a visual extent");
    let rows = [&marks.start, &marks.end].map(|pos| pos.position(snapshot).0);
    // The row a charwise change joins to the first: where what it takes ends.
    let joined = text.offset_to_point(end_of(&text, ends.hi())).row;
    let landings = match args.operator {
        Operator::Join | Operator::JoinRaw => join_landings(
            &text,
            first,
            (last - first + 1).max(2),
            args.operator == Operator::JoinRaw,
        ),
        _ => Vec::new(),
    };
    let moved = |pos: &Pos, row: u32| {
        let past = |anchor| Pos {
            row: anchor,
            past_end: true,
            ..pos.clone()
        };
        Some(match args.operator {
            Operator::Delete | Operator::Change if !lines && first < row && row <= joined => Pos {
                row: first_row,
                column: ends.lo() - text.row_start(first) + pos.column,
                past_end: false,
            },
            // Marks past the end, too, stay past it.
            Operator::Delete if lines && at_end && first > 0 && row >= first => {
                past(snapshot.anchor_after(text.row_start(first - 1)))
            }
            Operator::Delete if lines && at_end && row > last => Pos {
                past_end: false,
                ..pos.clone()
            },
            Operator::Change if lines && first < row && row <= last && at_end => past(first_row),
            Operator::Change if lines && first < row && row <= last => Pos {
                row: snapshot.anchor_after(text.row_start(last + 1)),
                ..pos.clone()
            },
            Operator::Join | Operator::JoinRaw
                if first < row && row <= first + landings.len() as u32 =>
            {
                let landing = landings[(row - first - 1) as usize];
                Pos {
                    row: first_row,
                    column: landing.column(pos.column),
                    past_end: false,
                }
            }
            _ => return None,
        })
    };
    let moved = [moved(&marks.start, rows[0]), moved(&marks.end, rows[1])];
    let dispatched = match args.operator {
        Operator::Delete
        | Operator::Yank
        | Operator::Change
        | Operator::Indent
        | Operator::Outdent => operator::operate_span(target, actor, args, span)?,
        // A linewise `J` starts from the first row's start, where it stays if
        // it joins nothing; `.` joins as `J` does, from the caret.
        Operator::Join | Operator::JoinRaw => {
            let at = match ends.kind == VisualKind::Line && !target.state.replaying {
                true => text.row_start(first),
                false => Cursor::at(&text, ends.lo()).offset(),
            };
            target.model.set_selections([operator::caret(at)]);
            let join = Some((last - first + 1).max(2));
            edit::join_rows(
                target,
                actor,
                JoinArgs {
                    count: join,
                    raw: args.operator == Operator::JoinRaw,
                },
            )?
        }
        Operator::Insert | Operator::Append => unreachable!("refused but over a block"),
        Operator::Format | Operator::FormatKeep => unreachable!("g_operators' own"),
        Operator::ToggleCase
        | Operator::Lowercase
        | Operator::Uppercase
        | Operator::Rot13
        | Operator::Replace(_) => {
            let range = ends.range(&text, if lines { VisualKind::Line } else { ends.kind });
            let land = match args.operator {
                Operator::Replace(_) => range.start,
                _ => start,
            };
            recase(target, actor, args.operator, range, land, lines)
        }
    };
    if let Some(marks) = target.state.last_visual.as_mut() {
        let [start, end] = moved;
        marks.start = start.unwrap_or_else(|| marks.start.clone());
        marks.end = end.unwrap_or_else(|| marks.end.clone());
    }
    if args.operator != Operator::Yank {
        target.state.set_last_change(Change::Operate(args).into());
        if !target.state.replaying {
            target.state.visual_extent = Some(extent);
        }
    }
    Ok(dispatched)
}

/// Where `J` puts a joined row's text on the first row.
#[derive(Copy, Clone, Debug)]
struct Landing {
    /// The column its text (without indentation) starts at.
    at: usize,
    /// The indentation `J` dropped from it.
    indent: usize,
    /// Whether `J` put a space before it.
    space: bool,
}

impl Landing {
    /// Where nvim moves a mark at `column` on the row: with its text, or
    /// where the rows met if it was in the dropped indentation.
    fn column(self, column: usize) -> usize {
        match column >= self.indent {
            true => self.at + column - self.indent,
            false => self.at - usize::from(self.space),
        }
    }
}

/// Where `J` over `rows` rows from `first` (as `edit::join` joins them)
/// puts each row after the first, read from the text before it joins (with
/// `raw`, as `gJ` does).
fn join_landings(text: &Text, first: u32, rows: u32, raw: bool) -> Vec<Landing> {
    let blank = |ch: char| ch == ' ' || ch == '\t';
    let last = first.saturating_add(rows - 1).min(text.row_count() - 1);
    let line = text.row_to_string(first);
    let (mut len, mut empty, mut after_blank) =
        (line.len(), line.is_empty(), line.ends_with(blank));
    (first + 1..=last)
        .map(|row| {
            let next = text.row_to_string(row);
            let rest = next.trim_start_matches(blank);
            let space =
                !raw && !rest.is_empty() && !empty && !after_blank && !rest.starts_with(')');
            // `gJ` keeps the indentation: the row's text starts after it.
            let indent = if raw { 0 } else { next.len() - rest.len() };
            let landing = Landing {
                at: len + usize::from(space),
                indent,
                space,
            };
            len = landing.at + next.len() - indent;
            empty &= rest.is_empty();
            after_blank = rest.ends_with(blank);
            landing
        })
        .collect()
}

/// `~`, `u`, `U`, `g?` or `r{char}` over `range`: each grapheme's first character
/// recased, or each grapheme but line breaks replaced; the caret goes to
/// `land`.
fn recase(
    target: &mut VimTarget<'_>,
    actor: &Actor,
    operator: Operator,
    range: Range<usize>,
    land: usize,
    lines: bool,
) -> Dispatched<TransactionId> {
    use unicode_segmentation::UnicodeSegmentation;
    let VimTarget { model, state, .. } = target;
    let text = model.text().clone();
    let mut edits = Vec::new();
    for (ix, grapheme) in text.slice_to_string(range.clone()).grapheme_indices(true) {
        let new: String = match operator {
            _ if grapheme == "\n" => continue,
            Operator::Replace(ch) => ch.to_string(),
            _ => recased_text(grapheme, operator),
        };
        if new != grapheme {
            let at = range.start + ix;
            edits.push((at..at + grapheme.len(), new));
        }
    }
    let carets_before = vec![history::pair(&text, land)];
    let landing = model.snapshot().anchor_before(land);
    // An undo step even when nothing changes, as in nvim (but for `r`).
    let (transaction, snapshot) = match operator {
        Operator::Replace(_) => operator::apply(model, edits, origin(actor)),
        _ => {
            let (id, snapshot, changed) =
                crate::g_operators::apply_noted(state, model, edits, actor);
            // `'[` and `']` span the selection's ends, or its rows.
            let last = text.clip_offset(range.end.max(range.start + 1) - 1, fe_text::Bias::Left);
            let (first, last) = (
                history::pair(&text, range.start),
                history::pair(&text, last),
            );
            let (start, end) = match lines {
                true => {
                    let len = text.row_len(last.0) as usize;
                    ((first.0, 0), (last.0, len.saturating_sub(1)))
                }
                false => {
                    let clamp = |(row, column): (u32, usize)| {
                        (
                            row,
                            column.min((text.row_len(row) as usize).saturating_sub(1)),
                        )
                    };
                    (clamp(first), clamp(last))
                }
            };
            let version = changed.then(|| snapshot.version());
            crate::marks::recased(state, model, version, start, end);
            (id, snapshot)
        }
    };
    operator::land(model, &snapshot, [snapshot.resolve(&landing)]);
    if let Some(id) = transaction {
        state.carets.record(id, carets_before);
    }
    state.goals = None;
    Dispatched::new(Effect::Changed).with_transactions(transaction.into_iter().collect())
}

/// `ch` as `~`, `u`, `U` or `g?` leave it, by nvim's tables.
pub(super) fn recased(ch: char, operator: Operator) -> char {
    match operator {
        Operator::Lowercase => edit::lowered(ch),
        Operator::Uppercase => edit::raised(ch),
        Operator::Rot13 => crate::g_operators::rot13(ch),
        _ => edit::toggled(ch),
    }
}
