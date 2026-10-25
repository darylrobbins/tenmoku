//! Colour swatches a host gives an editor
//! ([`EditorView::set_color_swatches`]): a small square in a colour drawn
//! before the text that spells it, as VS Code's `editor.colorDecorators`
//! draws one before each colour a language server finds.
//!
//! - **Gap.** Each swatch takes a gap one em wide (a space in the editor
//!   font) before its offset, laid out as an inlay hint's text is
//!   ([`crate::inlay_hints`]): the text after it shifts right, the caret
//!   is never inside it, a click in it puts the caret at its offset, and
//!   rows wrap as they do without it. A swatch and a hint at one offset
//!   both show, the hint first.
//! - **Square.** 0.85 em on a side, centred in the gap, with rounded
//!   corners and a 1 px border in the theme's muted grey, so white and
//!   black both stand out on either theme. A colour that isn't opaque is
//!   painted over a checkerboard of two greys from the theme.
//! - **Edits.** Swatches follow edits as hints do until the next set; one
//!   whose offset an edit deleted text strictly around goes, and so do all
//!   once the buffer can't map from where they were set.
//! - Swatches and hints are set apart: setting one never replaces the
//!   other.

use fe_buffer::Version;
use fe_display::layout::{self, Frame};
use gpui::{Bounds, Corners, Hsla, Rgba, Window, fill, point, px, size};

use crate::{EditorTheme, EditorView, element::FrameLayout, element::Line};

/// A colour drawn before a buffer offset.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ColorSwatch {
    pub offset: usize,
    /// Red, green, blue and alpha, each in `0..=1`.
    pub rgba: [f32; 4],
}

/// A view's swatches, with their offsets in the buffer at `synced`.
#[derive(Default)]
pub(crate) struct Swatches {
    synced: Option<Version>,
    swatches: Vec<ColorSwatch>,
}

impl Swatches {
    /// Moves the swatches on to `snapshot`'s version, dropping those an
    /// edit took the text around.
    fn advance(&mut self, snapshot: &fe_buffer::BufferSnapshot) {
        let Some(from) = self.synced else { return };
        if from == snapshot.version() {
            return;
        }
        let offsets: Vec<usize> = self.swatches.iter().map(|swatch| swatch.offset).collect();
        let mut moved = crate::inlay_hints::follow(snapshot, from, &offsets).into_iter();
        self.swatches
            .retain_mut(|swatch| match moved.next().flatten() {
                Some(offset) => {
                    swatch.offset = offset;
                    true
                }
                None => false,
            });
        self.synced = Some(snapshot.version());
    }
}

/// The text a swatch's gap is laid out as: one em in a monospace font.
const GAP: &str = " ";

/// The square's side, of the gap's width.
const SIDE: f32 = 0.85;

impl EditorView {
    /// Shows `swatches`, whose offsets are in the buffer as it is now, in
    /// place of any shown before, from the next frame: see
    /// [`crate::color_swatches`].
    pub fn set_color_swatches(&mut self, mut swatches: Vec<ColorSwatch>) {
        self.model.sync_display();
        let snapshot = self.model.snapshot();
        // Clipped, in order, so a frame lays them out in order.
        for swatch in &mut swatches {
            swatch.offset = snapshot.resolve(&snapshot.anchor_before(swatch.offset));
        }
        swatches.sort_by_key(|swatch| swatch.offset);
        self.color_swatches = Swatches {
            synced: Some(snapshot.version()),
            swatches,
        };
    }

    /// The swatches shown, at their offsets in the buffer now.
    pub fn color_swatches(&self) -> Vec<ColorSwatch> {
        let swatches = &self.color_swatches;
        let Some(from) = swatches.synced else {
            return Vec::new();
        };
        let offsets: Vec<usize> = (swatches.swatches.iter())
            .map(|swatch| swatch.offset)
            .collect();
        (crate::inlay_hints::follow(self.model.snapshot(), from, &offsets).into_iter())
            .zip(&swatches.swatches)
            .filter_map(|(offset, swatch)| {
                Some(ColorSwatch {
                    offset: offset?,
                    rgba: swatch.rgba,
                })
            })
            .collect()
    }

    /// Moves the swatches to the buffer as it is, before a frame is laid
    /// out.
    pub(crate) fn sync_color_swatches(&mut self) {
        if self.color_swatches.synced.is_none() {
            return;
        }
        let snapshot = self.model.snapshot().clone();
        self.color_swatches.advance(&snapshot);
    }
}

/// The gaps a frame lays out, after the view's hints: the frame's hint
/// indexes from `first` on are the swatches'.
pub(crate) fn inlays(view: &EditorView) -> impl Iterator<Item = layout::Inlay<Hsla>> + '_ {
    (view.color_swatches.swatches.iter()).map(|swatch| layout::Inlay {
        offset: swatch.offset,
        text: GAP.to_owned(),
        style: Hsla::transparent_black(),
    })
}

/// A swatch a frame laid out.
pub(crate) struct Shown {
    rgba: [f32; 4],
    rect: layout::Rect,
}

/// The swatches `frame` laid out, the hints it was laid out with from
/// `first` on being the view's swatches.
pub(crate) fn layout(frame: &Frame<Line>, view: &EditorView, first: usize) -> Vec<Shown> {
    (frame.hints.iter())
        .filter_map(|&(ix, rect)| {
            let swatch = view.color_swatches.swatches.get(ix.checked_sub(first)?)?;
            Some(Shown {
                rgba: swatch.rgba,
                rect,
            })
        })
        .collect()
}

/// What a frame painted of a swatch, in window coordinates.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct Painted {
    /// The gap laid out before its offset.
    pub(crate) gap: Bounds<gpui::Pixels>,
    /// The square in it.
    pub(crate) square: Bounds<gpui::Pixels>,
    pub(crate) color: Hsla,
    pub(crate) border: Hsla,
    /// The checkerboard's squares under a colour that isn't opaque, each
    /// with its grey; none under an opaque one.
    pub(crate) checker: Vec<(Bounds<gpui::Pixels>, Hsla)>,
}

/// The checkerboard's two greys: steps of the theme's muted grey over its
/// background.
fn checker_greys(theme: &EditorTheme) -> (Hsla, Hsla) {
    let step = |opacity| (theme.background).blend(theme.gutter_foreground.opacity(opacity));
    (step(0.3), step(0.7))
}

/// Paints the swatches, noting in tests what was painted.
pub(crate) fn paint(
    shown: &[Shown],
    layout: &FrameLayout,
    theme: &EditorTheme,
    window: &mut Window,
    #[cfg(test)] painted: &mut Vec<Painted>,
) {
    let border = theme.gutter_foreground;
    for swatch in shown {
        let gap = layout.bounds(swatch.rect);
        let side = (gap.size.width * SIDE).min(gap.size.height).round();
        let square = Bounds::new(
            gap.origin
                + point(
                    (gap.size.width - side) / 2.,
                    ((gap.size.height - side) / 2.).round(),
                ),
            size(side, side),
        );
        let radius = (side * 0.2).min(px(3.));
        let [r, g, b, a] = swatch.rgba;
        let color: Hsla = Rgba { r, g, b, a }.into();
        #[cfg(test)]
        let mut checker = Vec::new();
        if a < 1. {
            // Two by two: light under all, dark at top left and bottom
            // right, each rounded at the square's corner it's in.
            let (light, dark) = checker_greys(theme);
            let mut base = fill(square, light);
            base.corner_radii = Corners::all(radius);
            window.paint_quad(base);
            let half = side / 2.;
            let cells = [
                (
                    point(px(0.), px(0.)),
                    Corners {
                        top_left: radius,
                        ..Corners::default()
                    },
                ),
                (
                    point(half, half),
                    Corners {
                        bottom_right: radius,
                        ..Corners::default()
                    },
                ),
            ];
            #[cfg(test)]
            checker.push((square, light));
            for (at, corners) in cells {
                let cell = Bounds::new(square.origin + at, size(half, half));
                let mut quad = fill(cell, dark);
                quad.corner_radii = corners;
                window.paint_quad(quad);
                #[cfg(test)]
                checker.push((cell, dark));
            }
        }
        let quad = fill(square, color)
            .corner_radii(radius)
            .border_widths(px(1.))
            .border_color(border);
        window.paint_quad(quad);
        #[cfg(test)]
        painted.push(Painted {
            gap,
            square,
            color,
            border,
            checker,
        });
    }
}

#[cfg(test)]
#[path = "color_swatches_tests.rs"]
mod tests;
