//! Scrolling a Markdown view by source line.

use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
use fe_markdown_view::{MarkdownStyle, MarkdownView, MarkdownViewEvent};
use gpui::{
    Entity, Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px,
};
use kiln_ui::UiTheme;

/// Sections of a heading and two paragraphs, then a list and a table.
fn source() -> String {
    let mut text = String::new();
    for n in 0..30 {
        text +=
            &format!("## Section {n}\n\nA paragraph of text in section {n}.\n\nAnother one.\n\n");
    }
    text += "- first item\n- second item\n- third item\n\n";
    text += "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |\n\n";
    for n in 0..80 {
        text += &format!("Closing paragraph {n}.\n\n");
    }
    text
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
) -> (Entity<MarkdownView>, &'a mut VisualTestContext) {
    let style = cx.update(|cx| {
        let editor = EditorTheme::from_palette(
            EditorPalette::fastedit_dark(),
            &EditorSettings::default(),
            cx,
        );
        MarkdownStyle::new(&editor, &UiTheme::default_dark())
    });
    let document = Arc::new(fe_markdown::parse(markdown));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        MarkdownView::new(document, PathBuf::from("/doc.md"), None, style, cx)
    });
    settle(cx);
    (view, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn line_at_top(view: &Entity<MarkdownView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| view.source_line_at_top())
        .expect("a frame has been drawn")
}

fn line_of(markdown: &str, needle: &str) -> f32 {
    markdown
        .lines()
        .position(|line| line.starts_with(needle))
        .unwrap_or_else(|| panic!("no line {needle}")) as f32
}

#[gpui::test]
fn source_lines_and_scroll_offsets_map_both_ways(cx: &mut TestAppContext) {
    let markdown = source();
    let (view, cx) = open(cx, &markdown);
    let events = Rc::new(RefCell::new(Vec::new()));
    let log = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &MarkdownViewEvent, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach()
    });
    let first = line_at_top(&view, cx);
    assert!(first.abs() < 0.05, "{first}");

    // A block's first line goes to the top, and reads back.
    for needle in [
        "## Section 12",
        "A paragraph of text in section 20",
        "Closing paragraph 5",
    ] {
        let line = line_of(&markdown, needle);
        view.update(cx, |view, cx| view.scroll_to_source_line(line, cx));
        settle(cx);
        let top = line_at_top(&view, cx);
        assert!((top - line).abs() < 0.05, "{needle}: {top} for {line}");
    }

    // A line inside a list item or a table row lands on that item or row.
    for needle in ["- second item", "| 3 | 4 |"] {
        let line = line_of(&markdown, needle);
        view.update(cx, |view, cx| view.scroll_to_source_line(line, cx));
        settle(cx);
        let top = line_at_top(&view, cx);
        assert!((top - line).abs() < 0.05, "{needle}: {top} for {line}");
    }

    // Halfway through a paragraph of one line is as far down it.
    let line = line_of(&markdown, "## Section 3") + 2.5;
    view.update(cx, |view, cx| view.scroll_to_source_line(line, cx));
    settle(cx);
    let top = line_at_top(&view, cx);
    assert!((top - line).abs() < 0.1, "{top} for {line}");

    // Past the end scrolls to the end.
    view.update(cx, |view, cx| view.scroll_to_source_line(1e6, cx));
    settle(cx);
    let (offset, max) = view.read_with(cx, |view, _| {
        let handle = view.scroll_handle();
        (handle.offset().y, handle.max_offset().y)
    });
    assert_eq!(-offset, max);

    // Nothing so far was the user's.
    assert_eq!(*events.borrow(), []);
    view.update(cx, |view, cx| {
        view.scroll_to_heading("section-5", cx);
    });
    settle(cx);
    assert_eq!(*events.borrow(), []);

    // A wheel scroll is.
    let before = line_at_top(&view, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(100.), px(100.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-300.))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    settle(cx);
    assert_eq!(*events.borrow(), [MarkdownViewEvent::Scrolled]);
    assert!(line_at_top(&view, cx) > before);
}
