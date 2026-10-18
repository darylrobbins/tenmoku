//! The preview's colours, fonts and sizes, from the editor's theme and the
//! chrome's.

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
}

impl MarkdownStyle {
    /// The document column's widest, as VS Code's preview's.
    pub const MAX_WIDTH: Pixels = px(880.);
    /// Around the document column.
    pub const PADDING: Pixels = px(26.);
    /// Between blocks.
    pub const BLOCK_GAP: Pixels = px(16.);

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
            code_font_size: editor.font_size,
            code_line_height: editor.line_height,
            syntax: editor.syntax.clone(),
            text_size: px(14.),
            line_height: 1.6,
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
        font(self.code_font_family.clone())
    }

    /// The runs a code block draws `code` with, covering all of it: each
    /// highlight's span in its syntax colour and the rest in `foreground`,
    /// all in the code font. Highlights are by byte range of `code`, in
    /// order; a span overlapping the one before is drawn from where that
    /// one ends.
    pub fn code_runs(&self, code: &str, highlights: &[HighlightSpan]) -> Vec<TextRun> {
        let mut runs = Vec::new();
        let mut at = 0;
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
            let start = span.range.start.max(at).min(code.len());
            let end = span.range.end.min(code.len());
            if start >= end || !code.is_char_boundary(start) || !code.is_char_boundary(end) {
                continue;
            }
            push(start - at, self.foreground);
            let color = self.syntax_color(span.highlight).unwrap_or(self.foreground);
            push(end - start, color);
            at = end;
        }
        push(code.len() - at, self.foreground);
        runs
    }

    /// Headings' weight, as VS Code's preview's.
    pub const HEADING_WEIGHT: FontWeight = FontWeight::SEMIBOLD;
}
