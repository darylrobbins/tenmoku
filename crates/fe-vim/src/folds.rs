//! Folds: the `z` fold keys and `vim.fold` (a feature module; see [`crate::feature`]).
//!
//! `za zc zo zA zC zO zR zM zv zd zD zE` commit `vim.fold {"action",
//! "count"}` (context `editor && vim.mode == normal`, `mutates_buffer`
//! false) and run through `EditorModel`'s fold API. They don't edit text
//! and leave no undo step.
//!
//! - `zo` opens the closed folds at the cursor's row, outermost first, `count`
//!   levels (one by default); `zO` opens every level there.
//! - `zc` closes the innermost candidate containing the row that isn't closed
//!   yet, `count` levels outward; `zC` closes every level there.
//! - `za` opens if the row is in a closed fold, else closes; `zA` the same
//!   through all levels.
//! - `zR` opens every fold, `zM` closes every candidate.
//! - `zv` opens the folds hiding the cursor's row.
//! - `zd` deletes the fold at the cursor, the outermost closed one there,
//!   `zD` every fold nested in it too, and `zE` every fold.
//!
//! `zf{motion}` or `zf{object}` ([`Operator::Fold`], through `vim.operate`)
//! closes a fold over the rows the target covers, as `>` takes them (the
//! last being the row before when an exclusive motion ended at its column
//! 0), the caret going to the first row at its goal column; `{count}zF`
//! over `count` rows (as `zf` over `count` rows of `_`), and visual `zf`
//! over the selection's rows, the caret at its start. One row hides nothing,
//! so makes no fold. No undo step, and `.` doesn't repeat it.
//!
//! `zj` and `zk` ([`Motion::FoldNext`] and [`Motion::FoldPrevious`]) go to
//! the first row of the next candidate starting below the cursor's row, or
//! the last row of the previous one ending above it, at column 0, `count`
//! times or as far as there are candidates; with none, they fail without
//! moving. A closed fold counts as one: a row it hides stands for its
//! header, and the folds closed by `zf` count too. With an operator they're
//! exclusive and charwise, with `:help exclusive`'s adjustments: `dzj` from
//! at or before its row's first non-blank takes whole rows, up to the one
//! before the next candidate's first; from further along, the rest of its
//! row and the rows up to that one. With an operator pending, `z` takes
//! only `j` and `k`.
//!
//! **Divergence from nvim.** Folds are fastedit's own, not nvim's
//! `foldmethod=indent`: a fold starts at the header row, the one that opens
//! the block, where nvim's starts on the first indented row. Candidates come
//! from `EditorHost::fold_ranges` (syntax), or with none, from the buffer's
//! indented blocks. So these keys aren't checked against nvim, and a closed
//! fold's cursor sits on its header. Nor are there open manual folds: a
//! fold exists only while closed, so one `zd` deletes, `zo` opens or `zR`
//! opens is gone, and `zf` over open rows closes them at once (nvim's `zf`
//! closes its fold too). `zj` and `zk` go to any candidate's first or last
//! row, nested or not, where nvim's step through the levels at the cursor.
//! nvim's `dzj` takes the class of the motion typed before it (its operator
//! state isn't reset: after `j`, `dzj` is linewise); fastedit's is always
//! exclusive, as nvim's is fresh.

use std::{cell::RefCell, ops::RangeInclusive};

use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::EditorModel;
use fe_editor::commands::EditorHost;
use fe_text::Text;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    Motion, MoveArgs, Operator, Pending, Step, Target, VimCommands, VimKey, VimTarget,
    feature::Feature, motion::Cursor, operator, reducer::invocation, visual,
};

pub(crate) const FEATURE: Feature = Feature {
    z: Some(z),
    register,
    ..Feature::NONE
};

/// What `vim.fold` does.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FoldAction {
    /// `za`.
    Toggle,
    /// `zc`.
    Close,
    /// `zo`.
    Open,
    /// `zA`.
    ToggleAll,
    /// `zC`.
    CloseAll,
    /// `zO`.
    OpenAll,
    /// `zR`.
    OpenEvery,
    /// `zM`.
    CloseEvery,
    /// `zv`.
    Reveal,
    /// `zd`.
    Delete,
    /// `zD`.
    DeleteNested,
    /// `zE`.
    DeleteEvery,
}

/// `vim.fold`'s args.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FoldArgs {
    pub action: FoldAction,
    /// The levels `zo`, `zc` and `za` act on; at least one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// The key after `z`. With an operator pending only `zj` and `zk` complete
/// it, and as folds is the first feature with a `z` hook, it cancels any
/// other key (`dzt`) before another feature's hook reads it.
fn z(pending: &Pending, key: VimKey) -> Option<Step> {
    let pending = pending.clone();
    match key {
        VimKey::Char('j') => return Some(pending.motion(Motion::FoldNext)),
        VimKey::Char('k') => return Some(pending.motion(Motion::FoldPrevious)),
        _ if pending.operator().is_some() => return Some(Step::Cancel),
        VimKey::Char('f') if pending.is_visual() => {
            return Some(pending.operate(Operator::Fold, Target::Selection));
        }
        VimKey::Char('f') => return Some(pending.with_operator(Operator::Fold)),
        VimKey::Char('F') if !pending.is_visual() => {
            return Some(pending.operate(Operator::Fold, Target::Motion(Motion::Line)));
        }
        _ => {}
    }
    let VimKey::Char(ch) = key else { return None };
    let action = match ch {
        'a' => FoldAction::Toggle,
        'c' => FoldAction::Close,
        'o' => FoldAction::Open,
        'A' => FoldAction::ToggleAll,
        'C' => FoldAction::CloseAll,
        'O' => FoldAction::OpenAll,
        'R' => FoldAction::OpenEvery,
        'M' => FoldAction::CloseEvery,
        'v' => FoldAction::Reveal,
        'd' => FoldAction::Delete,
        'D' => FoldAction::DeleteNested,
        'E' => FoldAction::DeleteEvery,
        _ => return None,
    };
    let count = pending.count();
    Some(invocation("vim.fold", FoldArgs { action, count }))
}

fn register(registry: &mut Registry<VimCommands>) {
    let normal = ContextPredicate::parse("editor && vim.mode == normal")
        .expect("vim's context predicate parses");
    registry
        .register_with_args(
            "vim.fold",
            "Vim Fold",
            |target: &mut VimTarget<'_>, _: &Actor, args: FoldArgs| {
                let changed = fold(target, args);
                Ok::<_, CommandError>(Dispatched::new(match changed {
                    true => Effect::Changed,
                    false => Effect::Nothing,
                }))
            },
        )
        .expect("vim command IDs are valid and distinct")
        .description(
            "Opens or closes the editor's folds at the primary caret's row (`zo zc za zO zC zA`), every fold (`zR zM`) or those hiding the caret (`zv`), or deletes the fold at the caret (`zd`), every fold nested there (`zD`) or every fold (`zE`), which in fastedit opens them; `count` is the levels `zo`, `zc` and `za` act on. Candidates are the host's fold ranges, or indented blocks. Changes no text.",
        )
        .context(normal)
        .mutates_buffer(false);
}

/// Runs `args`; returns whether the folds or the selections changed.
fn fold(target: &mut VimTarget<'_>, args: FoldArgs) -> bool {
    let model = &mut *target.model;
    model.sync_display();
    let before = (fold_rows(model), model.selections());
    run(model, &*target.host, args);
    before != (fold_rows(model), model.selections())
}

pub(crate) fn fold_rows(model: &EditorModel) -> Vec<(u32, u32)> {
    let mut folds: Vec<_> = model
        .display()
        .folds()
        .iter()
        .map(|fold| (fold.start, fold.end))
        .collect();
    folds.sort();
    folds
}

pub(crate) fn run(model: &mut EditorModel, host: &dyn EditorHost, args: FoldArgs) {
    let row = model
        .text()
        .offset_to_point(model.primary_selection().head())
        .row;
    let levels = args.count.unwrap_or(1).max(1) as usize;
    let host_ranges = host.fold_ranges(0..model.text().row_count());
    let exclusive = host.folds_exclusive();
    let closed = closed_at(model, row);
    match args.action {
        FoldAction::OpenEvery | FoldAction::DeleteEvery => model.unfold_all(),
        FoldAction::CloseEvery if exclusive => model.fold_all_exact(host_ranges),
        FoldAction::CloseEvery => model.fold_all(host_ranges),
        FoldAction::Open => reopen(model, row, false, |ix, _| ix < levels),
        FoldAction::Delete => reopen(model, row, false, |ix, _| ix == 0),
        FoldAction::OpenAll | FoldAction::DeleteNested => reopen(model, row, true, |_, _| true),
        FoldAction::Reveal => reopen(model, row, false, |_, start| start < row),
        FoldAction::Close => close(model, &host_ranges, exclusive, row, levels),
        FoldAction::CloseAll => close(model, &host_ranges, exclusive, row, usize::MAX),
        FoldAction::Toggle if closed.is_empty() => {
            close(model, &host_ranges, exclusive, row, levels)
        }
        FoldAction::Toggle => reopen(model, row, false, |ix, _| ix < levels),
        FoldAction::ToggleAll if closed.is_empty() => {
            close(model, &host_ranges, exclusive, row, usize::MAX)
        }
        FoldAction::ToggleAll => reopen(model, row, true, |_, _| true),
    }
}

/// The candidates containing `row`: `host`'s, or if it has none there, the
/// buffer's indented blocks, whatever the host has elsewhere.
fn candidates_at(
    model: &EditorModel,
    host: &[RangeInclusive<u32>],
    exclusive: bool,
    row: u32,
) -> Vec<RangeInclusive<u32>> {
    let mut local: Vec<_> = host
        .iter()
        .filter(|rows| rows.contains(&row))
        .cloned()
        .collect();
    if local.is_empty() && !exclusive {
        local = indented_blocks(model.text(), model.tab_size());
        local.retain(|rows| rows.contains(&row));
    }
    local
}

/// `text`'s indented blocks.
fn indented_blocks(text: &Text, tab_size: usize) -> Vec<RangeInclusive<u32>> {
    let tab_size = tab_size.max(1);
    let indent = |row: u32| {
        let mut column = 0;
        for ch in text.row_to_string(row).chars() {
            match ch {
                ' ' => column += 1,
                '\t' => column += tab_size - column % tab_size,
                ch if ch.is_whitespace() => {}
                _ => return Some(column),
            }
        }
        None
    };
    let mut blocks = Vec::new();
    // The rows heading the blocks open so far with their indents, outermost
    // first, and the last row with text.
    let mut open: Vec<(u32, usize)> = Vec::new();
    let mut last = 0;
    let mut close = |open: &mut Vec<(u32, usize)>, level: usize, last: u32| {
        while let Some(&(header, _)) = open.last().filter(|(_, indent)| *indent >= level) {
            open.pop();
            if last > header {
                blocks.push(header..=last);
            }
        }
    };
    for row in 0..text.row_count() {
        let Some(level) = indent(row) else { continue };
        close(&mut open, level, last);
        open.push((row, level));
        last = row;
    }
    close(&mut open, 0, last);
    blocks
}

/// The closed folds at `row`, outermost first: those containing it, then if
/// the caret is on the innermost one's header (a closed fold moves it
/// there), the first fold nested in it, and so on inward.
fn closed_at(model: &EditorModel, row: u32) -> Vec<(u32, u32)> {
    let all: Vec<_> = model
        .display()
        .folds()
        .iter()
        .map(|fold| (fold.start, fold.end))
        .collect();
    let mut chain: Vec<_> = all
        .iter()
        .copied()
        .filter(|&(start, end)| start <= row && row <= end)
        .collect();
    chain.sort_by_key(|&(start, end)| (start, std::cmp::Reverse(end)));
    if chain.last().is_some_and(|&(start, _)| start == row) {
        extend(&all, &mut chain);
    }
    chain
}

/// `chain` with the first fold nested in its last, repeatedly.
fn extend(all: &[(u32, u32)], chain: &mut Vec<(u32, u32)>) {
    while let Some(&(start, end)) = chain.last() {
        let inner = all
            .iter()
            .copied()
            .filter(|&(s, e)| s > start && e <= end)
            .min_by_key(|&(s, e)| (s, std::cmp::Reverse(e)));
        let Some(inner) = inner else { break };
        chain.push(inner);
    }
}

/// Opens the closed folds at `row` that `open` takes, given their level
/// (outermost 0) and first row; the others stay closed. `recursive` opens
/// every fold nested in the outermost one instead.
fn reopen(model: &mut EditorModel, row: u32, recursive: bool, open: impl Fn(usize, u32) -> bool) {
    let closed = closed_at(model, row);
    let Some(&(outer_start, outer_end)) = closed.first() else {
        return;
    };
    if recursive {
        // Unfolding takes every fold it touches; the ones reaching outside
        // the outermost aren't nested in it, so they go back.
        let straddling: Vec<_> = fold_rows(model)
            .into_iter()
            .filter(|&(start, end)| start <= outer_end && end >= outer_start)
            .filter(|&(start, end)| start < outer_start || end > outer_end)
            .collect();
        model.unfold_rows(outer_start..=outer_end);
        for (start, end) in straddling {
            model.fold_rows(start..=end);
        }
        return;
    }
    model.unfold_rows(row..=row);
    for (ix, &(start, _)) in closed.iter().enumerate() {
        if open(ix, start) {
            model.unfold_rows(start..=start);
        }
    }
    for (ix, &(start, end)) in closed.iter().enumerate() {
        if !open(ix, start) {
            model.fold_rows(start..=end);
        }
    }
}

/// Closes the innermost `levels` candidates containing `row` that aren't
/// closed yet.
fn close(
    model: &mut EditorModel,
    host: &[RangeInclusive<u32>],
    exclusive: bool,
    row: u32,
    levels: usize,
) {
    let closed = closed_at(model, row);
    let mut open: Vec<_> = candidates_at(model, host, exclusive, row)
        .into_iter()
        .filter(|rows| {
            !closed
                .iter()
                .any(|&(start, end)| start == *rows.start() && end == *rows.end())
        })
        .collect();
    open.sort_by_key(|rows| (std::cmp::Reverse(*rows.start()), *rows.end()));
    for rows in open.into_iter().take(levels) {
        model.fold_rows(rows);
    }
}

/// Closes a fold over each of `rows` that spans more than one row; returns
/// whether any was made.
pub(crate) fn close_rows(
    model: &mut EditorModel,
    rows: impl IntoIterator<Item = RangeInclusive<u32>>,
) -> bool {
    let before = fold_rows(model);
    for rows in rows {
        if rows.start() < rows.end() {
            model.fold_rows(rows);
        }
    }
    before != fold_rows(model)
}

/// Visual `zf`: closes a fold over the selection's rows, leaving the visual
/// mode with the caret at its start.
pub(crate) fn fold_selection(
    target: &mut VimTarget<'_>,
) -> Result<Dispatched<fe_buffer::TransactionId>, CommandError> {
    target.model.sync_display();
    let Some(area) = visual::live_area(target.model, target.state) else {
        return Err(CommandError::new("no visual selection to fold"));
    };
    let (Some(first), Some(last)) = (area.rows.first(), area.rows.last()) else {
        return Err(CommandError::new("no visual selection to fold"));
    };
    let rows = first.0..=last.0;
    visual::exit(target)?;
    let model = &mut *target.model;
    model.set_selections([operator::caret(area.start)]);
    target.state.goals = None;
    close_rows(model, [rows]);
    Ok(Dispatched::new(Effect::Changed))
}

/// What `zj` and `zk` go between while [`with_candidates`] runs: the
/// candidates' first rows and last rows, each sorted, a row a closed fold
/// hides standing for its header.
#[derive(Default)]
struct Candidates {
    starts: Vec<u32>,
    ends: Vec<u32>,
}

impl Candidates {
    fn new(
        candidates: impl IntoIterator<Item = RangeInclusive<u32>>,
        closed: &[(u32, u32)],
    ) -> Self {
        // A closed fold hides the rows after its header, shown as it; one
        // starting in another's rows is hidden with them, so the rows each
        // run of them hides are shown as the run's first header.
        let mut sorted = closed.to_vec();
        sorted.sort_unstable();
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for (start, end) in sorted {
            match runs.last_mut() {
                Some(run) if start <= run.1 => run.1 = run.1.max(end),
                _ => runs.push((start, end)),
            }
        }
        let shown = |row: u32| {
            let ix = runs.partition_point(|&(start, _)| start < row);
            match ix.checked_sub(1).map(|ix| runs[ix]) {
                Some((start, end)) if row <= end => start,
                _ => row,
            }
        };
        let mut found = Self::default();
        for rows in candidates {
            found.starts.push(shown(*rows.start()));
            found.ends.push(shown(*rows.end()));
        }
        for list in [&mut found.starts, &mut found.ends] {
            list.sort_unstable();
            list.dedup();
        }
        found
    }

    /// The row `count` candidates on from `row`, as far as there are any;
    /// `None` if there's none.
    fn step(&self, row: u32, down: bool, count: usize) -> Option<u32> {
        match down {
            true => {
                let past = self.starts.partition_point(|&start| start <= row);
                let last = self.starts.len().checked_sub(1)?;
                (past <= last).then(|| self.starts[past.saturating_add(count - 1).min(last)])
            }
            false => {
                let before = self.ends.partition_point(|&end| end < row);
                (before > 0).then(|| self.ends[before.saturating_sub(count)])
            }
        }
    }
}

thread_local! {
    static CANDIDATES: RefCell<Option<Candidates>> = const { RefCell::new(None) };
}

/// Puts the candidates it replaced back when dropped, even in a panic.
struct Restore(Option<Candidates>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        CANDIDATES.with(|candidates| *candidates.borrow_mut() = previous);
    }
}

/// Runs `f` with the candidates `zj` and `zk` go between available to
/// [`motion`], which isn't given the host, if `motion` (the command's) is
/// one of them: the host's fold ranges, or with none, the buffer's indented
/// blocks, and the folds closed now. Outside it, [`motion`] reads the
/// indented blocks of the text it's given.
pub(crate) fn with_candidates<'a, R>(
    target: &mut VimTarget<'a>,
    motion: Option<Motion>,
    f: impl FnOnce(&mut VimTarget<'a>) -> R,
) -> R {
    if !matches!(motion, Some(Motion::FoldNext | Motion::FoldPrevious)) {
        return f(target);
    }
    let model = &mut *target.model;
    model.sync_display();
    let mut candidates = target.host.fold_ranges(0..model.text().row_count());
    if candidates.is_empty() && !target.host.folds_exclusive() {
        candidates = indented_blocks(model.text(), model.tab_size());
    }
    let closed = fold_rows(model);
    candidates.extend(closed.iter().map(|&(start, end)| start..=end));
    let found = Candidates::new(candidates, &closed);
    let _restore = Restore(CANDIDATES.with(|held| held.borrow_mut().replace(found)));
    f(target)
}

/// `zj` (`Motion::FoldNext`) or `zk` from `cursor`, as `motion::motion`
/// moves one.
pub(crate) fn motion(
    cursor: &mut Cursor,
    args: MoveArgs,
    goal: Option<usize>,
    tab: usize,
) -> Result<Option<usize>, Option<usize>> {
    let count = args.count.unwrap_or(1).max(1) as usize;
    let down = args.motion == Motion::FoldNext;
    let to = CANDIDATES.with(|held| match held.borrow().as_ref() {
        Some(candidates) => candidates.step(cursor.row, down, count),
        None => {
            Candidates::new(indented_blocks(cursor.text, tab), &[]).step(cursor.row, down, count)
        }
    });
    let Some(row) = to else {
        return Err(goal);
    };
    cursor.enter(row.min(cursor.text.row_count() - 1));
    cursor.ix = 0;
    Ok(None)
}
