//! Folds: the `z` fold keys and `vim.fold` (a feature module; see [`crate::feature`]).
//!
//! `za zc zo zA zC zO zR zM zv` commit `vim.fold {"action", "count"}` (context
//! `editor && vim.mode == normal`, `mutates_buffer` false) and run through
//! `EditorModel`'s fold API. They don't edit text and leave no undo step.
//!
//! - `zo` opens the closed folds at the cursor's row, outermost first, `count`
//!   levels (one by default); `zO` opens every level there.
//! - `zc` closes the innermost candidate containing the row that isn't closed
//!   yet, `count` levels outward; `zC` closes every level there.
//! - `za` opens if the row is in a closed fold, else closes; `zA` the same
//!   through all levels.
//! - `zR` opens every fold, `zM` closes every candidate.
//! - `zv` opens the folds hiding the cursor's row.
//!
//! **Divergence from nvim.** Folds are fastedit's own, not nvim's
//! `foldmethod=indent`: a fold starts at the header row, the one that opens
//! the block, where nvim's starts on the first indented row. Candidates come
//! from `EditorHost::fold_ranges` (syntax), or with none, from the buffer's
//! indented blocks. So these keys aren't checked against nvim, and a closed
//! fold's cursor sits on its header.

use std::ops::RangeInclusive;

use fe_command::{Actor, CommandError, ContextPredicate, Dispatched, Effect, Registry};
use fe_editor::EditorModel;
use fe_editor::commands::EditorHost;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Pending, Step, VimCommands, VimKey, VimTarget, feature::Feature, reducer::invocation};

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
}

/// `vim.fold`'s args.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FoldArgs {
    pub action: FoldAction,
    /// The levels `zo`, `zc` and `za` act on; at least one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

fn z(pending: &Pending, key: VimKey) -> Option<Step> {
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
            "Opens or closes the editor's folds at the primary caret's row (`zo zc za zO zC zA`), every fold (`zR zM`) or those hiding the caret (`zv`); `count` is the levels `zo`, `zc` and `za` act on. Candidates are the host's fold ranges, or indented blocks. Changes no text.",
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

fn fold_rows(model: &EditorModel) -> Vec<(u32, u32)> {
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
    let closed = closed_at(model, row);
    match args.action {
        FoldAction::OpenEvery => model.unfold_all(),
        FoldAction::CloseEvery => model.fold_all(host_ranges),
        FoldAction::Open => reopen(model, row, false, |ix, _| ix < levels),
        FoldAction::OpenAll => reopen(model, row, true, |_, _| true),
        FoldAction::Reveal => reopen(model, row, false, |_, start| start < row),
        FoldAction::Close => close(model, &host_ranges, row, levels),
        FoldAction::CloseAll => close(model, &host_ranges, row, usize::MAX),
        FoldAction::Toggle if closed.is_empty() => close(model, &host_ranges, row, levels),
        FoldAction::Toggle => reopen(model, row, false, |ix, _| ix < levels),
        FoldAction::ToggleAll if closed.is_empty() => close(model, &host_ranges, row, usize::MAX),
        FoldAction::ToggleAll => reopen(model, row, true, |_, _| true),
    }
}

/// The candidates containing `row`: `host`'s, or if it has none there, the
/// buffer's indented blocks, whatever the host has elsewhere.
fn candidates_at(
    model: &EditorModel,
    host: &[RangeInclusive<u32>],
    row: u32,
) -> Vec<RangeInclusive<u32>> {
    let mut local: Vec<_> = host
        .iter()
        .filter(|rows| rows.contains(&row))
        .cloned()
        .collect();
    if local.is_empty() {
        local = indented_blocks(model);
        local.retain(|rows| rows.contains(&row));
    }
    local
}

/// The buffer's indented blocks.
fn indented_blocks(model: &EditorModel) -> Vec<RangeInclusive<u32>> {
    let text = model.text();
    let tab_size = model.tab_size();
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
        model.unfold_rows(outer_start..=outer_end);
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
fn close(model: &mut EditorModel, host: &[RangeInclusive<u32>], row: u32, levels: usize) {
    let closed = closed_at(model, row);
    let mut open: Vec<_> = candidates_at(model, host, row)
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
