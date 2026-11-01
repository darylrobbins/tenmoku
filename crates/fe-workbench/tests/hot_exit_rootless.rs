//! Hot exit for a window with no root folder, as a launch with no argument
//! opens: quitting asks nothing about its untitled buffer, and the next
//! such launch restores it, dirty, at its caret; after a crash too, from
//! the backups kept for rootless windows, which a folder launch leaves
//! alone.

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use fe_buffer::{BackupId, BackupMeta, BackupStore, Buffer};
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_text::Text;
use fe_workbench::{Launch, WELCOME, Workbench, window_layouts};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::layout::{self, LayoutStore, WindowLayout};

/// Past the wait before a dirty buffer is backed up.
const BACKED_UP: Duration = Duration::from_secs(2);

static SERIAL: Mutex<()> = Mutex::new(());

/// Runs the rest of a test with no other test of this binary running.
///
/// Outside the crate's own tests nothing points the workbench at a config
/// directory of its own: with none, it neither reads the user's settings
/// nor watches them on a thread the test scheduler would reject. So the
/// guard also clears what would name one, with no other test's thread
/// running to read the environment meanwhile.
fn serially() -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for var in ["TENMOKU_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME"] {
        // SAFETY: every test takes the lock before it starts gpui, so no
        // other thread is running to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    guard
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
}

/// A launch's first window, as fastedit's opens it: on `root`, or with
/// none as a launch with no argument, saving its layout in the state
/// directory `state` and restoring its saved entry, drawn and focused.
fn launch(
    state: &Path,
    root: Option<&Path>,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, VisualTestContext) {
    let (layout, entry) = layout::launch(&layouts(state, root), true).remove(0);
    open(root, layout, entry, cx)
}

fn layouts(state: &Path, root: Option<&Path>) -> LayoutStore {
    window_layouts(state.join("layouts"), root)
}

fn open(
    root: Option<&Path>,
    layout: WindowLayout,
    entry: Option<Value>,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, VisualTestContext) {
    let launch = Launch {
        root: root.map(Path::to_owned),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let mut workbench = Workbench::new(launch, theme, EditorSettings::default(), window, cx);
        if let Some(entry) = &entry {
            workbench.restore_layout(entry, window, cx);
        }
        workbench.set_layout(layout, window, cx);
        workbench
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(&mut vcx);
    (workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// A new untitled buffer, typed into, in the window.
fn type_untitled(text: &str, cx: &mut VisualTestContext) {
    run("workbench.new_file", cx);
    cx.simulate_input(text);
    draw(cx);
}

/// Every pane's editors, in order.
fn editors(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<EditorView>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .flat_map(|pane| {
                (pane.read(cx).tabs().iter())
                    .filter_map(|tab| tab.editor().cloned())
                    .collect::<Vec<_>>()
            })
            .collect()
    })
}

/// Each editor's path, text, whether it's dirty, and its caret.
fn states(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(Option<PathBuf>, String, bool, usize)> {
    let editors = editors(workbench, cx);
    editors
        .iter()
        .map(|editor| {
            editor.read_with(cx, |editor, _| {
                let model = editor.model();
                let text = model.buffer().text().to_string();
                let caret = model.primary_selection().head();
                (
                    model.path().map(Path::to_owned),
                    text,
                    model.is_dirty(),
                    caret,
                )
            })
        })
        .collect()
}

/// The backups in the state directory `state` of the windows on `root`,
/// or with none: beside their layouts, as hot exit keeps a window's.
fn backups(state: &Path, root: Option<&Path>) -> BackupStore {
    let layouts = layouts(state, root).file();
    BackupStore::new(state.join("backups").join(layouts.file_stem().unwrap()))
}

/// A copy of the state directory `state` as a crash leaves it: with no
/// lock in it held, as the OS drops a dead process's.
fn crash_image(state: &Path) -> tempfile::TempDir {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let to = to.join(entry.file_name());
            match entry.file_type().unwrap().is_dir() {
                true => copy(&entry.path(), &to),
                false => drop(std::fs::copy(entry.path(), to).unwrap()),
            }
        }
    }
    let image = tempfile::tempdir().unwrap();
    copy(state, image.path());
    image
}

/// Quitting a rootless window asks nothing about its untitled buffer, and
/// the next launch with no argument shows it dirty, at its caret.
#[gpui::test]
fn quit_restores(first: &mut TestAppContext, second: &mut TestAppContext) {
    let _serial = serially();
    let state = tempfile::tempdir().unwrap();
    init(first);
    let (workbench, mut cx) = launch(state.path(), None, first);
    // Only the window holds it, so it goes as the app quits.
    drop(workbench);
    type_untitled("hello", &mut cx);
    run("app.quit", &mut cx);
    assert_eq!(cx.pending_prompt(), None, "quitting asks nothing");
    // The platform's quit, as the app goes.
    first.quit();

    init(second);
    let (workbench, mut cx) = launch(state.path(), None, second);
    assert_eq!(
        states(&workbench, &mut cx),
        [(None, "hello".into(), true, 5)]
    );
}

/// A crash of a rootless window, its untitled buffer backed up but the app
/// never quit, restores it on the next launch with no argument, with a
/// backup no layout names as a tab of its own. A folder's windows keep
/// their layouts and backups elsewhere, so a folder launch finds neither.
#[gpui::test]
fn crash_restores(first: &mut TestAppContext, second: &mut TestAppContext) {
    let _serial = serially();
    let state = tempfile::tempdir().unwrap();
    init(first);
    let (_workbench, mut cx) = launch(state.path(), None, first);
    type_untitled("hello", &mut cx);
    cx.executor().advance_clock(BACKED_UP);
    draw(&mut cx);
    let orphan: BackupId = "1000-1-0".parse().unwrap();
    let meta = BackupMeta::for_buffer(&Buffer::new(""), None, "untitled");
    let store = backups(state.path(), None);
    (store.write(orphan, &meta, &Text::new("orphan"))).unwrap();
    assert_eq!(store.ids().unwrap().len(), 2);

    // The first app crashes: it neither quits nor closes its window.
    let image = crash_image(state.path());
    let root = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(root.path()).unwrap();
    let folder = layout::launch(&layouts(image.path(), Some(&root)), true);
    assert!(folder.iter().all(|(_, entry)| entry.is_none()));
    let folder_backups = backups(image.path(), Some(&root));
    assert_eq!(folder_backups.ids().unwrap(), []);
    drop(folder);

    init(second);
    let (workbench, mut cx) = launch(image.path(), None, second);
    assert_eq!(
        states(&workbench, &mut cx),
        [
            (None, "hello".into(), true, 5),
            (None, "orphan".into(), true, 0)
        ]
    );
}

/// A launch with no argument after one that left nothing open shows the
/// welcome text, as one with no saved window does.
#[gpui::test]
fn nothing_saved_shows_the_welcome(first: &mut TestAppContext, second: &mut TestAppContext) {
    let _serial = serially();
    let state = tempfile::tempdir().unwrap();
    init(first);
    let (workbench, mut cx) = launch(state.path(), None, first);
    drop(workbench);
    run("workbench.close_tab", &mut cx);
    run("app.quit", &mut cx);
    first.quit();

    init(second);
    let (workbench, mut cx) = launch(state.path(), None, second);
    assert_eq!(
        states(&workbench, &mut cx),
        [(None, WELCOME.into(), false, 0)]
    );
}
