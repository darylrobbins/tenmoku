//! Row changes: which rows of an editor's text differ from a base text (the
//! file's text in git, say). GPUI-free; [`EditorModel::row_changes`] keeps
//! the answer current with every edit.
//!
//! [`EditorModel::row_changes`]: crate::EditorModel::row_changes

use std::cell::RefCell;
use std::cell::{Cell, OnceCell};
use std::ops::Range;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use fe_buffer::{BufferSnapshot, Version};

use crate::decorations::DecorationProvider;
use crate::row_change_decorations::RowChangeDecorations;

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

/// An editor's diff base and the changes against it for one buffer version,
/// shared by its splits and by the decoration provider, so a version is
/// diffed once whoever asks first.
#[derive(Default)]
pub(crate) struct RowChangeState {
    inner: RefCell<Inner>,
    /// The changed-rows decoration provider every model sharing this state
    /// queries; it holds the state weakly.
    provider: OnceCell<Rc<RefCell<dyn DecorationProvider>>>,
    /// The version each model sharing this state is at; a diff is kept
    /// while one of them is at its version.
    live: RefCell<Vec<Weak<Cell<Version>>>>,
}

#[derive(Default)]
struct Inner {
    base: Option<Arc<str>>,
    /// Per version: diffed against the base, or as supplied.
    cached: Vec<(Version, Arc<[RowChange]>)>,
    /// The changes last supplied, while the host supplies them (see
    /// [`RowChangeState::supply`]): what a version not yet supplied shows.
    supplied: Option<Arc<[RowChange]>>,
}

impl RowChangeState {
    pub(crate) fn new() -> Rc<Self> {
        let state = Rc::new(Self::default());
        let provider = RowChangeDecorations::new(Rc::downgrade(&state));
        let provider: Rc<RefCell<dyn DecorationProvider>> = Rc::new(RefCell::new(provider));
        let _ = state.provider.set(provider);
        state
    }

    /// The provider that turns the changes into decorations: the same one
    /// for every model sharing this state, whichever set the base.
    pub(crate) fn provider(&self) -> Option<Rc<RefCell<dyn DecorationProvider>>> {
        self.provider.get().cloned()
    }

    /// Keeps the diff of whatever version `token` holds.
    pub(crate) fn track(&self, token: &Rc<Cell<Version>>) {
        self.live.borrow_mut().push(Rc::downgrade(token));
    }

    pub(crate) fn has_base(&self) -> bool {
        self.inner.borrow().base.is_some()
    }

    /// The base, diffed against or supplied.
    pub(crate) fn base(&self) -> Option<Arc<str>> {
        self.inner.borrow().base.clone()
    }

    /// Sets the base and diffs against it from now on, leaving supplied
    /// mode. Returns whether anything changed.
    pub(crate) fn set_base(&self, base: Option<Arc<str>>) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner.base == base && inner.supplied.is_none() {
            return false;
        }
        inner.base = base;
        inner.supplied = None;
        inner.cached.clear();
        true
    }

    /// Takes `changes` as version `version`'s against `base`, and stops
    /// diffing: from now on a version shows the changes supplied for it,
    /// or until those arrive the last ones supplied, until
    /// [`RowChangeState::set_base`].
    pub(crate) fn supply(&self, base: Arc<str>, version: Version, changes: Arc<[RowChange]>) {
        let mut inner = self.inner.borrow_mut();
        // Changes for other versions were against another base, or diffed.
        if inner.base.as_ref() != Some(&base) || inner.supplied.is_none() {
            inner.cached.clear();
        }
        inner.base = Some(base);
        inner.cached.retain(|(cached, _)| *cached != version);
        inner.cached.push((version, changes.clone()));
        inner.supplied = Some(changes);
        self.forget_dead(&mut inner, version);
    }

    /// The changes in `snapshot`'s text against the base; empty without one.
    pub(crate) fn changes(&self, snapshot: &BufferSnapshot) -> Arc<[RowChange]> {
        let mut inner = self.inner.borrow_mut();
        let Some(base) = inner.base.clone() else {
            return Arc::new([]);
        };
        let version = snapshot.version();
        if let Some((_, changes)) = inner.cached.iter().find(|(cached, _)| *cached == version) {
            return changes.clone();
        }
        if let Some(supplied) = &inner.supplied {
            return supplied.clone();
        }
        let text = snapshot.text();
        let changes = if text.len() > MAX_DIFF_BYTES {
            Arc::new([])
        } else {
            diff_rows(&base, &text.to_string())
        };
        inner.cached.push((version, changes.clone()));
        self.forget_dead(&mut inner, version);
        changes
    }

    /// Drops the changes of versions no model sharing this state is at,
    /// but `version`'s.
    fn forget_dead(&self, inner: &mut Inner, version: Version) {
        let mut live = Vec::new();
        self.live.borrow_mut().retain(|token| {
            token.upgrade().is_some_and(|token| {
                live.push(token.get());
                true
            })
        });
        inner
            .cached
            .retain(|(cached, _)| *cached == version || live.contains(cached));
    }
}

/// Diffs run on this thread.
#[cfg(test)]
pub(crate) fn diff_count() -> usize {
    tests::DIFFS.with(std::cell::Cell::get)
}

/// Rows of a text, without their line terminators.
pub(crate) struct Rows<'a>(Vec<&'a str>);

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
pub(crate) fn rows<'a>(text: &'a str, scratch: &'a mut String) -> Rows<'a> {
    let text = if text.contains('\r') {
        *scratch = text.replace("\r\n", "\n").replace('\r', "\n");
        scratch.as_str()
    } else {
        text
    };
    Rows(text.split('\n').collect())
}

/// The rows of `text` (LF-normalised or not) that differ from `base`, sorted.
/// Empty when either is over 5 MiB. An empty base has no rows: against it,
/// every row of `text` is added.
pub(crate) fn diff_rows(base: &str, text: &str) -> Arc<[RowChange]> {
    #[cfg(test)]
    tests::DIFFS.with(|count| count.set(count.get() + 1));
    if base.len() > MAX_DIFF_BYTES || text.len() > MAX_DIFF_BYTES {
        return Arc::new([]);
    }
    let (mut base_scratch, mut text_scratch) = (String::new(), String::new());
    if base.is_empty() {
        // A new file's base: it has no rows, so every row is added, but for
        // the empty one after a final line break.
        let rows = rows(text, &mut text_scratch).0.len() as u32;
        let rows = rows - u32::from(text.is_empty() || text.ends_with(['\n', '\r']));
        return match rows {
            0 => Arc::new([]),
            rows => Arc::new([RowChange {
                rows: 0..rows,
                kind: RowChangeKind::Added,
            }]),
        };
    }
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

#[cfg(test)]
#[path = "row_changes_supplied_tests.rs"]
mod row_changes_supplied_tests;
