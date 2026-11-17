//! Find in a [`HexViewer`]: hex and text queries typed in the header's
//! input, the next and previous match, wrapping, and the background search
//! stopping on a new query, on closing and on drop.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use fe_fileview::FindOutcome;
use futures::channel::oneshot;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::tests::{draw, file_of, run, secondary, selected, viewer};
use super::*;

/// Opens Find with its key and types `text` over what it held.
fn find(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_keystrokes(&format!("{}-f", secondary()));
    draw(cx);
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn said(
    viewer: &Entity<HexViewer>,
    cx: &mut VisualTestContext,
) -> (Option<String>, Option<FindSaid>) {
    viewer.read_with(cx, |v, _| {
        (v.find_error().map(|e| e.to_string()), v.find_said())
    })
}

/// `len` dots with `marks` written at their offsets.
fn dots_with(len: usize, marks: &[(usize, &[u8])]) -> Vec<u8> {
    let mut bytes = vec![b'.'; len];
    for &(at, mark) in marks {
        bytes[at..at + mark.len()].copy_from_slice(mark);
    }
    bytes
}

#[gpui::test]
fn hex_query_finds_bytes_with_nul_and_newline(cx: &mut TestAppContext) {
    let needle: &[u8] = b"\xDE\x00\x0A\x0D\xEF";
    let bytes = dots_with(4096, &[(300, needle), (3000, needle)]);
    let (_dir, path) = file_of(&bytes);
    let (viewer, cx, events) = viewer(cx, &path);

    find(cx, "de 00 0A 0d EF");
    assert_eq!(viewer.read_with(cx, |v, _| v.opened()), Some(Opened::Find));
    assert_eq!(selected(&viewer, cx), 300..305);
    assert_eq!(said(&viewer, cx), (None, None));
    // The next from the match, by the find bar's Enter and the view's key.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 3000..3005);
    cx.simulate_keystrokes("shift-enter");
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 300..305);
    assert_eq!(
        *events.borrow(),
        [
            HexViewerEvent::Found { wrapped: false },
            HexViewerEvent::Found { wrapped: false },
            HexViewerEvent::Found { wrapped: false },
        ]
    );

    // Without spaces, the same bytes.
    cx.simulate_keystrokes(&format!("{}-down", secondary()));
    find(cx, "de000a0def");
    assert_eq!(selected(&viewer, cx), 300..305);

    // The match found is outlined, until the query changes.
    draw(cx);
    let outlined =
        |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.painted_match.clone());
    assert_eq!(outlined(cx), Some(300..305));

    // An odd digit count, or a char that isn't hex, says why, and searches
    // nothing.
    find(cx, "de0");
    draw(cx);
    assert_eq!(
        outlined(cx),
        None,
        "no current match for a query that doesn't parse"
    );
    assert_eq!(selected(&viewer, cx), 300..305);
    let error = said(&viewer, cx).0;
    assert_eq!(error.as_deref(), Some("An odd number of hex digits"));
    find(cx, "de zz");
    let error = said(&viewer, cx).0;
    assert_eq!(error.as_deref(), Some("Not a hex digit: z"));
    assert!(cx.debug_bounds(verbatim!("hex-viewer-said")).is_some());
    assert_eq!(selected(&viewer, cx), 300..305);
    assert!(!viewer.read_with(cx, |v, _| v.is_finding()));
}

#[gpui::test]
fn text_query_matches_case_by_toggle(cx: &mut TestAppContext) {
    let bytes = dots_with(256, &[(10, b"Hello"), (40, b"hello"), (90, b"HELLO")]);
    let (_dir, path) = file_of(&bytes);
    let (viewer, cx, _) = viewer(cx, &path);
    find(cx, "");
    viewer.update(cx, |v, cx| v.set_find_mode(FindMode::Text, cx));

    cx.simulate_input("hello");
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 10..15);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 40..45);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 90..95);

    // Match Case on: only the lowercase one, found again by wrapping.
    viewer.update(cx, |v, cx| v.toggle_match_case(cx));
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 40..45);
    assert_eq!(said(&viewer, cx).1, Some(FindSaid::Wrapped));
    run(cx, FIND_NEXT);
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 40..45);
    assert_eq!(said(&viewer, cx).1, Some(FindSaid::Wrapped));

    // In hex mode the same text reads as hex: `hello` isn't.
    viewer.update(cx, |v, cx| v.set_find_mode(FindMode::Hex, cx));
    assert_eq!(said(&viewer, cx).0.as_deref(), Some("Not a hex digit: h"));
}

#[gpui::test]
fn find_wraps_and_reports_not_found(cx: &mut TestAppContext) {
    let bytes = dots_with(2048, &[(16, b"\xCA\xFE"), (1000, b"\xCA\xFE")]);
    let (_dir, path) = file_of(&bytes);
    let (viewer, cx, events) = viewer(cx, &path);
    find(cx, "CAFE");
    assert_eq!(selected(&viewer, cx), 16..18);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(viewer.read_with(cx, |v, _| v.opened()), None);

    // With the input closed, the view's keys still step through the query.
    let next = if cfg!(target_os = "macos") {
        "cmd-g"
    } else {
        "f3"
    };
    let previous = if cfg!(target_os = "macos") {
        "cmd-shift-g"
    } else {
        "shift-f3"
    };
    cx.simulate_keystrokes(next);
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 1000..1002);
    cx.simulate_keystrokes(next);
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 16..18);
    assert_eq!(said(&viewer, cx).1, Some(FindSaid::Wrapped));
    // Said in the header, with the input closed.
    draw(cx);
    assert!(cx.debug_bounds(verbatim!("hex-viewer-said")).is_some());
    cx.simulate_keystrokes(previous);
    cx.run_until_parked();
    assert_eq!(selected(&viewer, cx), 1000..1002);
    assert_eq!(
        events.borrow()[1..],
        [
            HexViewerEvent::Found { wrapped: false },
            HexViewerEvent::Found { wrapped: true },
            HexViewerEvent::Found { wrapped: true },
        ]
    );

    // Not there: the selection stays, unmarked, and the header says so.
    events.borrow_mut().clear();
    find(cx, "CA FE BA BE");
    assert_eq!(selected(&viewer, cx), 1000..1002);
    draw(cx);
    assert_eq!(viewer.read_with(cx, |v, _| v.painted_match.clone()), None);
    assert_eq!(said(&viewer, cx), (None, Some(FindSaid::NotFound)));
    assert_eq!(*events.borrow(), [HexViewerEvent::NotFound]);
    draw(cx);
    assert!(cx.debug_bounds(verbatim!("hex-viewer-said")).is_some());
}

#[gpui::test]
fn a_new_query_stops_the_running_search(cx: &mut TestAppContext) {
    let bytes = dots_with(8192, &[(5000, b"\xAA\xBB\xCC")]);
    let (_dir, path) = file_of(&bytes);
    let (viewer, cx, events) = viewer(cx, &path);
    let hold = viewer.read_with(cx, |v, _| v.find.hold.clone());
    let flag = |cx: &mut VisualTestContext| {
        viewer.read_with(cx, |v, _| v.stop_flag().expect("a search runs"))
    };
    let stopped = |flag: &Arc<AtomicBool>| flag.load(Ordering::Relaxed);
    let ended = || hold.ended.lock().unwrap().clone();
    let hold_next = || {
        let (release, held) = oneshot::channel();
        hold.releases.lock().unwrap().push_back(held);
        release
    };
    let until_begun = |cx: &mut VisualTestContext, n: usize| {
        cx.run_until_parked();
        assert_eq!(hold.begun.load(Ordering::SeqCst), n);
    };

    // A new query, typed on: the first scan stops, the second finds.
    let release = hold_next();
    find(cx, "AA");
    let first = flag(cx);
    until_begun(cx, 1);
    assert!(!stopped(&first) && ended().is_empty());
    cx.simulate_input("BB");
    assert!(stopped(&first));
    until_begun(cx, 2);
    assert_eq!(selected(&viewer, cx), 5000..5002);
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(
        ended(),
        [
            Some(FindOutcome::Found {
                range: 5000..5002,
                wrapped: false
            }),
            Some(FindOutcome::Stopped),
        ]
    );
    assert_eq!(*events.borrow(), [HexViewerEvent::Found { wrapped: false }]);

    // Closing the input stops one.
    let release = hold_next();
    cx.simulate_input("CC");
    let second = flag(cx);
    until_begun(cx, 3);
    cx.simulate_keystrokes("escape");
    assert!(stopped(&second));
    assert!(!viewer.read_with(cx, |v, _| v.is_finding()));
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(ended().last(), Some(&Some(FindOutcome::Stopped)));

    // Switching to Go to Offset stops one, and the selection stays.
    let release = hold_next();
    cx.simulate_keystrokes(&format!("{}-f", secondary()));
    draw(cx);
    cx.simulate_input("AA");
    let switched = flag(cx);
    until_begun(cx, 4);
    let before = selected(&viewer, cx);
    cx.simulate_keystrokes("ctrl-g");
    assert!(stopped(&switched));
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(ended().last(), Some(&Some(FindOutcome::Stopped)));
    assert_eq!(selected(&viewer, cx), before);
    cx.simulate_keystrokes("escape");

    // And so does dropping the view.
    let release = hold_next();
    run(cx, FIND_PREVIOUS);
    let third = flag(cx);
    until_begun(cx, 5);
    cx.update(|window, cx| {
        let host = window
            .root::<super::tests::Host>()
            .flatten()
            .expect("the host");
        host.update(cx, |host, cx| {
            let theme = EditorTheme::default_dark(cx);
            let other = Arc::new(FileView::open(&path).unwrap());
            host.viewer = cx.new(|cx| HexViewer::new(other, theme, cx));
            cx.notify();
        });
    });
    drop(viewer);
    // The last frame's elements held it too.
    draw(cx);
    cx.run_until_parked();
    assert!(stopped(&third));
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(ended().last(), Some(&Some(FindOutcome::Stopped)));
}
