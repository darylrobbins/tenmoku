//! Caret x and hit-testing in rows with right-to-left text (T1431 G1, G2).
//!
//! The shaper paints a right-to-left run's glyphs right to left, each
//! keeping its byte index, but GPUI's `LineLayout::x_for_index`,
//! `closest_index_for_x` and `index_for_x` assume indices rise with x. So
//! for a row with a strong right-to-left char, [`Line`](super::Line)
//! measures from its glyphs itself, each grapheme's direction coming from
//! its Unicode bidi embedding level, which is what the shaper reorders by
//! (docs/decisions/yzegn08wk2-non-latin-text-audit.md, "Bidi fix"). Other
//! rows, found by a byte scan, keep GPUI's maths.

use std::{collections::HashMap, ops::Range};

use gpui::{FontId, LineLayout};
use unicode_bidi::{BidiClass, BidiInfo, bidi_class};
use unicode_segmentation::UnicodeSegmentation as _;

/// The graphemes of a shaped row with right-to-left text, with their
/// directions and where they're painted.
#[derive(Clone, Debug)]
pub(crate) struct Bidi {
    /// In byte order, covering the shaped text.
    graphemes: Vec<Grapheme>,
    /// Indices into `graphemes` of those painted, left to right.
    visual: Vec<usize>,
    width: f32,
    len: usize,
}

#[derive(Clone, Debug)]
struct Grapheme {
    bytes: Range<usize>,
    /// Whether its embedding level is right to left.
    rtl: bool,
    /// Whether it's only bidi controls and zero-width chars, which the
    /// shaper gives no glyph.
    invisible: bool,
    /// From its glyph to the next painted grapheme's, or the line's end,
    /// its share of a ligature's; `None` where it has no glyph, as a bidi
    /// control.
    painted: Option<Range<f32>>,
    /// The advance of its first char in the font of its glyph there, if it
    /// has one; what tells a base from a mark sharing its glyph index.
    advance: Option<f32>,
}

impl Grapheme {
    /// Its edge a caret before it is drawn at.
    fn leading(&self) -> Option<f32> {
        let painted = self.painted.as_ref()?;
        Some(if self.rtl { painted.end } else { painted.start })
    }

    /// Its edge a caret after it is drawn at.
    fn trailing(&self) -> Option<f32> {
        let painted = self.painted.as_ref()?;
        Some(if self.rtl { painted.start } else { painted.end })
    }
}

/// Whether `text` has a strong right-to-left char or an explicit
/// right-to-left control, the chars that make the shaper reorder it. Each
/// is U+0590 or above, so a text with no byte from 0xD6 up has none.
pub(crate) fn has_rtl(text: &str) -> bool {
    text.bytes().any(|byte| byte >= 0xD6)
        && text.chars().any(|ch| {
            matches!(
                bidi_class(ch),
                BidiClass::R | BidiClass::AL | BidiClass::RLE | BidiClass::RLO | BidiClass::RLI
            )
        })
}

impl Bidi {
    /// Of `text` shaped as `layout`, if it has right-to-left text. Its
    /// paragraph direction is its first strong char's, as the shaper's is.
    /// `advance` is a char's advance in a font where the text system knows
    /// it; without one, a mark sharing its base's glyph index is told from
    /// it by the row's mean advance.
    pub(crate) fn with_advances(
        text: &str,
        layout: &LineLayout,
        advance: impl Fn(FontId, char) -> Option<f32>,
    ) -> Option<Self> {
        has_rtl(text).then(|| Self::with_advances_of(text, layout, advance))
    }

    /// Of `text` shaped as `layout`, whatever its chars.
    #[cfg(test)]
    fn of(text: &str, layout: &LineLayout) -> Self {
        Self::with_advances_of(text, layout, |_, _| None)
    }

    fn with_advances_of(
        text: &str,
        layout: &LineLayout,
        advance: impl Fn(FontId, char) -> Option<f32>,
    ) -> Self {
        let info = BidiInfo::new(text, None);
        // Each paragraph's levels as a line ends it, as the shaper lays out
        // one: trailing whitespace takes the paragraph's direction.
        let mut levels = info.levels.clone();
        for paragraph in &info.paragraphs {
            let range = paragraph.range.clone();
            let line = info.reordered_levels(paragraph, range.clone());
            levels[range.clone()].copy_from_slice(&line[range]);
        }
        let mut fonts: HashMap<usize, FontId> = HashMap::new();
        for run in &layout.runs {
            for glyph in &run.glyphs {
                fonts.entry(glyph.index).or_insert(run.font_id);
            }
        }
        let graphemes = (text.grapheme_indices(true))
            .map(|(at, grapheme)| Grapheme {
                bytes: at..at + grapheme.len(),
                rtl: levels[at].is_rtl(),
                invisible: grapheme.chars().all(|ch| {
                    // The directional marks LRM, RLM and ALM, then the
                    // embedding controls and zero-width chars.
                    matches!(ch, '\u{200e}' | '\u{200f}' | '\u{61c}')
                        || matches!(
                            bidi_class(ch),
                            BidiClass::BN
                                | BidiClass::LRE
                                | BidiClass::RLE
                                | BidiClass::LRO
                                | BidiClass::RLO
                                | BidiClass::PDF
                                | BidiClass::LRI
                                | BidiClass::RLI
                                | BidiClass::FSI
                                | BidiClass::PDI
                        )
                }),
                painted: None,
                advance: (fonts.get(&at).copied())
                    .zip(grapheme.chars().next())
                    .and_then(|(font_id, ch)| advance(font_id, ch)),
            })
            .collect();
        let mut bidi = Self {
            graphemes,
            visual: Vec::new(),
            width: 0.,
            len: text.len(),
        };
        bidi.place(layout);
        bidi
    }

    /// The same graphemes painted as `layout`, the same text's glyphs
    /// moved.
    pub(crate) fn placed(&self, layout: &LineLayout) -> Self {
        let mut bidi = self.clone();
        bidi.place(layout);
        bidi
    }

    fn place(&mut self, layout: &LineLayout) {
        let graphemes = &mut self.graphemes;
        // Each grapheme's x: of the glyphs of its first char, which a mark
        // offset left of its base doesn't move; failing those, of any of
        // its glyphs. Where a mark shares its base's index, the base is the
        // glyph whose first char's advance reaches the x of the cluster to
        // its right, or the line's end, working from the right.
        //
        // Known limits, as GPUI gives no per-glyph advance: marks that
        // overlap an adjacent grapheme, their offset past the advance, can't
        // be told from geometry alone; and where `Line::new` gave no
        // advances, the row's mean stands in for the base's, so a row of
        // unequal widths may pick a mark.
        let mut first: Vec<Vec<f32>> = vec![Vec::new(); graphemes.len()];
        let mut any: Vec<Option<f32>> = vec![None; graphemes.len()];
        for glyph in layout.runs.iter().flat_map(|run| &run.glyphs) {
            let at = graphemes.partition_point(|grapheme| grapheme.bytes.end <= glyph.index);
            let Some(grapheme) = graphemes.get(at) else {
                continue;
            };
            let x = f32::from(glyph.position.x);
            if glyph.index == grapheme.bytes.start {
                first[at].push(x);
            } else {
                any[at] = Some(any[at].map_or(x, |left| left.min(x)));
            }
        }
        let width: f32 = layout.width.into();
        let mean = width / first.iter().filter(|xs| !xs.is_empty()).count().max(1) as f32;
        let mut by_x: Vec<(usize, f32)> = (first.iter().enumerate())
            .filter_map(|(at, xs)| Some((at, xs.iter().copied().min_by(f32::total_cmp)?)))
            .collect();
        by_x.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut base: Vec<Option<f32>> = vec![None; graphemes.len()];
        let mut edge = width;
        for &(at, min) in by_x.iter().rev() {
            let xs = &first[at];
            let advance = graphemes[at].advance.unwrap_or(mean);
            let miss = |x: f32| (edge - x - advance).abs();
            let chosen = (xs.iter().copied())
                .min_by(|&a, &b| miss(a).total_cmp(&miss(b)).then(a.total_cmp(&b)))
                .filter(|_| xs.len() > 1)
                .unwrap_or(min);
            base[at] = Some(chosen);
            edge = chosen;
        }
        // A grapheme with a glyph for its first char and the visible
        // graphemes after it with none for theirs, which its glyph draws
        // too, as a lam-alef ligature's, even where the alef has a mark of
        // its own: by their indices into `graphemes`, and their x.
        let mut clusters: Vec<(Range<usize>, f32)> = Vec::new();
        for ix in 0..graphemes.len() {
            let continues = clusters.last().is_some_and(|last| last.0.end == ix);
            match (base[ix], any[ix]) {
                (Some(x), _) => clusters.push((ix..ix + 1, x)),
                (None, _) if continues && !graphemes[ix].invisible => {
                    if let Some(last) = clusters.last_mut() {
                        last.0.end = ix + 1;
                    }
                }
                (None, Some(x)) => clusters.push((ix..ix + 1, x)),
                (None, None) => {}
            }
        }
        clusters.sort_by(|a, b| a.1.total_cmp(&b.1));
        self.width = width;
        for grapheme in graphemes.iter_mut() {
            grapheme.painted = None;
        }
        // Each cluster reaches to the next one's x, and is split evenly
        // between its graphemes in their direction.
        for (at, (members, start)) in clusters.iter().enumerate() {
            let end = clusters.get(at + 1).map_or(self.width, |next| next.1);
            let (start, end) = (*start, end.max(*start));
            let count = members.len();
            let edge = |i: usize| match i {
                0 => start,
                i if i == count => end,
                i => start + (end - start) * i as f32 / count as f32,
            };
            let rtl = graphemes[members.start].rtl;
            for (j, ix) in members.clone().enumerate() {
                let from = if rtl { count - j - 1 } else { j };
                graphemes[ix].painted = Some(edge(from)..edge(from + 1));
            }
        }
        let mut visual: Vec<usize> = (0..graphemes.len())
            .filter(|&ix| graphemes[ix].painted.is_some())
            .collect();
        let start = |ix: usize| {
            graphemes[ix]
                .painted
                .as_ref()
                .map_or(0., |painted| painted.start)
        };
        visual.sort_by(|&a, &b| start(a).total_cmp(&start(b)));
        self.visual = visual;
    }

    /// The x of a caret at `index`: on the trailing edge of the grapheme
    /// before it, so at a direction change the caret stays with the run of
    /// its logical predecessor, as Chromium's and VS Code's do. At the
    /// line's start, on the first grapheme's leading edge. An index inside
    /// a grapheme is taken as its start, and graphemes with no glyph are
    /// passed over.
    pub(crate) fn x_for_index(&self, index: usize) -> f32 {
        let at = (self.graphemes).partition_point(|grapheme| grapheme.bytes.end <= index);
        let (before, after) = self.graphemes.split_at(at);
        (before.iter().rev().find_map(Grapheme::trailing))
            .or_else(|| after.iter().find_map(Grapheme::leading))
            .unwrap_or(0.)
    }

    /// The painted grapheme under `x`: the last starting at or left of it,
    /// or the leftmost.
    fn under(&self, x: f32) -> Option<&Grapheme> {
        let at = (self.visual).partition_point(|&ix| {
            (self.graphemes[ix].painted.as_ref()).is_some_and(|painted| painted.start <= x)
        });
        let ix = *self.visual.get(at.saturating_sub(1))?;
        Some(&self.graphemes[ix])
    }

    /// The boundary closest to `x`: the nearer logical edge of the
    /// grapheme under it, so the right half of a right-to-left letter
    /// gives its start; at or past the line's width, the end.
    pub(crate) fn index_for_x(&self, x: f32) -> usize {
        if x >= self.width {
            return self.len;
        }
        let Some(grapheme) = self.under(x) else {
            return 0;
        };
        let painted = grapheme.painted.clone().unwrap_or(x..x);
        // Halfway is the left half, as in GPUI's `closest_index_for_x`.
        let left_half = x <= (painted.start + painted.end) / 2.;
        match left_half != grapheme.rtl {
            true => grapheme.bytes.start,
            false => grapheme.bytes.end,
        }
    }

    /// Where the grapheme under `x` starts; `None` at or past the line's
    /// width.
    pub(crate) fn char_at(&self, x: f32) -> Option<usize> {
        if x >= self.width {
            return None;
        }
        Some(self.under(x).map_or(0, |grapheme| grapheme.bytes.start))
    }

    /// Where the graphemes of `range` are painted, left to right, adjacent
    /// ones merged: one piece per visual run. An empty range, or one of no
    /// painted grapheme, is one empty piece at its start's caret.
    pub(crate) fn visual_ranges(&self, range: Range<usize>) -> Vec<Range<f32>> {
        let mut pieces: Vec<Range<f32>> = Vec::new();
        if !range.is_empty() {
            for &ix in &self.visual {
                let grapheme = &self.graphemes[ix];
                if grapheme.bytes.start >= range.end || grapheme.bytes.end <= range.start {
                    continue;
                }
                let Some(painted) = grapheme.painted.clone() else {
                    continue;
                };
                match pieces.last_mut() {
                    Some(last) if last.end == painted.start => last.end = painted.end,
                    _ => pieces.push(painted),
                }
            }
        }
        if pieces.is_empty() {
            let x = self.x_for_index(range.start);
            pieces.push(x..x);
        }
        pieces
    }
}

#[cfg(test)]
#[path = "bidi_tests.rs"]
mod tests;
