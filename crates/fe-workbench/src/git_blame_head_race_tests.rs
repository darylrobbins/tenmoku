//! A blame of the old HEAD landing after HEAD changed.
//!
//! Probing this ordering exposed no defect in `git_blame.rs`: the HEAD check
//! requested while the blame is pending is deferred (`recheck`), and the old
//! blame's landing runs it. The test pins that behaviour; it needs the
//! blame-hold hook added to `git_blame.rs` for tests, so it does not compile
//! at the base.

use gpui::TestAppContext;

use super::*;

#[gpui::test]
fn old_head_blame_landing_after_a_head_change_is_revalidated(cx: &mut TestAppContext) {
    let (dir, _) = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, Some("a.rs"));
    let cx = &mut cx;
    let tab = tab_editor(&workbench, cx);
    keys("2 j", cx);

    // The blame of the old HEAD is read, and held before it lands.
    let (release, hold) = futures::channel::oneshot::channel();
    cx.update(|_, cx| super::super::state(cx).hold = Some(hold));
    dispatch("git.toggle_line_blame", cx);
    for _ in 0..200 {
        cx.executor().tick();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(counted(cx).blames, 0, "held");

    // HEAD changes and is checked, which waits for the blame to land.
    amend(root);
    check_head_now(&workbench, cx);
    release.send(()).unwrap();
    settle(cx);
    assert_eq!(counted(cx).blames, 2, "the old HEAD's blame, then the new");
    assert_eq!(expected(root, 2), "Shout louder, Bob (3 days ago)");
    assert_eq!(annotations(&tab, cx), [(2, expected(root, 2))]);
}
