//! Clicking and hovering sticky scroll's band: a click on a pinned row puts
//! the caret at its header's first non-blank and shows the header directly
//! under its parent's band; it's a jump, for the host's history and vim's
//! jumplist, and leaves vim's visual mode; a row under the pointer is
//! painted in the hover colour, and the band keeps the pointer from the
//! text, the fold column and the hover popover under it.

use std::{cell::RefCell, rc::Rc, time::Duration};

use fe_buffer::{Buffer, BufferId, Version};
use fe_editor::EditorModel;
use fe_vim::{Remap, RemapMode};
use gpui::{
    Entity, Focusable as _, InputEvent as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
    Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext,
    point, px, size,
};

use fe_editor::decorations::Tint;
use gpui::SharedString;

use crate::{
    EditorTheme, EditorView, HoverContent, HoverFuture, HoverProvider,
    commands::{EditorViewEvent, HostBlock, HostBlockKind},
    element::sticky_scroll::Painted,
    hover::HOVER_DELAY,
    init,
    vim::{set_vim_enabled, set_vim_remaps},
};

/// The impl's header row, 0-based.
const IMPL: u32 = 4;
/// The function's header row, two rows into the impl.
const FN: u32 = 7;

/// Rust with a 43-row function two rows into an impl (rows 4 to 52), then
/// 40 rows of constants.
fn source() -> String {
    let mut text = String::from(
        "struct Point {\n    x: i32,\n}\n\nimpl Point {\n    const ORIGIN: i32 = 0;\n\n    fn new(x: i32) -> Self {\n",
    );
    for n in 0..40 {
        text.push_str(&format!("        let value_{n} = \"{n}\";\n"));
    }
    text.push_str("        Self { x }\n    }\n\n    fn other(&self) {}\n}\n");
    for n in 0..40 {
        text.push_str(&format!("const C_{n}: u32 = {n};\n"));
    }
    text
}

/// Answers every hover with some text, counting the requests.
#[derive(Default)]
struct Provider {
    calls: RefCell<usize>,
}

impl HoverProvider for Provider {
    fn hover(&self, _: BufferId, _: Version, _: usize) -> HoverFuture {
        *self.calls.borrow_mut() += 1;
        let content = HoverContent {
            text: "docs".into(),
            range: None,
        };
        Box::pin(async move { Some(content) })
    }
}

/// An editor on [`source`] as `main.rs`, in an 800 × 400 window, focused,
/// vim `on`, with the hover provider, drawn once parsed.
fn editor(
    on: bool,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, Rc<Provider>, &mut VisualTestContext) {
    editor_of(source(), on, cx)
}

/// [`editor`] on `text`.
fn editor_of(
    text: String,
    on: bool,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, Rc<Provider>, &mut VisualTestContext) {
    cx.update(init);
    let provider = Rc::new(Provider::default());
    let set = provider.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_hover_provider(Some(set));
        view
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
        set_vim_enabled(cx, on);
    });
    draw(cx);
    (view, provider, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn line_height(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").frame.line_height
    })
}

/// Puts the caret at the start of buffer row `row`, then scrolls so row
/// `top` is at the top, and draws.
fn caret_and_top(view: &Entity<EditorView>, row: u32, top: u32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let start = view.model.text().row_start(row);
        view.model.set_cursor(start);
        view.changed(cx);
    });
    draw(cx);
    let line_height = line_height(view, cx);
    view.update(cx, |view, cx| {
        view.scroll.y = px(top as f32 * line_height);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
}

fn band(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Painted> {
    view.read_with(cx, |view, _| view.painted_sticky_scroll.clone())
}

/// The buffer rows pinned, top first, and each row's top from the band's.
fn pinned(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, Pixels)> {
    let Some(band) = band(view, cx) else {
        return Vec::new();
    };
    (band.rows.iter())
        .map(|row| (row.buffer_row, row.bounds.origin.y - band.bounds.origin.y))
        .collect()
}

/// The middle of the pinned row of `header`, right of the gutter.
fn on_row(view: &Entity<EditorView>, header: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    let band = band(view, cx).expect("pinned");
    let row = (band.rows.iter())
        .find(|row| row.buffer_row == header)
        .expect("the header is pinned");
    row.bounds.center()
}

/// Clicks the pinned row of `header` with `modifiers` held.
fn click(view: &Entity<EditorView>, header: u32, modifiers: Modifiers, cx: &mut VisualTestContext) {
    let position = on_row(view, header, cx);
    cx.simulate_click(position, modifiers);
}

/// The caret's offset and whether the selection is empty.
fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (usize, bool) {
    view.read_with(cx, |view, _| {
        let selection = view.model.primary_selection();
        (selection.head(), selection.range.is_empty())
    })
}

fn caret_row(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> u32 {
    let (offset, _) = caret(view, cx);
    view.read_with(cx, |view, _| view.model.text().offset_to_point(offset).row)
}

/// Where the text area draws buffer row `row`'s top, in the window.
fn row_top(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Option<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let frame = &layout.frame;
        let shown = frame.rows.iter().find(|shown| shown.buffer_row == row)?;
        Some(layout.to_window(frame.line_origin(shown)).y)
    })
}

/// The first non-blank of buffer row `row`.
fn first_non_blank(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| {
        let text = view.model.text();
        let start = text.row_start(row);
        let line = text.slice_to_string(start..start + text.row_len(row) as usize);
        start + line.len() - line.trim_start().len()
    })
}

/// The events `view` emits from now on.
fn heard(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> Rc<RefCell<Vec<EditorViewEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let pushed = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(view, move |_, event: &EditorViewEvent, _| {
            pushed.borrow_mut().push(event.clone())
        })
        .detach()
    });
    events
}

#[gpui::test]
fn a_click_on_a_pinned_row_shows_its_header_under_its_parent(cx: &mut TestAppContext) {
    for on in [false, true] {
        let (view, _, cx) = editor(on, cx);
        let line_height = line_height(&view, cx);
        caret_and_top(&view, 30, 20, cx);
        assert_eq!(pinned(&view, cx), [(IMPL, px(0.)), (FN, px(line_height))]);

        click(&view, FN, Modifiers::none(), cx);
        draw(cx);
        let header = first_non_blank(&view, FN, cx);
        assert_eq!(caret(&view, cx), (header, true), "vim {on}");
        // One row above the header at the top: the impl's band, and the
        // header itself directly under it, as itself.
        let scroll = view.read_with(cx, |view, _| view.scroll.y);
        assert_eq!(scroll, px((FN - 1) as f32 * line_height), "vim {on}");
        assert_eq!(pinned(&view, cx), [(IMPL, px(0.))], "vim {on}");
        let bottom = band(&view, cx).unwrap().bounds.bottom();
        assert_eq!(row_top(&view, FN, cx), Some(bottom));

        // The impl's own row, with nothing above it, goes to the top.
        caret_and_top(&view, 30, 20, cx);
        click(&view, IMPL, Modifiers::none(), cx);
        draw(cx);
        assert_eq!(caret(&view, cx), (first_non_blank(&view, IMPL, cx), true));
        assert_eq!(
            view.read_with(cx, |view, _| view.scroll.y),
            px(IMPL as f32 * line_height)
        );
        assert_eq!(band(&view, cx), None);
    }
}

#[gpui::test]
fn a_click_on_a_pinned_row_is_a_jump(cx: &mut TestAppContext) {
    // The host hears where from, for its history.
    let (view, _, cx) = editor(false, cx);
    caret_and_top(&view, 30, 20, cx);
    let from = caret(&view, cx).0;
    let events = heard(&view, cx);
    click(&view, FN, Modifiers::none(), cx);
    draw(cx);
    let jumps: Vec<_> = (events.borrow().iter())
        .filter(|event| matches!(event, EditorViewEvent::Jumped { .. }))
        .cloned()
        .collect();
    assert_eq!(jumps, [EditorViewEvent::Jumped { from }]);

    // With vim on, `''` goes back to the row it left.
    let (view, _, cx) = editor(true, cx);
    caret_and_top(&view, 30, 20, cx);
    let events = heard(&view, cx);
    click(&view, FN, Modifiers::none(), cx);
    draw(cx);
    assert_eq!(caret_row(&view, cx), FN);
    assert!(events.borrow().contains(&EditorViewEvent::Jumped {
        from: view.read_with(cx, |view, _| view.model.text().row_start(30))
    }));
    cx.simulate_keystrokes("' '");
    draw(cx);
    assert_eq!(caret_row(&view, cx), 30);
    // And again to the header.
    cx.simulate_keystrokes("' '");
    assert_eq!(caret_row(&view, cx), FN);
}

#[gpui::test]
fn a_click_on_a_pinned_row_leaves_visual_mode(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(true, cx);
    caret_and_top(&view, 30, 20, cx);
    cx.simulate_keystrokes("v j j");
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("visual")
    );
    assert!(!caret(&view, cx).1);
    click(&view, FN, Modifiers::none(), cx);
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );
    assert_eq!(caret(&view, cx), (first_non_blank(&view, FN, cx), true));
}

#[gpui::test]
fn the_band_takes_the_pointer_from_what_is_under_it(cx: &mut TestAppContext) {
    let (view, provider, cx) = editor(false, cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    caret_and_top(&view, 30, 20, cx);
    // Off the band, over the text.
    let below = row_top(&view, 30, cx).unwrap() + px(2.);
    cx.simulate_mouse_move(point(px(400.), below), None, Modifiers::none());
    draw(cx);
    let band_before = band(&view, cx).unwrap();
    assert!((band_before.rows.iter()).all(|row| row.background == theme.sticky_scroll_background));

    // Resting on the function's row: it's painted in the hover colour, the
    // other isn't, and the text under it isn't asked about.
    let over = on_row(&view, FN, cx);
    cx.simulate_mouse_move(over, None, Modifiers::none());
    cx.executor()
        .advance_clock(HOVER_DELAY + Duration::from_millis(100));
    draw(cx);
    let band_now = band(&view, cx).unwrap();
    let colors: Vec<_> = band_now.rows.iter().map(|row| row.background).collect();
    assert_eq!(
        colors,
        [theme.sticky_scroll_background, theme.sticky_scroll_hover]
    );
    assert_eq!(*provider.calls.borrow(), 0);
    assert_eq!(view.read_with(cx, |view, _| view.popover_text()), None);

    // Over the fold column under the band, the chevrons don't show.
    let fold_column = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.fold_column).center().x
    });
    cx.simulate_mouse_move(point(fold_column, over.y), None, Modifiers::none());
    draw(cx);
    assert!(!view.read_with(cx, |view, _| view.fold_column_hovered));
    cx.simulate_mouse_move(point(fold_column, below), None, Modifiers::none());
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.fold_column_hovered));

    // Off the band, the row goes back to the band's colour, and the text
    // is hovered as ever.
    let on_word = view.update(cx, |view, _| {
        let offset = view.model.text().row_start(30) + "        let val".len();
        let caret = view.layout.as_mut().unwrap().caret_bounds(offset).unwrap();
        point(caret.left() + px(1.), caret.center().y)
    });
    cx.simulate_mouse_move(on_word, None, Modifiers::none());
    cx.executor()
        .advance_clock(HOVER_DELAY + Duration::from_millis(100));
    draw(cx);
    let band_after = band(&view, cx).unwrap();
    assert!((band_after.rows.iter()).all(|row| row.background == theme.sticky_scroll_background));
    assert_eq!(*provider.calls.borrow(), 1);

    // A click with shift held on the impl's row reveals it too, extending
    // no selection from the caret under it.
    caret_and_top(&view, 30, 20, cx);
    click(&view, IMPL, Modifiers::shift(), cx);
    draw(cx);
    assert_eq!(caret(&view, cx), (first_non_blank(&view, IMPL, cx), true));
}

#[gpui::test]
fn the_wheel_over_the_band_scrolls_the_view(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(false, cx);
    let line_height = line_height(&view, cx);
    caret_and_top(&view, 30, 20, cx);
    let before = caret(&view, cx);
    let position = on_row(&view, FN, cx);
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0., -3.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    let scroll = view.read_with(cx, |view, _| view.scroll.y);
    assert!(scroll > px(20. * line_height), "{scroll:?}");
    assert_eq!(caret(&view, cx), before);
}

#[gpui::test]
fn a_click_in_insert_mode_is_a_jump_for_vim_too(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(true, cx);
    caret_and_top(&view, 30, 20, cx);
    cx.simulate_keystrokes("i");
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("insert")
    );
    click(&view, FN, Modifiers::none(), cx);
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("insert")
    );
    assert_eq!(caret(&view, cx), (first_non_blank(&view, FN, cx), true));
    cx.simulate_keystrokes("escape ' '");
    assert_eq!(caret_row(&view, cx), 30);
}

/// Vim on with `xy` mapped, so a lone `x` is held, and `x` mapped to `to`.
fn held(to: &str, cx: &mut VisualTestContext) {
    let map = |from: &str, to: &str| Remap {
        modes: vec![RemapMode::Normal],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    };
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &[map("xy", "l"), map("x", to)], ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
}

/// Presses on the pinned row of `header` and lets a press that waits for
/// held keys run, a frame or two on.
fn press_waiting(view: &Entity<EditorView>, header: u32, cx: &mut VisualTestContext) {
    let position = on_row(view, header, cx);
    cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    for _ in 0..6 {
        cx.update(|window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
    }
    cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::none());
    draw(cx);
}

#[gpui::test]
fn a_click_resolves_held_keys_first(cx: &mut TestAppContext) {
    for to in ["dd", "<Cmd>fe:test.ping<CR>dd"] {
        let (view, _, cx) = editor(true, cx);
        cx.update(|_, cx| {
            kiln_platform::commands::register_app_commands(cx, |registry| {
                let ping = |_: &mut &mut gpui::App, _: &fe_command::Actor| {
                    Ok(fe_command::Dispatched::new(fe_command::Effect::Nothing))
                };
                registry.register("test.ping", "test.ping", ping).ok();
            })
        });
        held(to, cx);
        caret_and_top(&view, 30, 20, cx);
        let rows = view.read_with(cx, |view, _| view.model.text().row_count());
        let below = view.read_with(cx, |view, _| {
            let text = view.model.text();
            text.slice_to_string(text.row_start(31)..text.row_start(32))
        });
        cx.simulate_keystrokes("x");
        press_waiting(&view, FN, cx);
        // The held `x` deleted the caret's row, not the header's, then the
        // press revealed the header as it now stands.
        let text = view.read_with(cx, |view, _| view.model.text().clone());
        assert_eq!(text.row_count(), rows - 1, "{to}");
        assert_eq!(
            text.slice_to_string(text.row_start(30)..text.row_start(31)),
            below,
            "{to}"
        );
        assert_eq!(
            caret(&view, cx),
            (first_non_blank(&view, FN, cx), true),
            "{to}"
        );
        // From the row the deletion left the caret on.
        cx.simulate_keystrokes("' '");
        assert_eq!(caret_row(&view, cx), 30, "{to}");
    }
}

#[gpui::test]
fn a_click_after_unused_autoindent_lands_on_the_header(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(true, cx);
    let line_height = line_height(&view, cx);
    caret_and_top(&view, 1, 0, cx);
    // A row opened below `x: i32,`, its autoindent typed over by nothing,
    // above the headers, which move down a row.
    cx.simulate_keystrokes("o");
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("insert")
    );
    let opened = view.read_with(cx, |view, _| view.model.text().row_len(2));
    assert!(opened > 0, "autoindented");
    let events = heard(&view, cx);
    view.update(cx, |view, cx| {
        view.scroll.y = px(21. * line_height);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
    assert_eq!(
        pinned(&view, cx),
        [(IMPL + 1, px(0.)), (FN + 1, px(line_height))]
    );

    click(&view, FN + 1, Modifiers::none(), cx);
    draw(cx);
    // At the header in the text the session left, whatever went from the
    // opened row.
    assert_eq!(caret(&view, cx), (first_non_blank(&view, FN + 1, cx), true));
    // Under its parent's band, found in the syntax of that text, and so it
    // stays.
    for _ in 0..2 {
        assert_eq!(pinned(&view, cx), [(IMPL + 1, px(0.))]);
        let bottom = band(&view, cx).unwrap().bounds.bottom();
        assert_eq!(row_top(&view, FN + 1, cx), Some(bottom));
        draw(cx);
    }
    let row_two = view.read_with(cx, |view, _| {
        let text = view.model.text();
        text.row_start(2) + text.row_len(2) as usize
    });
    let jumps: Vec<_> = (events.borrow().iter())
        .filter(|event| matches!(event, EditorViewEvent::Jumped { .. }))
        .cloned()
        .collect();
    assert_eq!(jumps, [EditorViewEvent::Jumped { from: row_two }]);
    cx.simulate_keystrokes("escape ' '");
    assert_eq!(caret_row(&view, cx), 2);
}

#[gpui::test]
fn a_click_that_ends_a_block_insert_lands_on_the_headers_first_non_blank(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(true, cx);
    let line_height = line_height(&view, cx);
    // A block insert at the start of the rows from the impl's constant to
    // the function's header, which only the first row shows until it ends.
    caret_and_top(&view, IMPL + 1, 0, cx);
    cx.simulate_keystrokes("ctrl-v j j shift-i");
    cx.simulate_input("/**/");
    draw(cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("insert")
    );
    let header = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let text = view.model.text();
            text.slice_to_string(text.row_start(FN)..text.row_start(FN + 1))
        })
    };
    assert!(header(cx).starts_with("    fn new"), "{:?}", header(cx));
    view.update(cx, |view, cx| {
        view.scroll.y = px(20. * line_height);
        view.autoscroll = false;
        cx.notify();
    });
    draw(cx);
    assert_eq!(pinned(&view, cx), [(IMPL, px(0.)), (FN, px(line_height))]);

    // The click ends the insert, which fills the header's start: the caret
    // goes to what's first there now.
    click(&view, FN, Modifiers::none(), cx);
    draw(cx);
    assert!(header(cx).starts_with("/**/    fn new"), "{:?}", header(cx));
    let start = view.read_with(cx, |view, _| view.model.text().row_start(FN));
    assert_eq!(first_non_blank(&view, FN, cx), start);
    assert_eq!(caret(&view, cx), (start, true));
    assert_eq!(pinned(&view, cx), [(IMPL, px(0.))]);
    let bottom = band(&view, cx).unwrap().bounds.bottom();
    assert_eq!(row_top(&view, FN, cx), Some(bottom));
}

/// Puts the view's syntax back to unparsed, its parses going to the
/// background from now on, landing only as the test executor runs them:
/// its clock moves a second on at each read, always past the budget. Not
/// drawn, so the last frame's band stays to click.
pub(super) fn unparse(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        let language = view.syntax.as_ref().expect("parsed").syntax().language();
        let start = std::time::Instant::now();
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let clock = move || {
            elapsed.set(elapsed.get() + Duration::from_secs(1));
            start + elapsed.get()
        };
        let driver = crate::view::syntax_driver(language, view.model.snapshot());
        view.syntax = Some(driver.with_clock(clock));
    });
}

/// A frame drawn with nothing run in the background first, or after: a
/// parse under way stays so.
fn draw_before_parse(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn a_click_reveals_its_header_once_parsed(cx: &mut TestAppContext) {
    let (view, _, cx) = editor(true, cx);
    let line_height = line_height(&view, cx);
    caret_and_top(&view, 30, 20, cx);
    assert_eq!(pinned(&view, cx), [(IMPL, px(0.)), (FN, px(line_height))]);

    // The click comes while the text is being parsed (after an edit, say):
    // the caret goes to the header, and the view waits for the band its
    // scopes form.
    unparse(&view, cx);
    let position = on_row(&view, FN, cx);
    cx.update(|window, cx| {
        let down = MouseDownEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        };
        window.dispatch_event(down.to_platform_input(), cx);
        let up = MouseUpEvent {
            position,
            button: MouseButton::Left,
            modifiers: Modifiers::none(),
            click_count: 1,
        };
        window.dispatch_event(up.to_platform_input(), cx);
    });
    draw_before_parse(cx);
    assert!(view.read_with(cx, |view, _| view.syntax.as_ref().unwrap().is_parsing()));
    assert_eq!(caret(&view, cx), (first_non_blank(&view, FN, cx), true));
    let scroll = view.read_with(cx, |view, _| view.scroll.y);
    assert_eq!(scroll, px(20. * line_height));
    assert!(view.read_with(cx, |view, _| view.sticky_reveal.is_some()));
    // Parsed, it's under its parent's band.
    draw(cx);
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.sticky_reveal.is_none()));
    assert_eq!(pinned(&view, cx), [(IMPL, px(0.))]);
    let bottom = band(&view, cx).unwrap().bounds.bottom();
    assert_eq!(row_top(&view, FN, cx), Some(bottom));
}

#[gpui::test]
fn a_click_on_a_wide_header_scrolled_across_shows_the_caret(cx: &mut TestAppContext) {
    for block in [false, true] {
        // The header wide, and the rows in view wide enough to scroll across.
        let params: String = (0..40).map(|n| format!("y_{n}: i32, ")).collect();
        let wide = format!("; // {}\n", "-".repeat(400));
        let text = source()
            .replacen("fn new(x: i32)", &format!("fn new(x: i32, {params})"), 1)
            .replace("\";\n", &format!("\"{wide}"));
        let (view, _, cx) = editor_of(text, false, cx);
        let line_height = line_height(&view, cx);
        // A wide text block beside the header once it's revealed, which the
        // view scrolls across as far as reaches.
        if block {
            let lines = HostBlockKind::Lines {
                lines: vec![SharedString::from("x".repeat(400))],
                tint: Tint::Added,
                words: Vec::new(),
            };
            let block = HostBlock {
                above: FN + 2,
                rows: 1,
                kind: lines,
                id: 1,
            };
            view.update(cx, |view, cx| view.set_blocks(vec![block], cx));
        }
        caret_and_top(&view, 30, 20, cx);
        view.update(cx, |view, cx| {
            view.scroll.x = px(600.);
            cx.notify();
        });
        draw(cx);
        assert_eq!(view.read_with(cx, |view, _| view.scroll.x), px(600.));
        assert_eq!(pinned(&view, cx), [(IMPL, px(0.)), (FN, px(line_height))]);

        click(&view, FN, Modifiers::none(), cx);
        draw(cx);
        // Back across to the header's first non-blank, which is painted, and
        // the header still directly under the impl's band.
        assert_eq!(caret(&view, cx), (first_non_blank(&view, FN, cx), true));
        assert_eq!(
            view.read_with(cx, |view, _| view.scroll.x),
            px(0.),
            "{block}"
        );
        let carets = view.read_with(cx, |view, _| view.painted_carets.clone());
        assert_eq!(carets.len(), 1);
        let bottom = band(&view, cx).unwrap().bounds.bottom();
        assert_eq!(row_top(&view, FN, cx), Some(bottom));
        assert_eq!(carets[0].top(), bottom);
    }
}
