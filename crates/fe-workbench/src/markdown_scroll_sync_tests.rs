//! A Markdown preview and its source scrolling together by source line,
//! each putting the other where it is without echo, and the preview staying
//! at the source's line through a re-render.

use std::{fs, time::Duration};

use fe_gpui::EditorView;
use gpui::{
    Entity, Modifiers, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px,
};

use super::draw;
use crate::{
    tests::open_root,
    workbench_commands::markdown_preview::{DELAY, MarkdownPreview},
};

/// A long Markdown file: a paragraph on every even row, a long one first
/// so soft wrap makes rows of it, and a heading on row 80.
fn source_text() -> String {
    let mut text = format!("{}\n\n", "a long first paragraph ".repeat(40));
    for n in 1..100 {
        if n == 40 {
            text += "# Chapter\n\n";
        } else {
            text += &format!("Paragraph {n} of the file.\n\n");
        }
    }
    text
}

struct Opened {
    _dir: tempfile::TempDir,
    source: Entity<EditorView>,
    preview: Entity<MarkdownPreview>,
}

/// README.md open with soft wrap on, and its preview to the side.
fn opened(cx: &mut TestAppContext) -> (Opened, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("README.md");
    fs::write(&path, source_text()).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|window, cx| window.focus(&gpui::Focusable::focus_handle(&workbench, cx), cx));
    draw(cx);
    cx.simulate_keystrokes("alt-z");
    draw(cx);
    cx.simulate_keystrokes("cmd-k v");
    for _ in 0..3 {
        draw(cx);
    }
    let source = workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).unwrap().clone()
    });
    let preview = workbench.read_with(cx, |workbench, cx| {
        let panes = workbench
            .pane_group(cx)
            .panes()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        panes
            .iter()
            .find_map(|pane| {
                pane.read(cx)
                    .tabs()
                    .iter()
                    .find_map(|tab| tab.item().downcast_ref::<MarkdownPreview>().cloned())
            })
            .expect("a preview")
    });
    (
        Opened {
            _dir: dir,
            source,
            preview,
        },
        cx,
    )
}

fn wheel(at: Point<Pixels>, dy: f32, cx: &mut VisualTestContext) {
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), px(-dy))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    for _ in 0..3 {
        draw(cx);
    }
}

/// Where the left half (the source) and the right half (the preview) are.
fn sides(cx: &mut VisualTestContext) -> (Point<Pixels>, Point<Pixels>) {
    let size = cx.update(|window, _| window.bounds().size);
    let y = size.height / 2.;
    (point(size.width * 0.25, y), point(size.width * 0.75, y))
}

fn editor_line(source: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    source
        .read_with(cx, |source, _| source.top_buffer_line())
        .expect("drawn")
}

fn preview_line(preview: &Entity<MarkdownPreview>, cx: &mut VisualTestContext) -> f32 {
    preview
        .read_with(cx, |preview, cx| {
            preview.view().read(cx).source_line_at_top()
        })
        .expect("drawn")
}

#[gpui::test]
fn editor_scroll_moves_the_preview_to_the_same_source_line(cx: &mut TestAppContext) {
    let (
        Opened {
            source, preview, ..
        },
        cx,
    ) = opened(cx);
    let (left, _) = sides(cx);
    assert!(preview_line(&preview, cx) < 1.);

    // Wheel the source until buffer row 40 is at its top.
    for _ in 0..8 {
        let top = editor_line(&source, cx);
        if (top - 40.).abs() < 0.5 {
            break;
        }
        wheel(left, (40. - top) * 20., cx);
    }
    let top = editor_line(&source, cx);
    assert!((top - 40.).abs() < 1., "the source's top is {top}");
    let line = preview_line(&preview, cx);
    assert!(
        (line - top).abs() < 1.,
        "the preview is at {line}, the source at {top}"
    );

    // The editor wasn't moved back in response.
    for _ in 0..3 {
        draw(cx);
    }
    assert_eq!(editor_line(&source, cx), top);
    assert_eq!(preview_line(&preview, cx), line);
}

#[gpui::test]
fn preview_scroll_moves_the_editor_to_the_same_source_line(cx: &mut TestAppContext) {
    let (
        Opened {
            source, preview, ..
        },
        cx,
    ) = opened(cx);
    let (_, right) = sides(cx);
    let heading = 80.;

    // Wheel the preview until the heading is at its top.
    for _ in 0..12 {
        let line = preview_line(&preview, cx);
        if (line - heading).abs() < 0.3 {
            break;
        }
        wheel(right, (heading - line) * 12., cx);
    }
    let line = preview_line(&preview, cx);
    assert!((line - heading).abs() < 1., "the preview is at {line}");
    let top = editor_line(&source, cx);
    assert!((top - heading).abs() < 1., "the source is at {top}");

    // No echo back.
    for _ in 0..3 {
        draw(cx);
    }
    assert_eq!(preview_line(&preview, cx), line);
    assert_eq!(editor_line(&source, cx), top);

    // A re-render after an edit keeps the preview at the source's line.
    source.update(cx, |source, cx| {
        let end = source.model().buffer().text().len();
        let edit = vec![(end..end, "\nMore.\n".to_owned())];
        source
            .model_mut()
            .buffer_mut()
            .edit(edit, fe_buffer::Origin::User);
        cx.notify();
    });
    draw(cx);
    cx.executor()
        .advance_clock(DELAY + Duration::from_millis(1));
    for _ in 0..3 {
        draw(cx);
    }
    let after = editor_line(&source, cx);
    assert!((after - top).abs() < 0.01, "the source moved to {after}");
    let kept = preview_line(&preview, cx);
    assert!(
        (kept - after).abs() < 1.,
        "the preview is at {kept}, the source at {after}"
    );
}
