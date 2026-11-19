//! Clickable rows above an editor's rows, as VS Code's CodeLens: a
//! language server's "3 references" or a merge conflict's "Accept Current
//! Change · Accept Incoming Change". A [`LensProvider`], added with
//! [`crate::EditorView::add_lens_provider`], gives [`Lens`]es for the text
//! each frame draws; the providers' lenses over one row make one lens row,
//! a block row above the row's first display row, laid out and painted by
//! [`crate::element`]'s `host_blocks`. A click on an item runs its command.
//!
//! A lens row is set in the design's UI small, the UI's sans family, in
//! faint text, its items `·` separated (`docs/design/design-system.md#editor`,
//! Language-server formatting).

use std::collections::BTreeMap;

use fe_buffer::BufferSnapshot;
use fe_command::Invocation;
use gpui::{Bounds, Pixels, SharedString};

/// Between a lens row's items.
pub(crate) const SEPARATOR: &str = " \u{b7} ";

/// One item of a lens row: clickable, or plain text.
#[derive(Clone, Debug, PartialEq)]
pub struct LensItem {
    pub title: SharedString,
    /// What a click runs, from the view, as a bound click's command. None
    /// for plain text, which a click, the pointer and the palette pass over.
    pub invocation: Option<Invocation>,
    /// Set in the warning colour, not the faint one a lens row is.
    pub warning: bool,
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

/// A lens item a frame laid out, for assistive technology
/// (`view/lens_targets.rs`): its row, its place in the row's items, and
/// the part of it in view, in window coordinates, which its pointer hitbox
/// covers too.
#[derive(Clone, Debug)]
pub(crate) struct LensTarget {
    /// The buffer row the lens sits above.
    pub(crate) row: u32,
    /// Its index in the row's items, from 0.
    pub(crate) index: usize,
    pub(crate) item: LensItem,
    pub(crate) bounds: Bounds<Pixels>,
}

/// What a screen reader calls a lens button: its title without a leading
/// decorative glyph, such as the test lens's "▷ Run test", and the space
/// after it. A glyph is a first character that is neither a letter, a
/// digit nor ASCII, with any variation selectors after it ("▶\u{fe0e}"),
/// followed by white space. A title that is only that is left whole.
pub(crate) fn accessible_name(title: &SharedString) -> SharedString {
    let Some(first) = title.chars().next() else {
        return title.clone();
    };
    if first.is_ascii() || first.is_alphanumeric() {
        return title.clone();
    }
    let rest = &title[first.len_utf8()..];
    let rest = rest.trim_start_matches(|ch| matches!(ch, '\u{fe00}'..='\u{fe0f}'));
    let name = rest.trim_start();
    match name.len() < rest.len() && !name.is_empty() {
        true => SharedString::new(name),
        false => title.clone(),
    }
}
