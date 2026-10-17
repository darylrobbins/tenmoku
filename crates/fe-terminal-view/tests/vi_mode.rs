//! Vi copy mode in a terminal view: ctrl-shift-space, the keys that move,
//! select, yank and leave, none of which reach the pty, and the search
//! prompt drawn at the bottom of the view.

use std::thread;
use std::time::{Duration, Instant};

use fe_terminal_view::{TerminalOptions, TerminalQuery, TerminalStyle, TerminalView};
use gpui::{
    ClipboardItem, Entity, Focusable as _, Hsla, TestAppContext, VisualTestContext, px, rgb, size,
};

/// The platform's paste key.
const PASTE: &str = if cfg!(target_os = "macos") {
    "cmd-v"
} else {
    "ctrl-shift-v"
};

fn style() -> TerminalStyle {
    let background: Hsla = rgb(0x101010).into();
    TerminalStyle {
        font_family: "Menlo".into(),
        font_fallbacks: Vec::new(),
        font_size: px(14.),
        line_height: TerminalStyle::LINE_HEIGHT,
        background,
        foreground: rgb(0xd0d0d0).into(),
        cursor: rgb(0xffcc00).into(),
        selection: rgb(0x334466).into(),
        ansi: TerminalStyle::ansi_for(background),
    }
}

/// Raw, unechoed input, each read printed as a line of hex bytes.
const HEX: &str = "stty raw -echo opost; echo ready; \
     while :; do dd bs=64 count=1 2>/dev/null | od -An -tx1 -v; done";

/// Three marked prompts (OSC 133;A), each followed by 30 lines, then a
/// marked prompt waiting for input.
const PROMPTS: &str = r"for i in 1 2 3; do
        printf '\033]133;A\007$ prompt %d\n' $i
        j=1; while [ $j -le 30 ]; do echo out $i $j; j=$((j+1)); done
    done; printf '\033]133;A\007$ '; read x";

/// The view's height: its bottom, where the search prompt is drawn.
const HEIGHT: f32 = 400.;

/// Draws until `done` holds. Panics after 5 s, naming `what`.
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Draws for a while, so anything still on its way arrives.
fn settle(cx: &mut VisualTestContext) {
    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        draw(cx);
        thread::sleep(Duration::from_millis(10));
    }
}

fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.terminal().map(|t| t.snapshot().text()))
        .unwrap_or_default()
}

/// A focused view running `/bin/sh -c script`, once its screen shows
/// `ready`.
fn open<'a>(
    cx: &'a mut TestAppContext,
    script: &str,
    ready: &str,
) -> (Entity<TerminalView>, &'a mut VisualTestContext) {
    // The pty's threads wake the view's event task.
    cx.executor().allow_parking();
    let mut options = TerminalOptions::new(std::env::temp_dir());
    options.shell = Some((
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), script.to_owned()],
    ));
    let (view, cx) =
        cx.add_window_view(|window, cx| TerminalView::new(options, style(), window, cx).unwrap());
    cx.simulate_resize(size(px(600.), px(HEIGHT)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx)
    });
    until(cx, ready, |cx| screen(&view, cx).contains(ready));
    (view, cx)
}

fn vi_mode(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.is_vi_mode())
}

/// Every byte the hex program has read so far.
fn written(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Vec<u8> {
    view.update(cx, |view, cx| {
        // In vi mode the view stays put: read from the bottom.
        let was = view.is_vi_mode();
        if was {
            view.toggle_vi_mode(cx);
        }
        let text = view.terminal().unwrap().snapshot().text();
        if was {
            view.toggle_vi_mode(cx);
        }
        text
    })
    .split_whitespace()
    .filter(|word| word.len() == 2)
    .filter_map(|word| u8::from_str_radix(word, 16).ok())
    .collect()
}

/// The text of the vi cursor's row, its column, and how far back the view
/// is scrolled.
fn at(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> (String, usize, usize) {
    view.read_with(cx, |view, _| {
        let snapshot = view.terminal().unwrap().snapshot();
        assert!(snapshot.modes.vi, "not in vi mode");
        (
            snapshot.row_text(snapshot.cursor.row),
            snapshot.cursor.col,
            snapshot.display_offset,
        )
    })
}

/// The text the vi cursor is on and its column.
fn on(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> (String, usize) {
    let (text, col, _) = at(view, cx);
    (text, col)
}

fn here(text: &str, col: usize) -> (String, usize) {
    (text.to_owned(), col)
}

fn selected(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.terminal().unwrap().selected_text())
}

fn rows(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> usize {
    view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().rows.len())
}

#[gpui::test]
fn no_key_reaches_the_pty_in_vi_mode(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, HEX, "ready");
    cx.simulate_keystrokes("ctrl-shift-space");
    assert!(vi_mode(&view, cx));

    // Motions, counts, scrolls, unbound keys, the prompt and its keys,
    // selections cleared with escape, and typed text: the pty hears none.
    cx.simulate_keystrokes("j k h l 5 j w b e g g G $ 0 ctrl-d ctrl-u ctrl-e ctrl-y");
    cx.simulate_keystrokes("x z a s tab enter backspace ctrl-a ctrl-z alt-x f1 up down");
    cx.simulate_keystrokes("/ a b c enter ? x backspace backspace v escape n N");
    cx.simulate_input("héllo");
    view.update_in(cx, |view, window, cx| {
        use gpui::EntityInputHandler as _;
        view.replace_and_mark_text_in_range(None, "に", None, window, cx);
        view.replace_text_in_range(None, "に", window, cx);
    });
    assert!(vi_mode(&view, cx));
    settle(cx);
    assert_eq!(written(&view, cx), b"");

    // ctrl-shift-space leaves, and keys reach the pty again.
    cx.simulate_keystrokes("ctrl-shift-space");
    assert!(!vi_mode(&view, cx));
    cx.simulate_keystrokes("a");
    until(cx, "the program to read a", |cx| {
        !written(&view, cx).is_empty()
    });
    settle(cx);
    assert_eq!(written(&view, cx), b"a");
}

#[gpui::test]
fn keys_that_leave(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, HEX, "ready");
    for leave in ["q", "i", "ctrl-c", "escape", "ctrl-shift-space"] {
        cx.simulate_keystrokes("ctrl-shift-space");
        assert!(vi_mode(&view, cx), "{leave}");
        cx.simulate_keystrokes(leave);
        assert!(!vi_mode(&view, cx), "{leave} leaves");
    }

    // After the first key of gg, [[ or ]] too.
    for first in ["g", "[", "]"] {
        for leave in ["q", "i", "ctrl-c", "escape"] {
            cx.simulate_keystrokes("ctrl-shift-space");
            cx.simulate_keystrokes(first);
            cx.simulate_keystrokes(leave);
            assert!(!vi_mode(&view, cx), "{first} {leave} leaves");
        }
        cx.simulate_keystrokes("ctrl-shift-space k v");
        cx.simulate_keystrokes(first);
        cx.simulate_keystrokes("escape");
        assert!(vi_mode(&view, cx), "{first} escape");
        assert_eq!(selected(&view, cx), None, "{first} escape");
        cx.simulate_keystrokes("escape");
        assert!(!vi_mode(&view, cx));
    }

    // Escape clears a selection first, then leaves.
    cx.simulate_keystrokes("ctrl-shift-space k v");
    assert!(selected(&view, cx).is_some());
    cx.simulate_keystrokes("escape");
    assert!(vi_mode(&view, cx));
    assert_eq!(selected(&view, cx), None);
    cx.simulate_keystrokes("escape");
    assert!(!vi_mode(&view, cx));

    // y with nothing selected does nothing; with a selection it copies it
    // and leaves.
    cx.write_to_clipboard(ClipboardItem::new_string("before".into()));
    cx.simulate_keystrokes("ctrl-shift-space y");
    assert!(vi_mode(&view, cx));
    cx.simulate_keystrokes("k 0 v 4 l y");
    assert!(!vi_mode(&view, cx));
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("ready"));
    assert_eq!(selected(&view, cx), None);

    // The paste key leaves and pastes, also from the search prompts.
    cx.simulate_keystrokes("ctrl-shift-space");
    cx.simulate_keystrokes(PASTE);
    assert!(!vi_mode(&view, cx));
    until(cx, "the paste", |cx| written(&view, cx).len() >= 5);
    for prompt in ["/ a", "?"] {
        cx.simulate_keystrokes("ctrl-shift-space");
        cx.simulate_keystrokes(prompt);
        assert!(view.read_with(cx, |view, _| view.vi_prompt().is_some()));
        cx.simulate_keystrokes(PASTE);
        assert!(!vi_mode(&view, cx), "{prompt} paste leaves");
        assert_eq!(view.read_with(cx, |view, _| view.vi_prompt()), None);
    }
    until(cx, "the pastes", |cx| written(&view, cx).len() >= 15);
    settle(cx);
    // ctrl-c, i, q and the rest never reached it.
    assert_eq!(written(&view, cx), b"readyreadyready");
}

#[gpui::test]
fn keys_move_the_vi_cursor(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, PROMPTS, "out 3 30");
    let rows = rows(&view, cx);
    assert!(rows > 12, "{rows} rows");
    cx.simulate_keystrokes("ctrl-shift-space");
    assert_eq!(on(&view, cx), here("$", 2));

    // [[ and ]] to the prompt marks, with a count.
    cx.simulate_keystrokes("[ [");
    assert_eq!(on(&view, cx), here("$ prompt 3", 0));
    cx.simulate_keystrokes("[ [");
    assert_eq!(on(&view, cx), here("$ prompt 2", 0));
    cx.simulate_keystrokes("] ]");
    assert_eq!(on(&view, cx), here("$ prompt 3", 0));
    cx.simulate_keystrokes("2 [ [");
    assert_eq!(on(&view, cx), here("$ prompt 1", 0));
    let (_, _, top) = at(&view, cx);
    assert!(top > 2 * rows, "{top}");

    // Counts.
    cx.simulate_keystrokes("5 j");
    assert_eq!(on(&view, cx), here("out 1 5", 0));
    cx.simulate_keystrokes("2 w");
    assert_eq!(on(&view, cx), here("out 1 5", 6));
    cx.simulate_keystrokes("3 w");
    assert_eq!(on(&view, cx), here("out 1 6", 6));
    cx.simulate_keystrokes("1 0 k 0");
    assert_eq!(on(&view, cx), here("$ prompt 1", 0));
    cx.simulate_keystrokes("1 2 j");
    assert_eq!(on(&view, cx), here("out 1 12", 0));

    // ctrl-d and ctrl-u move the cursor with the view on screen; ctrl-f and
    // ctrl-b page two rows less than the view and put it on the new top or
    // bottom row; ctrl-e and ctrl-y leave it on its line.
    let row = view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().cursor.row);
    let page = rows - 2;
    let (_, _, offset) = at(&view, cx);
    // The fixture's line `n` rows below `$ prompt 1`.
    let row_text = |n: usize| match n {
        1..=30 => format!("out 1 {n}"),
        31 => "$ prompt 2".to_owned(),
        _ => format!("out 2 {}", n - 31),
    };
    let half = rows / 2;
    let steps = [
        ("ctrl-e", 0, offset - 1),
        ("ctrl-y", 0, offset),
        ("ctrl-d", half, offset - half),
        ("ctrl-u", 0, offset),
        ("ctrl-f", page - row, offset - page),
        ("ctrl-b", rows - 1 - row, offset),
        ("2 ctrl-e", rows - 1 - row, offset - 2),
    ];
    for (keys, down, want) in steps {
        cx.simulate_keystrokes(keys);
        let (text, col, offset) = at(&view, cx);
        assert_eq!(
            (text, col, offset),
            (row_text(12 + down), 0, want),
            "{keys}"
        );
    }
}

#[gpui::test]
fn the_search_prompt(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, PROMPTS, "out 3 30");
    view.update(cx, |view, _| view.record_paint(true));
    cx.simulate_keystrokes("ctrl-shift-space");
    let prompt = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.vi_prompt());
    let find = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.find_query().map(|query| query.text.clone())
        })
    };

    // / opens a prompt at the bottom of the view; keys type into it.
    cx.simulate_keystrokes("/");
    draw(cx);
    assert_eq!(prompt(cx).as_deref(), Some("/"));
    cx.simulate_keystrokes("o u t space 2 space 1 [ 0 - 9 ]");
    draw(cx);
    assert_eq!(prompt(cx).as_deref(), Some("/out 2 1[0-9]"));
    let bounds = view.read_with(cx, |view, _| view.vi_prompt_bounds());
    let bounds = bounds.expect("the prompt is drawn");
    assert_eq!(bounds.bottom(), px(HEIGHT));
    assert_eq!(bounds.left(), px(0.));
    assert!(bounds.size.height > px(0.) && bounds.size.height < px(40.));

    // Enter: the next match, wrapping from the bottom to the top of
    // history, and every match on screen highlighted.
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(prompt(cx), None);
    assert_eq!(view.read_with(cx, |view, _| view.vi_prompt_bounds()), None);
    assert_eq!(on(&view, cx), here("out 2 10", 0));
    assert_eq!(find(cx).as_deref(), Some("out 2 1[0-9]"));
    let matches = view.read_with(cx, |view, _| view.find_matches().len());
    assert_eq!(matches, 10);

    // n on, N back, with counts.
    cx.simulate_keystrokes("n");
    assert_eq!(on(&view, cx), here("out 2 11", 0));
    cx.simulate_keystrokes("3 n");
    assert_eq!(on(&view, cx), here("out 2 14", 0));
    cx.simulate_keystrokes("N");
    assert_eq!(on(&view, cx), here("out 2 13", 0));

    // ? searches back, so n goes back and N forward.
    cx.simulate_keystrokes("? p r o m p t enter");
    assert_eq!(on(&view, cx), here("$ prompt 2", 2));
    cx.simulate_keystrokes("n");
    assert_eq!(on(&view, cx), here("$ prompt 1", 2));
    cx.simulate_keystrokes("N");
    assert_eq!(on(&view, cx), here("$ prompt 2", 2));

    // Escape, and backspace on an empty query, cancel without moving.
    cx.simulate_keystrokes("/ o u t escape");
    assert_eq!(prompt(cx), None);
    cx.simulate_keystrokes("/ o backspace backspace");
    assert_eq!(prompt(cx), None);
    assert_eq!(on(&view, cx), here("$ prompt 2", 2));
    assert_eq!(find(cx).as_deref(), Some("prompt"));
    // A query that doesn't compile, or has no match, leaves it too.
    cx.simulate_keystrokes("/ ( enter / n o w h e r e enter");
    assert_eq!(on(&view, cx), here("$ prompt 2", 2));

    // A query the find row set: n goes forward through it, N back.
    view.update(cx, |view, cx| {
        view.set_find(Some(TerminalQuery::literal("out 3 1")), cx)
            .unwrap()
    });
    cx.simulate_keystrokes("n");
    assert_eq!(on(&view, cx), here("out 3 1", 0));
    cx.simulate_keystrokes("n");
    assert_eq!(on(&view, cx), here("out 3 10", 0));
    cx.simulate_keystrokes("N");
    assert_eq!(on(&view, cx), here("out 3 1", 0));

    // A ? search's query, replaced or cleared and then set again from the
    // find row, goes forward too.
    for between in [None, Some(TerminalQuery::literal("out"))] {
        cx.simulate_keystrokes("? p r o m p t enter");
        assert_eq!(on(&view, cx), here("$ prompt 3", 2));
        view.update(cx, |view, cx| {
            view.set_find(between.clone(), cx).unwrap();
            view.set_find(Some(TerminalQuery::regex("prompt")), cx)
                .unwrap();
        });
        // Forward from the last prompt wraps to the first.
        cx.simulate_keystrokes("n");
        assert_eq!(on(&view, cx), here("$ prompt 1", 2), "{between:?}");
        cx.simulate_keystrokes("2 n");
        assert_eq!(on(&view, cx), here("$ prompt 3", 2), "{between:?}");
        cx.simulate_keystrokes("j");
    }
}

#[gpui::test]
fn vi_mode_takes_no_text_input(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler as _;
    let (view, cx) = open(cx, HEX, "ready");
    let accepts = |cx: &mut VisualTestContext| {
        view.update_in(cx, |view, window, cx| view.accepts_text_input(window, cx))
    };
    assert!(accepts(cx));

    // In vi mode the platform hears there's no text input, so an input
    // method leaves printable keys to the view, which takes them as
    // commands.
    cx.simulate_keystrokes("ctrl-shift-space");
    draw(cx);
    assert!(!accepts(cx));
    let row = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().cursor.row)
    };
    let before = row(cx);
    for key in ["k", "/", "r", "enter"] {
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap().with_simulated_ime(),
            is_held: false,
            prefer_character_input: true,
        });
    }
    draw(cx);
    assert_eq!(row(cx), before - 1);
    assert_eq!(on(&view, cx), here("ready", 0));
    let query = view.read_with(cx, |view, _| view.find_query().map(|q| q.text.clone()));
    assert_eq!(query.as_deref(), Some("r"));

    // Text input is back once it leaves.
    cx.simulate_keystrokes("q");
    draw(cx);
    assert!(accepts(cx));
    settle(cx);
    assert_eq!(written(&view, cx), b"");
}

#[gpui::test]
fn alt_keys_do_nothing_in_vi_mode(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, HEX, "ready");
    cx.simulate_keystrokes("ctrl-shift-space k v l");
    let before = at(&view, cx);
    assert_eq!(selected(&view, cx).as_deref(), Some("re"));

    // Neither the exit keys, nor y, nor the motions, with alt.
    cx.simulate_keystrokes("alt-q alt-i alt-y alt-j alt-k alt-l alt-left alt-right alt-escape");
    cx.simulate_keystrokes("alt-shift-g alt-0 alt-$ alt-v alt-/");
    draw(cx);
    assert!(vi_mode(&view, cx));
    assert_eq!(at(&view, cx), before);
    assert_eq!(selected(&view, cx).as_deref(), Some("re"));
    assert_eq!(view.read_with(cx, |view, _| view.vi_prompt()), None);
    settle(cx);
    assert_eq!(written(&view, cx), b"");
}
