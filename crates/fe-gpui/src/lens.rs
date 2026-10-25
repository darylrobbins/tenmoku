//! Clickable rows above an editor's rows, as VS Code's CodeLens: a
//! language server's "3 references" or a merge conflict's "Accept Current
//! Change | Accept Incoming Change". A [`LensProvider`], added with
//! [`crate::EditorView::add_lens_provider`], gives [`Lens`]es for the text
//! each frame draws; the providers' lenses over one row make one lens row,
//! a block row above the row's first display row, laid out and painted by
//! [`crate::element`]'s `host_blocks`. A click on an item runs its command.

use std::collections::BTreeMap;

use fe_buffer::BufferSnapshot;
use fe_command::Invocation;
use gpui::{Bounds, Pixels, SharedString};

/// One clickable item of a lens row.
#[derive(Clone, Debug, PartialEq)]
pub struct LensItem {
    pub title: SharedString,
    /// What a click runs, from the view, as a bound click's command.
    pub invocation: Invocation,
}

/// Items over a row.
#[derive(Clone, Debug, PartialEq)]
pub struct Lens {
    /// The buffer row it sits above.
    pub row: u32,
    pub items: Vec<LensItem>,
}

/// A source of lenses, registered with an [`crate::EditorView`].
pub trait LensProvider {
    /// The lenses over `snapshot`'s rows: asked each frame for the snapshot
    /// drawn, so a provider computes them from the text, caching by
    /// [`BufferSnapshot::version`], or returns what a host stored, as a
    /// decoration provider does.
    fn lenses(&mut self, snapshot: &BufferSnapshot) -> Vec<Lens>;
}

/// The lenses of `lenses`, in provider order, joined by row, in row order:
/// one lens a row, without the rows that have no items.
pub(crate) fn join(lenses: impl IntoIterator<Item = Lens>) -> Vec<Lens> {
    let mut rows = BTreeMap::<u32, Vec<LensItem>>::new();
    for lens in lenses {
        rows.entry(lens.row).or_default().extend(lens.items);
    }
    (rows.into_iter())
        .filter(|(_, items)| !items.is_empty())
        .map(|(row, items)| Lens { row, items })
        .collect()
}

/// A lens row a frame painted, for clicks: the part of it in view, and of
/// each of its items, in window coordinates.
#[derive(Clone, Debug)]
pub(crate) struct PaintedRow {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) items: Vec<(LensItem, Bounds<Pixels>)>,
    /// Of the items in view, whether each was drawn underlined, for tests.
    #[cfg(test)]
    pub(crate) underlined: Vec<bool>,
    /// The bytes of each item's title shaped, for tests.
    #[cfg(test)]
    pub(crate) shaped: Vec<usize>,
}
