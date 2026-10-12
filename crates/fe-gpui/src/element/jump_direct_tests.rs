//! A direct target, the match typing its `{c2}` jumps straight to, paints as
//! a border around its char, not a label.

use gpui::TestAppContext;

use super::{jump_style_tests::*, *};

#[gpui::test]
fn direct_target_marked(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "ab ab ab ac", 1, 600., 10.);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    cx.simulate_keystrokes("g s a");
    draw(&view, cx);
    let laid = laid_out(&view, cx);
    let direct: Vec<_> = (laid.iter())
        .filter(|(_, state)| *state == JumpLabelState::Direct)
        .map(|(bounds, _)| *bounds)
        .collect();
    // `ab` autojumps to (0, 3), `ac` to (0, 9).
    assert_eq!(direct.len(), 2);
    let paints = paints();
    let marks: Vec<_> = (paints.iter())
        .filter_map(|paint| match *paint {
            JumpPaint::Direct {
                bounds,
                color,
                width,
            } => Some((bounds, color, width)),
            _ => None,
        })
        .collect();
    let want: Vec<_> = direct
        .iter()
        .map(|&bounds| (bounds, theme.jump_label, px(2.)))
        .collect();
    assert_eq!(marks, want);
    // No label box at a direct target's offset: the one preview label is
    // the other.
    let boxes = label_paints(&paints);
    assert_eq!(boxes.len(), 1);
    assert!(direct.iter().all(|mark| boxes[0].0 != *mark));
}
