//! The `keystroke` bench's harness, with no text system or renderer, on the
//! small corpus: a keystroke gives a sample of every stage, `reset` undoes
//! it, and a keystroke that changes nothing gives no frame instead of a
//! hang. Nothing here is timed against a threshold.

#[allow(dead_code)]
#[path = "../benches/keystroke/harness.rs"]
mod harness;

use std::{sync::Arc, time::Duration};

use fe_buffer::ResolvedSelection;
use gpui::{Bounds, EntityInputHandler as _, NoopTextSystem, Pixels};
use harness::{Harness, NoFrame, cases};

fn small() -> Harness {
    let case = cases()
        .into_iter()
        .find(|case| case.name == "type/small")
        .expect("the small case exists");
    Harness::new(Arc::new(NoopTextSystem), || Ok(None), &case)
}

/// The text, the selections, and where the last frame put the primary caret
/// in the window, which moves with the scroll position.
fn state(harness: &mut Harness) -> (String, Vec<ResolvedSelection>, Option<Bounds<Pixels>>) {
    let view = harness.view.clone();
    harness
        .cx
        .update_window(harness.window.into(), |_, window, cx| {
            view.update(cx, |view, cx| {
                let selections = view.model().selections();
                let caret = selections[0].range.start;
                // The corpus is ASCII, so UTF-16 offsets are byte offsets.
                let at = view.bounds_for_range(caret..caret, Bounds::default(), window, cx);
                (view.model().text().to_string(), selections, at)
            })
        })
        .expect("the window is open")
}

#[test]
fn typing_records_every_stage() {
    let mut harness = small();
    let before = state(&mut harness);
    assert!(before.2.is_some_and(|at| at.origin.y > Pixels::ZERO));

    let sample = harness.keystroke("x").expect("typing draws a frame");

    let instants = sample.instants();
    assert!(
        instants.windows(2).all(|pair| pair[0] <= pair[1]),
        "{sample:#?}"
    );
    assert_eq!(sample.stages().iter().sum::<Duration>(), sample.total());
    assert_eq!(
        sample.stages()[5],
        Duration::ZERO,
        "nothing to present with"
    );
    let caret = before.1[0].range.start;
    let mut typed = before.0.clone();
    typed.insert(caret, 'x');
    assert_eq!(state(&mut harness).0, typed);

    harness.reset();
    assert_eq!(state(&mut harness), before);
}

#[test]
fn movement_records_a_frame_without_an_edit() {
    let mut harness = small();
    let (text, selections, _) = state(&mut harness);

    harness.keystroke("right").expect("moving draws a frame");

    let caret = selections[0].range.start + 1;
    let moved = vec![ResolvedSelection {
        range: caret..caret,
        reversed: false,
    }];
    assert_eq!(state(&mut harness).0, text);
    assert_eq!(state(&mut harness).1, moved);
}

#[test]
fn a_keystroke_with_no_effect_gives_no_frame() {
    let mut harness = small();
    let before = state(&mut harness);

    let result = harness.keystroke("ctrl-alt-shift-f19");

    assert_eq!(result.err(), Some(NoFrame::NotNotified));
    assert_eq!(state(&mut harness), before);
}
