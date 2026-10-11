//! Row changes: which rows of an editor's text differ from a base text (the
//! file's text in git, say). GPUI-free; [`EditorModel::row_changes`] keeps
//! the answer current with every edit.
//!
//! [`EditorModel::row_changes`]: crate::EditorModel::row_changes

use std::ops::Range;
use std::sync::Arc;

use imara_diff::{Algorithm, Diff, InternedInput, TokenSource};

/// A base or text over this many bytes isn't diffed.
pub(crate) const MAX_DIFF_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowChangeKind {
    /// Rows the base doesn't have.
    Added,
    /// Rows that replace others.
    Modified,
    /// Rows the base has that the text lacks.
    Deleted,
}

/// A run of changed rows in the current text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowChange {
    /// Buffer rows of the current text. Empty for [`RowChangeKind::Deleted`]:
    /// the row that now follows the removed rows, or the row count at the end
    /// of the text.
    pub rows: Range<u32>,
    pub kind: RowChangeKind,
}

/// Rows of a text, without their line terminators.
struct Rows<'a>(Vec<&'a str>);

impl<'a> TokenSource for Rows<'a> {
    type Token = &'a str;
    type Tokenizer = std::vec::IntoIter<&'a str>;

    fn tokenize(&self) -> Self::Tokenizer {
        self.0.clone().into_iter()
    }

    fn estimate_tokens(&self) -> u32 {
        self.0.len() as u32
    }
}

/// `base`'s rows, split as [`fe_text::Text`] splits: `\r\n`, `\r` and `\n`
/// all end a row. `scratch` holds the normalised text when it's needed.
fn rows<'a>(text: &'a str, scratch: &'a mut String) -> Rows<'a> {
    let text = if text.contains('\r') {
        *scratch = text.replace("\r\n", "\n").replace('\r', "\n");
        scratch.as_str()
    } else {
        text
    };
    Rows(text.split('\n').collect())
}

/// The rows of `text` (LF-normalised or not) that differ from `base`, sorted.
/// Empty when either is over 5 MiB.
pub(crate) fn diff_rows(base: &str, text: &str) -> Arc<[RowChange]> {
    #[cfg(test)]
    tests::DIFFS.with(|count| count.set(count.get() + 1));
    if base.len() > MAX_DIFF_BYTES || text.len() > MAX_DIFF_BYTES {
        return Arc::new([]);
    }
    let (mut base_scratch, mut text_scratch) = (String::new(), String::new());
    let (old, new) = (rows(base, &mut base_scratch), rows(text, &mut text_scratch));
    let input = InternedInput::new(old, new);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .map(|hunk| {
            let kind = if hunk.before.is_empty() {
                RowChangeKind::Added
            } else if hunk.after.is_empty() {
                RowChangeKind::Deleted
            } else {
                RowChangeKind::Modified
            };
            RowChange {
                rows: hunk.after,
                kind,
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "row_changes_tests.rs"]
mod tests;
