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

fn wheel(cx: &mut VisualTestContext, dy: f32) {
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(100.), px(100.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
    });
    settle(cx);
}

fn log_events(
    view: &Entity<MarkdownView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<MarkdownViewEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let log = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(view, move |_, event: &MarkdownViewEvent, _| {
            log.borrow_mut().push(event.clone())
        })
        .detach()
    });
    events
}

#[gpui::test]
fn wheel_overscroll_reads_the_clamped_offset(cx: &mut TestAppContext) {
    let markdown = source();
    let (view, cx) = open(cx, &markdown);
    let events = log_events(&view, cx);

    // Far past the end: the top is where the clamped view has it, which
    // the same line the end scroll reads.
    wheel(cx, -1e6);
    let (offset, max) = view.read_with(cx, |view, _| {
        let handle = view.scroll_handle();
        (handle.offset().y, handle.max_offset().y)
    });
    assert_eq!(-offset, max);
    assert_eq!(*events.borrow(), [MarkdownViewEvent::Scrolled]);
    let end = line_at_top(&view, cx);
    let last = markdown.lines().count() as f32;
    assert!(end < last - 1., "{end} of {last}");

    // Still more is no movement.
    wheel(cx, -1e6);
    assert_eq!(events.borrow().len(), 1);
    assert!((line_at_top(&view, cx) - end).abs() < 0.01);

    // Back up past the start.
    wheel(cx, 1e7);
    assert_eq!(events.borrow().len(), 2);
    assert!(line_at_top(&view, cx).abs() < 0.05);
    wheel(cx, 1e7);
    assert_eq!(events.borrow().len(), 2);
}

#[gpui::test]
fn a_zero_wheel_after_heading_navigation_is_not_a_scroll(cx: &mut TestAppContext) {
    let markdown = source();
    let (view, cx) = open(cx, &markdown);
    let events = log_events(&view, cx);
    view.update(cx, |view, cx| {
        view.scroll_to_heading("section-20", cx);
    });
    settle(cx);
    wheel(cx, 0.);
    assert_eq!(*events.borrow(), []);
}

/// A table, a quote with blank lines between its blocks, and a loose list.
fn nested_source() -> String {
    let mut text = String::new();
    for n in 0..6 {
        text += &format!("Intro {n}.\n\n");
    }
    text += "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |\n\n";
    text += "> quoted one\n>\n> quoted two\n>\n> quoted three\n\n";
    text += "- loose one\n\n- loose two\n\n- loose three\n\n";
    for n in 0..60 {
        text += &format!("Closing {n}.\n\n");
    }
    text
}

#[gpui::test]
fn lines_in_gaps_between_nested_regions_are_monotonic(cx: &mut TestAppContext) {
    let markdown = nested_source();
    let (view, cx) = open(cx, &markdown);
    let start = line_of(&markdown, "| a |");
    let end = line_of(&markdown, "Closing 0");
    let mut previous = f32::NEG_INFINITY;
    let mut line = start;
    while line < end {
        view.update(cx, |view, cx| view.scroll_to_source_line(line, cx));
        settle(cx);
        let top = line_at_top(&view, cx);
        assert!(top >= previous - 0.01, "{line}: {top} after {previous}");
        assert!(top <= line + 1.01 && top >= line - 1.01, "{line}: {top}");
        previous = top;
        line += 0.1;
    }
    // Lines of a block's own, in a quote or list gap, read back close.
    for needle in ["> quoted two", "- loose two", "| 3 | 4 |"] {
        let line = line_of(&markdown, needle);
        view.update(cx, |view, cx| view.scroll_to_source_line(line, cx));
        settle(cx);
        let top = line_at_top(&view, cx);
        assert!((top - line).abs() < 0.05, "{needle}: {top} for {line}");
    }
}
