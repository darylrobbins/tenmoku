//! What a turn carries from the workspace, and its budget (the record's
//! § 13): each [`ContextItem`] has an [`estimate`](ContextItem::estimate)
//! of a token for every 3 bytes, and [`budget`] fits a turn's items into
//! `chat.context_budget` in the record's order, cutting the first that
//! doesn't fit at a row boundary or dropping it.

use crate::conversation::Rows;

/// The fewest rows an item is cut to: with fewer left, it's dropped.
pub const MIN_ROWS: u32 = 10;

/// Something attached to a turn, gathered by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextItem {
    pub kind: ContextKind,
    /// The chip's label.
    pub label: String,
    pub text: String,
}

/// What a [`ContextItem`] is, and how its tag names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextKind {
    /// A file, or some of its rows.
    File {
        path: String,
        rows: Option<Rows>,
    },
    Selection {
        path: String,
        rows: Rows,
    },
    /// The active file's problems.
    Diagnostics,
    /// The active file, tagged as a file is: `rows` only once it's cut.
    ActiveFile {
        path: String,
        rows: Option<Rows>,
    },
    OpenEditors,
}

impl ContextKind {
    /// Where the budget puts it: the selection, files, diagnostics, the
    /// active file, then the open editors.
    fn rank(&self) -> u8 {
        match self {
            ContextKind::Selection { .. } => 0,
            ContextKind::File { .. } => 1,
            ContextKind::Diagnostics => 2,
            ContextKind::ActiveFile { .. } => 3,
            ContextKind::OpenEditors => 4,
        }
    }
}

impl ContextItem {
    /// Its tokens, by the record's estimate: a token for every 3 bytes,
    /// rounded up.
    pub fn estimate(&self) -> u32 {
        estimate(self.text.len())
    }
}

/// How much of an item the budget sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Whole,
    /// Its first `rows_kept` rows of `rows_total`, then a
    /// `[truncated: N more rows]` line.
    Truncated {
        rows_kept: u32,
        rows_total: u32,
    },
    /// None of it: fewer than [`MIN_ROWS`] rows fit.
    Dropped,
}

/// An item as the budget left it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Budgeted {
    /// The item as sent: when truncated, its text the rows kept and the
    /// truncation line, and a file's or selection's rows those kept.
    pub item: ContextItem,
    /// Its index in the items budgeted.
    pub index: usize,
    /// The whole item's estimate.
    pub estimate: u32,
    pub fit: Fit,
}

impl Budgeted {
    /// Whether any of it is sent.
    pub fn sent(&self) -> bool {
        self.fit != Fit::Dropped
    }
}

/// `items` in the record's order, each item of a kind in the order given,
/// filled into `limit` tokens: each item that fits whole is sent whole; one
/// that doesn't is cut after the most rows that fit with its truncation
/// line, if that's at least [`MIN_ROWS`], else dropped.
pub fn budget(items: &[ContextItem], limit: u32) -> Vec<Budgeted> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&index| items[index].kind.rank());
    let mut left = limit;
    order
        .into_iter()
        .map(|index| {
            let item = &items[index];
            let estimate = item.estimate();
            let (item, fit) = if estimate <= left {
                left -= estimate;
                (item.clone(), Fit::Whole)
            } else {
                match truncate(item, left) {
                    Some((item, fit)) => {
                        left -= item.estimate();
                        (item, fit)
                    }
                    None => (item.clone(), Fit::Dropped),
                }
            };
            Budgeted {
                item,
                index,
                estimate,
                fit,
            }
        })
        .collect()
}

/// `item` cut to the most rows whose text and truncation line fit in
/// `left` tokens, if that's at least [`MIN_ROWS`] and not all of them.
fn truncate(item: &ContextItem, left: u32) -> Option<(ContextItem, Fit)> {
    let rows: Vec<&str> = item.text.split_inclusive('\n').collect();
    let total = rows.len();
    let mut bytes: usize = rows.iter().map(|row| row.len()).sum();
    // From all but the last row down, the most that fit.
    let mut kept = total;
    while kept > 0 {
        bytes -= rows[kept - 1].len();
        kept -= 1;
        if estimate(bytes + marker(total - kept).len()) <= left {
            break;
        }
    }
    if kept < MIN_ROWS as usize {
        return None;
    }
    let (rows_kept, rows_total) = (count(kept), count(total));
    let mut text = rows[..kept].concat();
    text.push_str(&marker(total - kept));
    let kind = match &item.kind {
        ContextKind::File { path, rows } => ContextKind::File {
            path: path.clone(),
            rows: Some(kept_rows(rows.map_or(1, |rows| rows.first), rows_kept)),
        },
        ContextKind::ActiveFile { path, rows } => ContextKind::ActiveFile {
            path: path.clone(),
            rows: Some(kept_rows(rows.map_or(1, |rows| rows.first), rows_kept)),
        },
        ContextKind::Selection { path, rows } => ContextKind::Selection {
            path: path.clone(),
            rows: kept_rows(rows.first, rows_kept),
        },
        kind @ (ContextKind::Diagnostics | ContextKind::OpenEditors) => kind.clone(),
    };
    let item = ContextItem {
        kind,
        label: item.label.clone(),
        text,
    };
    let fit = Fit::Truncated {
        rows_kept,
        rows_total,
    };
    Some((item, fit))
}

/// The line that ends a truncated item.
fn marker(more: usize) -> String {
    format!("[truncated: {more} more rows]")
}

/// `kept` rows from row `first`.
fn kept_rows(first: u32, kept: u32) -> Rows {
    Rows {
        first,
        last: first + kept - 1,
    }
}

fn count(rows: usize) -> u32 {
    u32::try_from(rows).unwrap_or(u32::MAX)
}

/// The tokens of `bytes` bytes of text.
fn estimate(bytes: usize) -> u32 {
    u32::try_from(bytes.div_ceil(3)).unwrap_or(u32::MAX)
}
