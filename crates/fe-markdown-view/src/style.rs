//! The preview's colours, fonts and sizes, from the editor's theme and the
//! chrome's.

use std::ops::Range;

use fe_gpui::EditorTheme;
use fe_syntax::{Highlight, HighlightSpan};
use gpui::{Font, FontWeight, Hsla, Pixels, SharedString, TextRun, font, px};
use kiln_ui::UiTheme;

/// How a [`crate::MarkdownView`] draws its document.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownStyle {
    /// Behind the document: the editor's background.
    pub background: Hsla,
    /// Body text: the editor's foreground.
    pub foreground: Hsla,
    /// List markers, image notes and h6 headings.
    pub muted: Hsla,
    /// Heading and table rules, quote bars and horizontal rules.
    pub border: Hsla,
    pub link: Hsla,
    /// Behind code blocks and inline code.
    pub code_background: Hsla,
    /// Behind every other table body row.
    pub row_tint: Hsla,
    pub code_font_family: SharedString,
    pub code_font_features: gpui::FontFeatures,
    pub code_font_size: Pixels,
    /// Code's line height as a multiple of `code_font_size`.
    pub code_line_height: f32,
    /// Code's colour for each [`Highlight`], by [`Highlight::index`]; `None`
    /// uses `foreground`.
    pub syntax: Vec<Option<Hsla>>,
    /// Body text's size, in the window's default font.
    pub text_size: Pixels,
    /// Body text's line height as a multiple of `text_size`.
    pub line_height: f32,
    /// Between blocks: [`MarkdownStyle::BLOCK_GAP`], or
    /// [`MarkdownStyle::COMPACT_GAP`] in the compact form.
    pub block_gap: Pixels,
    /// Around a code block's code: [`MarkdownStyle::CODE_PADDING`], or
    /// [`MarkdownStyle::COMPACT_GAP`] in the compact form.
    pub code_padding: Pixels,
    /// Horizontal rules: `border`, or in the compact form `float_border`,
    /// since `border` hardly shows on a floating surface.
    pub rule: Hsla,
    /// The hairline around a floating surface, the editor's popover
    /// border: what the compact form, drawn on one, draws its rules in.
    pub float_border: Hsla,
}

impl MarkdownStyle {
    /// The document column's widest, as VS Code's preview's.
    pub const MAX_WIDTH: Pixels = px(880.);
    /// Around the document column.
    pub const PADDING: Pixels = px(26.);
    /// Between blocks.
    pub const BLOCK_GAP: Pixels = px(16.);
    /// Around a code block's code.
    pub const CODE_PADDING: Pixels = px(16.);
    /// The compact form's gap between blocks, and its code blocks' padding:
    /// a popover's own padding is 8, so more would float its code.
    pub const COMPACT_GAP: Pixels = px(8.);

    /// The preview of a document in `editor`'s colours and code font, with
    /// the chrome's muted text, borders, links and panels. A light editor
    /// theme gives a light preview.
    pub fn new(editor: &EditorTheme, ui: &UiTheme) -> Self {
        Self {
            background: editor.background,
            foreground: editor.foreground,
            muted: ui.muted_text,
            border: ui.border,
            link: ui.accent,
            code_background: ui.panel_background,
            row_tint: ui.row_hover,
            code_font_family: editor.font_family.clone(),
            code_font_features: editor.font_features.clone(),
            code_font_size: editor.font_size,
            code_line_height: editor.line_height,
            syntax: editor.syntax.clone(),
            text_size: px(14.),
            line_height: 1.6,
            block_gap: Self::BLOCK_GAP,
            code_padding: Self::CODE_PADDING,
            rule: ui.border,
            float_border: editor.popover_border,
        }
    }

    /// This style for a hover popover or completion docs, as VS Code sizes
    /// their text: the editor's font size less one, at line height 1.4,
    /// code at its own (the editor's). Blocks are [`Self::COMPACT_GAP`]
    /// apart, code is padded as much, and rules are the floating surface's
    /// hairline, so they show on it.
    pub fn compact(&self) -> MarkdownStyle {
        MarkdownStyle {
            text_size: self.code_font_size - px(1.),
            line_height: 1.4,
            block_gap: Self::COMPACT_GAP,
            code_padding: Self::COMPACT_GAP,
            rule: self.float_border,
            ..self.clone()
        }
    }

    /// A heading's text size, as VS Code's preview sizes them: h1 2em down
    /// to h6 0.85em.
    pub fn heading_size(&self, level: u8) -> Pixels {
        let em = match level {
            1 => 2.,
            2 => 1.5,
            3 => 1.25,
            4 => 1.,
            5 => 0.875,
            _ => 0.85,
        };
        self.text_size * em
    }

    /// A heading's text colour: h6 is muted.
    pub fn heading_color(&self, level: u8) -> Hsla {
        if level >= 6 {
            self.muted
        } else {
            self.foreground
        }
    }

    pub fn syntax_color(&self, highlight: Highlight) -> Option<Hsla> {
        self.syntax.get(highlight.index()).copied().flatten()
    }

    /// The code font, in its regular weight.
    pub fn code_font(&self) -> Font {
        let mut code = font(self.code_font_family.clone());
        code.features = self.code_font_features.clone();
        code
    }

    /// The runs a code block draws `code` with, covering all of it: each
    /// highlight's span in its syntax colour and the rest in `foreground`,
    /// all in the code font. Highlights are by byte range of `code`, in
    /// order; a span overlapping the one before is drawn from where that
    /// one ends.
    pub fn code_runs(&self, code: &str, highlights: &[HighlightSpan]) -> Vec<TextRun> {
        self.code_runs_in(code, highlights, 0..code.len())
    }

    /// The runs for `code[range]` alone, as [`MarkdownStyle::code_runs`]
    /// draws that stretch of `code`: for a host laying a block's lines out
    /// again in its own rows, keeping their syntax colours. `highlights`
    /// are by byte range of the whole of `code`; the runs cover
    /// `range.len()` bytes, `range` cut to `code`'s length.
    pub fn code_runs_in(
        &self,
        code: &str,
        highlights: &[HighlightSpan],
        range: Range<usize>,
    ) -> Vec<TextRun> {
        let last = range.end.min(code.len());
        let mut runs = Vec::new();
        let mut at = range.start.min(last);
        let mut push = |len: usize, color: Hsla| {
            if len > 0 {
                runs.push(TextRun {
                    len,
                    font: self.code_font(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                });
            }
        };
        for span in highlights {
            let start = span.range.start.max(at).min(last);
            let end = span.range.end.min(last);
            if start >= end || !code.is_char_boundary(start) || !code.is_char_boundary(end) {
                continue;
            }
            push(start - at, self.foreground);
            let color = self.syntax_color(span.highlight).unwrap_or(self.foreground);
            push(end - start, color);
            at = end;
        }
        push(last - at, self.foreground);
        runs
    }

    /// Headings' weight, as VS Code's preview's.
    pub const HEADING_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
}

#[cfg(test)]
mod tests {
    use gpui::hsla;

    use super::*;

    #[test]
    fn code_runs_in_cut_the_runs_to_the_range() {
        let keyword = Highlight::from_capture("keyword").expect("keyword");
        let (foreground, red) = (hsla(0., 0., 0.9, 1.), hsla(0., 1., 0.5, 1.));
        let mut syntax = vec![None; Highlight::all().len()];
        syntax[keyword.index()] = Some(red);
        let style = MarkdownStyle {
            background: hsla(0., 0., 0., 1.),
            foreground,
            muted: foreground,
            border: foreground,
            link: foreground,
            code_background: foreground,
            row_tint: foreground,
            code_font_family: "Menlo".into(),
            code_font_features: gpui::FontFeatures::default(),
            code_font_size: px(13.),
            code_line_height: 1.5,
            syntax,
            text_size: px(14.),
            line_height: 1.6,
            block_gap: MarkdownStyle::BLOCK_GAP,
            code_padding: MarkdownStyle::CODE_PADDING,
            rule: foreground,
            float_border: foreground,
        };
        let code = "let a = 1;\nlet b = 2;\n";
        let span = |range| HighlightSpan {
            range,
            highlight: keyword,
        };
        // `let`, twice.
        let highlights = [span(0..3), span(11..14)];
        let runs = |range| {
            (style.code_runs_in(code, &highlights, range).into_iter())
                .map(|run| (run.len, run.color))
                .collect::<Vec<_>>()
        };
        // The second line's alone, its keyword still coloured.
        assert_eq!(runs(11..21), [(3, red), (7, foreground)]);
        // From inside the first keyword, and across the line break.
        assert_eq!(runs(1..13), [(2, red), (8, foreground), (2, red)]);
        // With no highlight in it, or empty.
        assert_eq!(runs(4..10), [(6, foreground)]);
        assert_eq!(runs(5..5), []);
        // Past the code's end, cut to it.
        assert_eq!(runs(20..40), [(2, foreground)]);
        // The whole of it is code_runs.
        let whole: Vec<_> = (style.code_runs(code, &highlights).into_iter())
            .map(|run| (run.len, run.color))
            .collect();
        assert_eq!(whole, runs(0..code.len()));
    }
}
