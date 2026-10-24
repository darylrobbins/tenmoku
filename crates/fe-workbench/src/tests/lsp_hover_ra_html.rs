//! rust-analyzer's hover over `Option::transpose`, whose docs carry
//! rustdoc's `<code>` around links, is drawn without the tags, its links
//! and its fences formatted (T256).

use fe_lsp::fake::{Reply, Scripted};
use fe_markdown::BlockKind;
use fe_markdown_view::MarkdownStyle;
use fe_syntax::Highlight;
use fe_ui::UiTheme;
use gpui::{
    Bounds, Focusable as _, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TestDispatcher, TouchPhase, point,
};
use lsp_types::{LSPAny, LSPObject, request::HoverRequest, request::Request as _};

use super::super::lsp::Lsp;
use super::hover_markdown::painted;

/// rust-analyzer 1.98.1's hover text.
const HOVER: &str = include_str!("../../../fe-markdown/tests/fixtures/ra_hover_transpose.md");

/// The fixture's blocks: two fences, a rule, the types, a rule, the
/// summary, the `<code>` paragraph, the heading and the example.
const EXAMPLE: &str = "md-block-8";

/// Whether `inner` shows any of `outer`.
fn overlaps(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.top() < outer.bottom() && outer.top() < inner.bottom()
}

/// Built by hand rather than by `#[gpui::test]`, to give the window a text
/// system that keeps each line it lays out.
#[test]
fn hover_rustdoc_code_html_is_drawn_as_code() {
    let lines = painted::Lines::default();
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some("hover_rustdoc_code_html_is_drawn_as_code"),
        painted::text_system(lines.clone()),
    );
    hover(&mut cx, &lines);
    cx.run_until_parked();
    cx.update(|cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
}

fn hover(cx: &mut TestAppContext, lines: &painted::Lines) {
    let markup = LSPObject::from_iter([
        ("kind".to_owned(), LSPAny::from("markdown")),
        ("value".to_owned(), LSPAny::from(HOVER)),
    ]);
    let hover = LSPObject::from_iter([("contents".to_owned(), LSPAny::Object(markup))]);
    let mut s = Lsp::new(cx);
    s.fake.set_reply(
        HoverRequest::METHOD,
        Scripted::now(Reply::Result(LSPAny::Object(hover))),
    );
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor("fn ma".len());
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    lines.lock().expect("the lines").clear();

    s.cx.simulate_keystrokes("cmd-k cmd-i");
    s.until("the hover", |s| {
        editor
            .read_with(&s.cx, |view, _| view.popover_text())
            .is_some()
    });
    s.draw();
    // The popover holds the server's Markdown, its fences' code apart.
    let held = (editor.read_with(&s.cx, |view, _| view.popover_text())).expect("shown");
    for line in (HOVER.lines()).filter(|line| !line.is_empty() && !line.starts_with("```")) {
        assert!(held.contains(line), "{line:?} isn't in {held:?}");
    }

    // It's at its height cap, its fences at the top, the example below
    // the fold.
    let content = s.cx.debug_bounds("hover-popover-content").expect("drawn");
    let body = s.cx.debug_bounds("hover-popover-body").expect("drawn");
    assert!(
        body.size.height > content.size.height,
        "{body:?} in {content:?}"
    );
    for fence in ["md-block-0", "md-block-1"] {
        let bounds = s.cx.debug_bounds(fence).expect("the fence is drawn");
        assert!(
            overlaps(bounds, content),
            "{fence} {bounds:?} in {content:?}"
        );
    }
    let example = s.cx.debug_bounds(EXAMPLE).expect("drawn");
    assert!(!overlaps(example, content), "{example:?} in {content:?}");

    // Scrolled past the fences, as in the user's screenshot.
    s.cx.simulate_event(ScrollWheelEvent {
        position: content.center(),
        delta: ScrollDelta::Lines(point(0., -8.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    s.draw();
    for fence in ["md-block-0", "md-block-1"] {
        let bounds = s.cx.debug_bounds(fence).expect("the fence is drawn");
        assert!(
            !overlaps(bounds, content),
            "{fence} {bounds:?} in {content:?}"
        );
    }
    let example = s.cx.debug_bounds(EXAMPLE).expect("drawn");
    assert!(overlaps(example, content), "{example:?} in {content:?}");

    // What the popover laid out shows no tag, link source or escape.
    let shown = lines.lock().expect("the lines").join("\n");
    for tag in ["<code>", "</code>", "](https://", "\\_"] {
        assert!(!shown.contains(tag), "{tag:?} is drawn in {shown:?}");
    }
    let words = shown.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        words.contains("Some(Ok(_)) is mapped to Ok(Some(_))"),
        "{words:?}"
    );

    // The test window paints no glyph, so the fences' colours are resolved
    // as the popover's markdown view draws them, without painting: the
    // fixture parsed by `fe_markdown::parse`, each rust fence's highlights
    // run through `MarkdownStyle::code_runs` in the editor's theme.
    let theme = editor.read_with(&s.cx, |view, _| view.theme().clone());
    let style = MarkdownStyle::new(&theme, &UiTheme::default_dark());
    let keyword = Highlight::from_capture("keyword").expect("a highlight");
    let keyword = style.syntax_color(keyword).expect("a keyword colour");
    assert_eq!(
        Some(keyword),
        theme.syntax_color(Highlight::from_capture("keyword").unwrap())
    );
    assert_ne!(keyword, style.foreground, "keywords look like plain text");
    let mut fences = 0;
    let (mut keywords, mut plain) = (Vec::new(), 0);
    for block in fe_markdown::parse(HOVER).blocks {
        let BlockKind::Code {
            language: Some("rust"),
            code,
            highlights,
            ..
        } = block.kind
        else {
            continue;
        };
        fences += 1;
        let mut at = 0;
        for run in style.code_runs(&code, &highlights) {
            if run.color == keyword {
                keywords.push(code[at..at + run.len].to_owned());
            } else if run.color == style.foreground {
                plain += 1;
            }
            at += run.len;
        }
        assert_eq!(at, code.len(), "the runs cover {code:?}");
    }
    assert_eq!(
        fences, 3,
        "the fixture's rust fences: two atop, one example"
    );
    for word in ["impl", "pub", "const", "fn"] {
        assert!(
            keywords.iter().any(|run| run == word),
            "{word:?} isn't a keyword-coloured run of {keywords:?}"
        );
    }
    assert!(plain > 0, "everything is keyword colour");
}
