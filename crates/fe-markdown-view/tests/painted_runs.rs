//! `PaintedRuns` records the runs the renderer builds, only when installed.

use std::sync::Arc;

use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
use fe_markdown::Document;
use fe_markdown_view::{ElementOptions, Images, MarkdownStyle, PaintedRuns, document_element};
use fe_syntax::Highlight;
use gpui::{Context, IntoElement, Render, TestAppContext, Window};
use kiln_ui::UiTheme;

struct Host {
    document: Arc<Document>,
    style: MarkdownStyle,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        document_element(
            &self.document,
            &self.style,
            ElementOptions {
                images: Images::AltText,
                on_link: None,
                code_header: None,
            },
        )
    }
}

const MARKDOWN: &str = "Some prose\n\n```rust\nfn one() {}\n```\n";

fn draw(
    cx: &mut TestAppContext,
    record: bool,
) -> (MarkdownStyle, Vec<(String, Vec<gpui::TextRun>)>) {
    let style = cx.update(|cx| {
        let editor = EditorTheme::from_palette(
            EditorPalette::fastedit_dark(),
            &EditorSettings::default(),
            cx,
        );
        if record {
            PaintedRuns::set(cx);
        }
        MarkdownStyle::new(&editor, &UiTheme::default_dark())
    });
    let host = Host {
        document: Arc::new(fe_markdown::parse(MARKDOWN)),
        style: style.clone(),
    };
    let (_, cx) = cx.add_window_view(|_, _| host);
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    let recorded = cx.update(|_, cx| PaintedRuns::take(cx));
    (style, recorded)
}

#[gpui::test]
fn records_keyword_colour_of_a_fence(cx: &mut TestAppContext) {
    let (style, recorded) = draw(cx, true);
    let keyword = Highlight::from_capture("keyword").expect("a highlight");
    let colour = style.syntax_color(keyword).expect("a keyword colour");
    assert!(recorded.iter().any(|(text, _)| text == "Some prose"));
    let (_, runs) = recorded
        .iter()
        .find(|(text, _)| text == "fn one() {}")
        .expect("the fence");
    // `fn` is the first two bytes.
    assert_eq!(runs[0].len, 2);
    assert_eq!(runs[0].color, colour);
    assert_ne!(runs[0].color, style.foreground);
}

#[gpui::test]
fn nothing_recorded_without_the_global(cx: &mut TestAppContext) {
    let (_, recorded) = draw(cx, false);
    assert!(recorded.is_empty());
    cx.update(|cx| assert!(!cx.has_global::<PaintedRuns>()));
}
