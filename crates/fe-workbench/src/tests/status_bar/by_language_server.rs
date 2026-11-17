//! The nav source item ("by <provider>") names the answer just given; it
//! must not stay on the bar afterwards, bare of any context.

use std::{fs, time::Duration};

use gpui::TestAppContext;

use crate::{StatusSide, nav::SOURCE_ITEM};

use super::super::open_root;

#[gpui::test]
fn the_status_bar_shows_no_bare_by_language_server_label(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    // A language server answered a go-to-definition earlier.
    cx.update(|_, cx| crate::nav::show_source(&bar, "language server", cx));
    cx.run_until_parked();
    let label = |cx: &mut gpui::VisualTestContext| {
        bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left))
            .into_iter()
            .any(|(id, text)| id == SOURCE_ITEM || text.contains("by language server"))
    };
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // Later, with nothing asked since, it is gone.
    cx.executor().advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !label(cx),
        "{:?}",
        bar.read_with(cx, |b, _| b.drawn(StatusSide::Left))
    );
}
