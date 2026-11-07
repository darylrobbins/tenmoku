//! The Start page keeps its keyboard selection in view and lists the recent
//! list as it is now, not as it was when the page came up.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::recent::{self, Entry};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_workbench::{Launch, Workbench};
use gpui::{
    Bounds, Entity, Focusable as _, Keystroke, Pixels, TestAppContext, VisualTestContext, px, size,
};

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other test of this binary running, the
/// environment cleared of what names a config directory, and the state
/// directory `state`, where the recent list is kept.
fn serially(state: &Path) -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in [
        "TENMOKU_CONFIG_DIR",
        "XDG_CONFIG_HOME",
        "HOME",
        "APPDATA",
        "USERPROFILE",
    ] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    // SAFETY: as above.
    unsafe { std::env::set_var("TENMOKU_STATE_DIR", state) };
    guard
}

/// `count` files under `dir`, recorded as recent, the first the oldest.
fn seed(dir: &Path, count: usize) -> Vec<PathBuf> {
    let list = recent::recent_path().expect("the state directory is set");
    let files: Vec<PathBuf> = (0..count)
        .map(|ix| dir.join(format!("recent-file-{ix:02}.txt")))
        .collect();
    for file in &files {
        fs::write(file, "text\n").unwrap();
        recent::record(&list, Entry::File(file)).unwrap();
    }
    files
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn launch(cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: None,
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        window.activate_window();
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut vcx);
    (workbench, vcx)
}

fn press(key: &str, times: usize, cx: &mut VisualTestContext) {
    for _ in 0..times {
        cx.update(|window, cx| window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx));
        draw(cx);
    }
}

fn row(file: &Path, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    let name = file.file_name().unwrap().to_string_lossy();
    let selector: &'static str = Box::leak(format!("start-page-row:{name}").into_boxed_str());
    cx.debug_bounds(selector)
}

fn inside(row: Bounds<Pixels>, page: Bounds<Pixels>) -> bool {
    row.top() >= page.top() && row.bottom() <= page.bottom()
}

/// The last row of the page with vim off; 25 Downs clamp to it.
const LAST: &str = "start-page-row:Keyboard Shortcuts";

/// A window short enough that the page overflows it: the last row is drawn
/// below the page's bounds.
fn overflowing(cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.simulate_resize(size(px(800.), px(240.)));
    draw(cx);
    let page = cx.debug_bounds("start-page").expect("the page shows");
    let last = cx.debug_bounds(LAST).expect("the last row is drawn");
    assert!(!inside(last, page), "the list overflows the page");
    page
}

#[gpui::test]
fn down_scrolls_the_selected_row_into_view(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let _guard = serially(&dir.path().join("state"));
    seed(dir.path(), 30);
    let (_workbench, mut cx) = launch(cx);
    let page = overflowing(&mut cx);
    // The page lists five recents; 25 Downs select its last row.
    press("down", 25, &mut cx);
    assert_eq!(cx.debug_bounds("start-page"), Some(page));
    let last = cx.debug_bounds(LAST).expect("the selected row is drawn");
    assert!(inside(last, page), "the selected row is inside the page");
    let first = cx.debug_bounds("start-page-row:New File").unwrap();
    assert!(
        !inside(first, page),
        "the page scrolled away from its first row"
    );
}

#[gpui::test]
fn up_scrolls_back_to_the_first_row(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let _guard = serially(&dir.path().join("state"));
    seed(dir.path(), 30);
    let (_workbench, mut cx) = launch(cx);
    let page = overflowing(&mut cx);
    press("down", 25, &mut cx);
    let last = cx.debug_bounds(LAST).unwrap();
    assert!(inside(last, page), "the selection scrolled down first");
    press("up", 25, &mut cx);
    let first = cx
        .debug_bounds("start-page-row:New File")
        .expect("the first row is drawn");
    assert!(inside(first, page), "the first row is back inside the page");
}

#[gpui::test]
fn clearing_recents_refreshes_a_shown_page(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let _guard = serially(&dir.path().join("state"));
    let files = seed(dir.path(), 3);
    let (_workbench, mut cx) = launch(cx);
    for file in &files {
        assert!(row(file, &mut cx).is_some(), "{} is listed", file.display());
    }
    let run = RunCommand(
        Invocation::of("workbench.clear_recent")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(&mut cx);
    assert!(
        cx.debug_bounds("start-page").is_some(),
        "the page still shows"
    );
    for file in &files {
        assert!(row(file, &mut cx).is_none(), "{} is gone", file.display());
    }
}
