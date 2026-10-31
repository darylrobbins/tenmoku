//! The minimap in a headless window: the text area narrows by its width,
//! its runs are the text's colours at their display rows, and the slider
//! covers the rows the editor shows, while the pointer is over it or always.

use fe_buffer::Buffer;
use fe_display::minimap::{MinimapOptions, MinimapShowSlider};
use fe_editor::EditorModel;
use gpui::{
    Bounds, Entity, Focusable as _, Hsla, Modifiers, MouseExitEvent, Pixels, Point, TestAppContext,
    VisualTestContext, point, px, size,
};

use super::RUN_ALPHA;
use crate::{EditorTheme, EditorView, element::minimap::Painted, init};

/// A Rust editor on `text` in an 800 × 400 window, drawn once parsed.
fn editor<'a>(
    text: &str,
    soft_wrap: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    editor_with(text, "main.rs", soft_wrap, cx)
}

/// An editor on `text`, unwrapped, as the file at `path`.
fn editor_on<'a>(
    text: &str,
    path: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    editor_with(text, path, false, cx)
}

fn editor_with<'a>(
    text: &str,
    path: &str,
    soft_wrap: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (text, path) = (text.to_owned(), path.to_owned());
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), Some(path.into()));
        model.set_soft_wrap(soft_wrap);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn set_minimap(
    view: &Entity<EditorView>,
    options: Option<MinimapOptions>,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| view.set_minimap(options, cx));
    draw(cx);
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Painted> {
    view.read_with(cx, |view, _| view.painted_minimap.clone())
}

/// `rows` numbered rows of Rust.
fn numbered(rows: usize) -> String {
    (0..rows)
        .map(|n| format!("let value_{n} = {n};\n"))
        .collect()
}

/// Scrolls so display row `row` is at the top, and draws.
fn scroll_to(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let line_height = view.layout.as_ref().expect("drawn").frame.line_height;
        view.scroll.y = px(row as f32 * line_height);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
}

/// The y the minimap draws display row `row` at, in the window.
fn row_y(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Pixels {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let minimap = layout.minimap.as_ref().expect("shown");
        layout.origin.y + px(minimap.geometry.row_y(row))
    })
}

#[gpui::test]
fn the_text_area_and_soft_wrap_narrow_by_the_minimap(cx: &mut TestAppContext) {
    let (view, cx) = editor(&"word ".repeat(400), true, cx);
    // Wide enough for all its columns.
    cx.simulate_resize(size(px(1400.), px(400.)));
    draw(cx);
    let shown = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let frame = &view.layout.as_ref().expect("drawn").frame;
            let wrap = view.model.display().wrap_width().expect("soft wrap is on");
            (frame.width, wrap, view.model.display().row_count())
        })
    };
    let (width, wrap, rows) = shown(cx);
    assert_eq!(width, 1400.);
    assert!(painted(&view, cx).is_none(), "off by default");

    set_minimap(&view, Some(MinimapOptions::default()), cx);
    let minimap = painted(&view, cx).expect("shown");
    // VS Code's default: 120 columns, a pixel each.
    assert_eq!(minimap.bounds.size.width, px(120.));
    assert_eq!(minimap.bounds.right(), px(1400.));
    let (narrowed, narrowed_wrap, narrowed_rows) = shown(cx);
    assert_eq!(narrowed, 1280.);
    // Soft wrap goes by whole columns.
    let less = wrap - narrowed_wrap;
    assert!((less - 120.).abs() < 10., "{wrap} to {narrowed_wrap}");
    assert!(
        narrowed_rows > rows,
        "{narrowed_rows} vs {rows}: wraps earlier"
    );

    set_minimap(&view, None, cx);
    assert_eq!(shown(cx), (width, wrap, rows));
    assert!(painted(&view, cx).is_none());
}

#[gpui::test]
fn a_narrow_editor_hides_the_minimap(cx: &mut TestAppContext) {
    let (view, cx) = editor("fn main() {}\n", false, cx);
    set_minimap(&view, Some(MinimapOptions::default()), cx);
    assert!(painted(&view, cx).is_some());
    // Too narrow for 2 of its columns beside the text's.
    cx.simulate_resize(size(px(12.), px(400.)));
    draw(cx);
    assert!(painted(&view, cx).is_none());
    let width = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").frame.width
    });
    assert_eq!(width, 12.);
    // Somewhat narrow, it shrinks to the text's columns.
    cx.simulate_resize(size(px(300.), px(400.)));
    draw(cx);
    let minimap = painted(&view, cx).expect("shown");
    assert!(minimap.bounds.size.width < px(120.), "{:?}", minimap.bounds);
}

#[gpui::test]
fn runs_paint_in_the_texts_colours_over_the_background_at_their_display_rows(
    cx: &mut TestAppContext,
) {
    let text = "fn main() {\n    let x = 1;\n    let y = 2;\n}\n// done\n";
    let (view, cx) = editor(text, false, cx);
    set_minimap(&view, Some(MinimapOptions::default()), cx);
    let minimap = painted(&view, cx).expect("shown");
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    assert_eq!(minimap.background, theme.minimap_background);
    let (left, top) = (minimap.bounds.left(), minimap.bounds.top());
    // A row is 2 pixels tall, a column a pixel wide.
    let run = |x: f32, y: f32, width: f32| {
        Bounds::new(point(left + px(x), top + px(y)), size(px(width), px(2.)))
    };
    let drawn = |offset: usize, cx: &mut VisualTestContext| -> Hsla {
        let color = view.read_with(cx, |view, _| view.drawn_color(offset));
        color.expect("drawn").opacity(RUN_ALPHA)
    };
    let fn_ = drawn(0, cx);
    let main = drawn(3, cx);
    assert_ne!(fn_, main, "a keyword and a function name");
    assert_eq!(minimap.runs[0], (run(0., 0., 2.), fn_));
    assert_eq!(minimap.runs[1], (run(3., 0., 4.), main));
    // Indented by 4 columns on the next row.
    let let_ = text.find("let x").unwrap();
    let second = (minimap.runs.iter()).find(|(bounds, _)| bounds.top() == top + px(2.));
    assert_eq!(second, Some(&(run(4., 2., 3.), drawn(let_, cx))));

    // Folded away, rows 1 and 2 aren't display rows: `}` is drawn on the
    // second.
    view.update(cx, |view, cx| {
        view.model.fold_rows(0..=3);
        cx.notify();
    });
    draw(cx);
    let minimap = painted(&view, cx).expect("shown");
    let ys: Vec<_> = (minimap.runs.iter())
        .map(|(bounds, _)| bounds.top() - top)
        .collect();
    let comment = drawn(text.find("// done").unwrap(), cx);
    let last = minimap.runs.last().expect("runs");
    assert_eq!(last.1, comment);
    assert_eq!(last.0.top() - top, px(2.), "{ys:?}");
}

#[gpui::test]
fn the_slider_covers_the_visible_rows_at_the_top_middle_and_end(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(300), false, cx);
    let always = MinimapOptions {
        show_slider: MinimapShowSlider::Always,
        ..MinimapOptions::default()
    };
    set_minimap(&view, Some(always), cx);
    let (rows, visible) = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        (
            view.model.display().row_count(),
            frame.height / frame.line_height,
        )
    });
    let slider =
        |cx: &mut VisualTestContext| painted(&view, cx).expect("shown").slider.expect("always");
    let minimap = painted(&view, cx).expect("shown").bounds;

    let (top, color) = slider(cx);
    assert_eq!(
        color,
        view.read_with(cx, |view, _| view.theme.minimap_slider)
    );
    assert_eq!(top.top(), minimap.top());
    assert_eq!(
        (top.left(), top.size.width),
        (minimap.left(), minimap.size.width)
    );
    assert_eq!(top.size.height, px(visible * 2.));
    assert_eq!(row_y(&view, 0, cx), minimap.top());

    // The minimap scrolls with the editor, and the slider stays over the
    // rows it shows.
    scroll_to(&view, 150, cx);
    let (middle, _) = slider(cx);
    assert!(middle.top() > top.top());
    assert!((middle.top() - row_y(&view, 150, cx)).abs() < px(0.01));
    assert!((middle.size.height - top.size.height).abs() < px(0.01));

    // Down to the last row at the top: the slider reaches the bottom.
    scroll_to(&view, rows - 1, cx);
    let (end, _) = slider(cx);
    assert!((end.top() - row_y(&view, rows - 1, cx)).abs() < px(0.01));
    assert!(
        (end.bottom() - minimap.bottom()).abs() < px(0.01),
        "{end:?} {minimap:?}"
    );
}

/// Moves the pointer to `position` and draws.
fn move_to(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(position, None, Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn the_slider_shows_on_hover_with_mouseover_and_always_with_always(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(300), false, cx);
    cx.update(|window, _| window.activate_window());
    set_minimap(&view, Some(MinimapOptions::default()), cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    let slider = |cx: &mut VisualTestContext| painted(&view, cx).expect("shown").slider;
    let minimap = painted(&view, cx).expect("shown").bounds;
    assert_eq!(slider(cx), None, "the pointer hasn't come over it");

    let below_slider = point(minimap.center().x, minimap.bottom() - px(4.));
    move_to(below_slider, cx);
    let (_, color) = slider(cx).expect("over the minimap");
    assert_eq!(color, theme.minimap_slider);
    let on_slider = point(minimap.center().x, minimap.top() + px(4.));
    move_to(on_slider, cx);
    assert_eq!(
        slider(cx).map(|(_, color)| color),
        Some(theme.minimap_slider_hover)
    );
    move_to(point(px(300.), px(100.)), cx);
    assert_eq!(slider(cx), None, "over the text");
    move_to(on_slider, cx);
    cx.simulate_event(MouseExitEvent {
        position: on_slider,
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    draw(cx);
    assert_eq!(slider(cx), None, "the pointer left the window");

    let always = MinimapOptions {
        show_slider: MinimapShowSlider::Always,
        ..MinimapOptions::default()
    };
    set_minimap(&view, Some(always), cx);
    assert_eq!(
        slider(cx).map(|(_, color)| color),
        Some(theme.minimap_slider)
    );
}

#[gpui::test]
fn a_diff_views_editors_show_no_minimap(cx: &mut TestAppContext) {
    cx.update(init);
    let (diff, cx) = cx.add_window_view(|_, cx| {
        let left = EditorModel::new(Buffer::new("fn a() {}\n"), None);
        let right = EditorModel::new(Buffer::new("fn b() {}\n"), Some("a.rs".into()));
        let left = crate::DiffSide::new("Index", left).read_only();
        crate::DiffView::new(
            "a.rs",
            left,
            crate::DiffSide::new("Working Tree", right),
            cx,
        )
    });
    draw(cx);
    let sides = diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()]);
    for side in sides {
        assert_eq!(side.read_with(cx, |view, _| view.minimap()), None);
        assert!(painted(&side, cx).is_none());
        let drawn = side.read_with(cx, |view, _| view.layout.is_some());
        assert!(drawn);
    }
}

/// A lens above every row.
struct EveryRow;

impl crate::LensProvider for EveryRow {
    fn lenses(&mut self, snapshot: &fe_buffer::BufferSnapshot) -> Vec<crate::Lens> {
        (0..snapshot.text().row_count())
            .map(|row| crate::Lens {
                row,
                items: vec![crate::LensItem {
                    title: "1 reference".into(),
                    invocation: fe_command::Invocation::of("test.lens").unwrap(),
                }],
            })
            .collect()
    }
}

#[gpui::test]
fn with_lens_rows_the_slider_covers_only_the_text_rows_shown(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(300), false, cx);
    let always = MinimapOptions {
        show_slider: MinimapShowSlider::Always,
        ..MinimapOptions::default()
    };
    view.update(cx, |view, cx| {
        view.add_lens_provider(std::rc::Rc::new(std::cell::RefCell::new(EveryRow)));
        view.set_minimap(Some(always), cx);
    });
    draw(cx);
    scroll_to(&view, 101, cx);
    let (shown, first, screen_rows, slider_rows) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let minimap = layout.minimap.as_ref().expect("shown");
        let geometry = &minimap.geometry;
        let frame = &layout.frame;
        let shown: Vec<u32> = frame.rows.iter().map(|row| row.row).collect();
        let slider_rows = geometry.slider_height / geometry.row_height;
        (
            shown,
            frame
                .block_map
                .to_display_top(frame.scroll.y / frame.line_height),
            minimap.screen_rows,
            slider_rows,
        )
    });
    // Half the screen is lens rows.
    assert!(
        slider_rows < screen_rows * 0.6,
        "{slider_rows} of {screen_rows}"
    );
    let (top, last) = (shown[0] as f32, *shown.last().unwrap() as f32 + 1.);
    assert!(first >= top - 1. && first <= top, "{first} vs {shown:?}");
    assert!(
        (first + slider_rows - last).abs() <= 1.,
        "{first} + {slider_rows} vs {shown:?}"
    );
}

#[gpui::test]
fn re_entering_the_window_over_the_minimap_shows_the_slider(cx: &mut TestAppContext) {
    let (view, cx) = editor(&numbered(300), false, cx);
    cx.update(|window, _| window.activate_window());
    set_minimap(&view, Some(MinimapOptions::default()), cx);
    let minimap = painted(&view, cx).expect("shown").bounds;
    let over = point(minimap.center().x, minimap.bottom() - px(4.));
    move_to(over, cx);
    cx.simulate_event(MouseExitEvent {
        position: over,
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    cx.run_until_parked();
    let hover = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.minimap_hover.over);
    assert!(!hover(cx));
    // Back in, the first move finds the minimap under the pointer, with no
    // draw to reconcile it.
    cx.simulate_mouse_move(over, None, Modifiers::none());
    cx.run_until_parked();
    assert!(hover(cx));
}

#[gpui::test]
fn a_cold_fence_seen_only_in_the_minimap_gets_its_colours(cx: &mut TestAppContext) {
    let mut text: String = (0..100).map(|n| format!("Paragraph {n}.\n")).collect();
    text.push_str("```ruby\ndef greet(name)\n  name\nend\n```\n");
    // Markdown is warm, so no load of its own draws again: the minimap's
    // query is the last to find Ruby missing. No other test of this crate loads
    // Ruby, so the cache is cold however many tests share the process.
    for name in ["markdown", "markdown_inline"] {
        fe_syntax::language_by_name(name).expect("compiled in");
    }
    assert!(fe_syntax::loaded_language("ruby").is_none(), "cold");
    let (view, cx) = editor_on(&text, "notes.md", cx);
    assert!(
        fe_syntax::loaded_language("ruby").is_none(),
        "the text alone lacks it"
    );
    // The minimap's first frame is drawn as the test app draws a window
    // its views notified, when effects flush; nothing else draws again.
    view.update(cx, |view, cx| {
        view.set_minimap(Some(MinimapOptions::default()), cx)
    });
    cx.run_until_parked();
    assert!(fe_syntax::loaded_language("ruby").is_some(), "loaded");
    let (def, plain) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let minimap = layout.minimap.as_ref().expect("shown");
        assert!(
            !layout.frame.rows.iter().any(|row| row.buffer_row > 90),
            "off screen"
        );
        let y = layout.origin.y + px(minimap.geometry.row_y(101));
        let painted = view.painted_minimap.as_ref().expect("painted");
        let def = (painted.runs.iter()).find(|(bounds, _)| bounds.top() == y);
        (
            def.expect("a run").1,
            view.theme.foreground.opacity(RUN_ALPHA),
        )
    });
    assert_ne!(def, plain, "`def` is coloured");
}
