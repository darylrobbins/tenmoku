//! The rail tests read the stroke the painter drew: the painter hands each
//! stroke's endpoints to the hooks below (and the probe records the same
//! ones), so shortening the stroke breaks the rail. Written against what
//! the base has: a base painter never calls the hooks.

use std::{cell::RefCell, collections::BTreeMap};

use gpui::{TestAppContext, px};

use super::{draw, ends, expanded, fixture};

thread_local! {
    static PULL_IN: RefCell<f32> = const { RefCell::new(0.) };
    static PAINTED: RefCell<BTreeMap<usize, Vec<[f32; 4]>>> = const { RefCell::new(BTreeMap::new()) };
}

#[unsafe(no_mangle)]
fn fe_rail_pull_in() -> f32 {
    PULL_IN.with_borrow(|pull| *pull)
}

#[unsafe(no_mangle)]
fn fe_rail_begin(ix: usize) {
    PAINTED.with_borrow_mut(|painted| painted.remove(&ix));
}

#[unsafe(no_mangle)]
fn fe_rail_painted(ix: usize, fx: f32, fy: f32, tx: f32, ty: f32) {
    PAINTED.with_borrow_mut(|painted| painted.entry(ix).or_default().push([fx, fy, tx, ty]));
}

/// Whether each of the commit's rows leaves what the next one enters.
fn continuous(seen: &crate::workbench::git_commands::git_log::RowsSeen) -> bool {
    (0..3).all(|ix| ends(seen, ix, true) == ends(seen, ix + 1, false))
}

#[gpui::test]
fn a_shortened_stroke_breaks_the_rail(cx: &mut TestAppContext) {
    let dir = fixture("t.rs");
    let (panel, mut cx) = expanded(cx, &dir);
    let seen = panel.read_with(&cx, |panel, _| panel.rows_seen());
    assert!(continuous(&seen.borrow()), "the real endpoints meet");
    PULL_IN.with_borrow_mut(|pull| *pull = px(1.).into());
    draw(&mut cx);
    let broken = !continuous(&seen.borrow());
    PULL_IN.with_borrow_mut(|pull| *pull = 0.);
    assert!(broken, "a stroke one pixel short breaks the rail");
}

#[gpui::test]
fn the_probe_and_the_painter_share_one_segment(cx: &mut TestAppContext) {
    let dir = fixture("t.rs");
    let (panel, cx) = expanded(cx, &dir);
    let seen = panel.read_with(&cx, |panel, _| panel.rows_seen());
    let seen = seen.borrow();
    PAINTED.with_borrow(|painted| {
        assert!(!painted.is_empty(), "the painter reported its strokes");
        for (ix, strokes) in painted {
            let probed: Vec<_> = seen.segments[ix]
                .iter()
                .map(|(from, to, _)| [from.x, from.y, to.x, to.y].map(f32::from))
                .collect();
            assert_eq!(&probed, strokes, "row {ix}");
        }
    });
}
