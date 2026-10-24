//! Inline content as `StyledText` runs: emphasis, strong, strikethrough,
//! inline code and links, with images in the flow of the text.

use std::{ops::Range, rc::Rc};

use fe_markdown::Inline;
use gpui::{
    AnyElement, Font, FontStyle, FontWeight, Hsla, InteractiveElement as _, InteractiveText,
    IntoElement, ParentElement as _, StatefulInteractiveElement as _, StrikethroughStyle,
    Styled as _, StyledText, TextRun, UnderlineStyle, Window, div, px,
};

use crate::{MarkdownStyle, blocks, image, view::RenderCx};

/// A block's inline content, laid out.
pub(crate) enum Inlines {
    /// Without images: one text, its hard breaks newlines in it.
    Text(Runs),
    /// With images: lines, one per hard break, each the words and images
    /// in it, which wrap as one flow.
    Flow(Vec<Vec<Piece>>),
}

/// A word, with the spaces after it, or an image, in a line of a flow.
pub(crate) enum Piece {
    Text(Runs),
    Image {
        dest: String,
        alt: String,
        /// The link the image is in.
        link: Option<LinkTo>,
    },
}

/// Text and the runs that style it, with the byte ranges of its links.
#[derive(Default)]
pub(crate) struct Runs {
    pub(crate) text: String,
    pub(crate) runs: Vec<TextRun>,
    links: Vec<Link>,
}

struct Link {
    range: Range<usize>,
    to: LinkTo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinkTo {
    pub(crate) dest: String,
    /// The link's place among the document's links, which hover is by.
    pub(crate) ordinal: usize,
}

/// The base a block's inlines are styled from: the block's font, weight
/// and colour.
#[derive(Clone, Copy)]
pub(crate) struct Base {
    pub(crate) weight: FontWeight,
    pub(crate) color: Hsla,
}

#[derive(Clone, Copy)]
struct Marks {
    weight: FontWeight,
    italic: bool,
    struck: bool,
    code: bool,
    link: Option<usize>,
}

/// Lays inlines out as [`Inlines`], keeping each open link's range across
/// the pieces the text is cut into.
struct Builder {
    base: Base,
    flow: bool,
    runs: Runs,
    /// The links the text being pushed is in, outermost first, and where
    /// each starts in `runs`.
    open: Vec<(usize, LinkTo)>,
    line: Vec<Piece>,
    lines: Vec<Vec<Piece>>,
}

/// `inlines` laid out in `base`'s weight and colour.
pub(crate) fn layout(inlines: &[Inline], base: Base, cx: &mut RenderCx) -> Inlines {
    let mut builder = Builder {
        base,
        flow: has_image(inlines),
        runs: Runs::default(),
        open: Vec::new(),
        line: Vec::new(),
        lines: Vec::new(),
    };
    let marks = Marks {
        weight: base.weight,
        italic: false,
        struck: false,
        code: false,
        link: None,
    };
    builder.push_inlines(inlines, marks, cx);
    if !builder.flow {
        return Inlines::Text(builder.runs);
    }
    builder.end_line();
    Inlines::Flow(builder.lines)
}

fn has_image(inlines: &[Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Image { .. } => true,
        Inline::Emphasis(content)
        | Inline::Strong(content)
        | Inline::Strikethrough(content)
        | Inline::Link { content, .. } => has_image(content),
        _ => false,
    })
}

impl Builder {
    fn push_inlines(&mut self, inlines: &[Inline], marks: Marks, cx: &mut RenderCx) {
        for inline in inlines {
            match inline {
                Inline::Text(text) | Inline::Html(text) => self.push_text(text, marks, cx),
                Inline::Code(text) => self.push_text(
                    text,
                    Marks {
                        code: true,
                        ..marks
                    },
                    cx,
                ),
                Inline::Emphasis(content) => {
                    let marks = Marks {
                        italic: true,
                        ..marks
                    };
                    self.push_inlines(content, marks, cx);
                }
                Inline::Strong(content) => {
                    let marks = Marks {
                        weight: FontWeight::BOLD,
                        ..marks
                    };
                    self.push_inlines(content, marks, cx);
                }
                Inline::Strikethrough(content) => {
                    let marks = Marks {
                        struck: true,
                        ..marks
                    };
                    self.push_inlines(content, marks, cx);
                }
                Inline::Link { dest, content, .. } => {
                    let ordinal = cx.next_link();
                    let to = LinkTo {
                        dest: dest.clone(),
                        ordinal,
                    };
                    self.open.push((self.runs.text.len(), to));
                    let marks = Marks {
                        link: Some(ordinal),
                        ..marks
                    };
                    self.push_inlines(content, marks, cx);
                    if let Some((start, to)) = self.open.pop() {
                        self.runs.add_link(start, to);
                    }
                }
                Inline::Image { dest, alt, .. } => {
                    self.end_piece();
                    self.line.push(Piece::Image {
                        dest: dest.clone(),
                        alt: alt.clone(),
                        link: self.open.last().map(|(_, to)| to.clone()),
                    });
                }
                Inline::SoftBreak => self.push_text(" ", marks, cx),
                Inline::HardBreak if self.flow => self.end_line(),
                Inline::HardBreak => self.push_text("\n", marks, cx),
            }
        }
    }

    /// Adds `text`; in a flow, each word with the spaces after it is a
    /// piece of its own, and a newline in inline HTML is a space.
    fn push_text(&mut self, text: &str, marks: Marks, cx: &RenderCx) {
        if !self.flow {
            self.runs.push(text, marks, self.base, cx);
            return;
        }
        let text = text.replace('\n', " ");
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let word = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let spaces =
                (rest[word..].find(|c: char| !c.is_whitespace())).unwrap_or(rest.len() - word);
            let (piece, after) = rest.split_at(word + spaces);
            self.runs.push(piece, marks, self.base, cx);
            if spaces > 0 {
                self.end_piece();
            }
            rest = after;
        }
    }

    /// Ends the text piece being built, if any, the open links carrying on
    /// into the next one.
    fn end_piece(&mut self) {
        if self.runs.text.is_empty() {
            return;
        }
        for (start, to) in &mut self.open {
            self.runs.add_link(*start, to.clone());
            *start = 0;
        }
        self.line.push(Piece::Text(std::mem::take(&mut self.runs)));
    }

    fn end_line(&mut self) {
        self.end_piece();
        self.lines.push(std::mem::take(&mut self.line));
    }
}

/// The font a run with `marks` is drawn in, `body` being the body text's.
fn run_font(marks: Marks, body: &Font, style: &MarkdownStyle) -> Font {
    let mut font = if marks.code {
        style.code_font()
    } else {
        body.clone()
    };
    font.weight = marks.weight;
    if marks.italic {
        font.style = FontStyle::Italic;
    }
    font
}

impl Runs {
    fn push(&mut self, text: &str, marks: Marks, base: Base, cx: &RenderCx) {
        if text.is_empty() {
            return;
        }
        let style = cx.style;
        let color = if marks.link.is_some() {
            style.link
        } else {
            base.color
        };
        let hovered = marks.link.is_some_and(|link| cx.is_hovered(link));
        self.text.push_str(text);
        self.runs.push(TextRun {
            len: text.len(),
            font: run_font(marks, &cx.font, style),
            color,
            background_color: marks.code.then_some(style.code_background),
            underline: hovered.then_some(UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            }),
            strikethrough: marks.struck.then_some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            }),
        });
    }

    /// Makes the text from `start` on a link to `to`.
    fn add_link(&mut self, start: usize, to: LinkTo) {
        if start < self.text.len() {
            self.links.push(Link {
                range: start..self.text.len(),
                to,
            });
        }
    }

    /// The text drawn with its runs, its links clickable and underlined
    /// while hovered.
    pub(crate) fn render(self, cx: &mut RenderCx) -> AnyElement {
        let id = cx.next_text();
        crate::PaintedRuns::record(cx.app, &self.text, &self.runs);
        let text = StyledText::new(self.text).with_runs(self.runs);
        if self.links.is_empty() {
            return text.into_any_element();
        }
        let ranges: Vec<Range<usize>> = self.links.iter().map(|link| link.range.clone()).collect();
        let links = Rc::new(self.links);
        // The link at a char index of the text.
        let link_at = {
            let links = links.clone();
            Rc::new(move |index: Option<usize>| {
                let index = index?;
                (links.iter())
                    .find(|link| link.range.contains(&index))
                    .map(|link| link.to.clone())
            })
        };
        let layout = text.layout().clone();
        let text = {
            let sink = cx.sink.clone();
            InteractiveText::new(("md-text", id), text)
                .on_click(ranges, {
                    let sink = sink.clone();
                    move |ix, window: &mut Window, cx| {
                        sink.clicked(links[ix].to.dest.clone(), window, cx);
                    }
                })
                .on_hover({
                    let (sink, link_at) = (sink, link_at.clone());
                    move |index, _event, _window, cx| sink.hovered(id, link_at(index), cx)
                })
        };
        // The text reports the char under the pointer only while the
        // pointer is over it and the char changes, so the box around it
        // tells of the pointer leaving, and of it coming back to the char
        // it left from.
        let sink = cx.sink.clone();
        div()
            .id(("md-link-text", id))
            .on_hover(move |hovered, window, cx| {
                let link = if *hovered {
                    link_at(layout.index_for_position(window.mouse_position()).ok())
                } else {
                    None
                };
                sink.hovered(id, link, cx);
            })
            .child(text)
            .into_any_element()
    }
}

/// `inlines` drawn, as the children of a column: the text, or each line
/// of the flow.
pub(crate) fn render(inlines: Inlines, cx: &mut RenderCx) -> Vec<AnyElement> {
    match inlines {
        Inlines::Text(runs) => vec![runs.render(cx)],
        Inlines::Flow(lines) => lines
            .into_iter()
            .map(|line| {
                let pieces: Vec<AnyElement> =
                    line.into_iter().map(|piece| piece.render(cx)).collect();
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_end()
                    .children(pieces)
                    .into_any_element()
            })
            .collect(),
    }
}

impl Piece {
    fn render(self, cx: &mut RenderCx) -> AnyElement {
        match self {
            Piece::Text(runs) => runs.render(cx),
            Piece::Image { dest, alt, link } => {
                let image = if alt_text_only() {
                    alt_text(&alt, cx)
                } else {
                    image::render(&dest, &alt, cx)
                };
                let Some(to) = link else {
                    return image;
                };
                let (id, sink) = (cx.next_text(), cx.sink.clone());
                div()
                    .id(("md-image-link", id))
                    .cursor_pointer()
                    .on_click(move |_, window, cx| sink.clicked(to.dest.clone(), window, cx))
                    .child(image)
                    .into_any_element()
            }
        }
    }
}

/// Whether images are drawn as their alt text alone, as a
/// [`crate::document_element`] with [`crate::Images::AltText`] asks.
fn alt_text_only() -> bool {
    blocks::element().is_some_and(|element| element.alt_text)
}

/// An image as its alt text, muted, with nothing read. Its
/// `debug_selector` is `md-image-alt-<n>`, `n` being its place among the
/// document's images.
fn alt_text(alt: &str, cx: &mut RenderCx) -> AnyElement {
    let n = cx.next_image();
    div()
        .debug_selector(|| format!("md-image-alt-{n}"))
        .text_color(cx.style.muted)
        .child(alt.to_owned())
        .into_any_element()
}

/// How wide `inlines` are on their widest line, unwrapped.
pub(crate) fn max_width(inlines: &Inlines, cx: &mut RenderCx) -> f32 {
    match inlines {
        // A hard break's line is measured on its own.
        Inlines::Text(runs) => (split_lines(&runs.text, &runs.runs).into_iter())
            .map(|(text, runs)| cx.shaped_width(text, &runs))
            .fold(0., f32::max),
        Inlines::Flow(lines) => (lines.iter())
            .map(|line| {
                (line.iter())
                    .map(|piece| match piece {
                        Piece::Text(runs) => cx.shaped_width(runs.text.clone(), &runs.runs),
                        Piece::Image { alt, .. } if alt_text_only() => cx.text_width(alt),
                        Piece::Image { dest, alt, .. } => image::width(dest, alt, cx),
                    })
                    .sum::<f32>()
            })
            .fold(0., f32::max),
    }
}

/// `text`'s lines and the part of `runs` each covers.
fn split_lines(text: &str, runs: &[TextRun]) -> Vec<(String, Vec<TextRun>)> {
    let mut lines = Vec::new();
    let mut runs = runs.iter().cloned().peekable();
    let mut carried: Option<TextRun> = None;
    for line in text.split('\n') {
        let mut line_runs = Vec::new();
        let mut need = line.len();
        while need > 0 {
            let Some(mut run) = carried.take().or_else(|| runs.next()) else {
                break;
            };
            if run.len > need {
                let mut rest = run.clone();
                rest.len = run.len - need;
                run.len = need;
                carried = Some(rest);
            }
            need -= run.len;
            line_runs.push(run);
        }
        // Skip the newline's byte.
        if let Some(mut run) = carried.take().or_else(|| runs.next()) {
            run.len -= 1;
            if run.len > 0 {
                carried = Some(run);
            }
        }
        lines.push((line.to_owned(), line_runs));
    }
    lines
}

#[cfg(test)]
mod tests {
    use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
    use gpui::{Hsla, TestAppContext, TextRun, font};
    use kiln_ui::UiTheme;

    use super::*;

    fn run(len: usize) -> TextRun {
        TextRun {
            len,
            font: font("Menlo"),
            color: Hsla::default(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }
    }

    #[test]
    fn lines_take_their_part_of_the_runs() {
        let lines = split_lines("ab\ncd", &[run(1), run(3), run(1)]);
        let lens: Vec<(String, Vec<usize>)> = (lines.into_iter())
            .map(|(text, runs)| (text, runs.iter().map(|run| run.len).collect()))
            .collect();
        assert_eq!(
            lens,
            [("ab".to_owned(), vec![1, 1]), ("cd".to_owned(), vec![1, 1])]
        );
    }

    #[gpui::test]
    fn inline_code_keeps_the_weight_around_it(cx: &mut TestAppContext) {
        let style = cx.update(|cx| {
            let editor = EditorTheme::from_palette(
                EditorPalette::fastedit_dark(),
                &EditorSettings::default(),
                cx,
            );
            MarkdownStyle::new(&editor, &UiTheme::default_dark())
        });
        let body = font("Helvetica");
        let code = |weight| Marks {
            weight,
            italic: false,
            struck: false,
            code: true,
            link: None,
        };
        // `**`code`**`, and code in a heading.
        for weight in [FontWeight::BOLD, MarkdownStyle::HEADING_WEIGHT] {
            let font = run_font(code(weight), &body, &style);
            assert_eq!(
                (font.family, font.weight),
                (style.code_font_family.clone(), weight)
            );
        }
    }
}
