//! Frames of a stop across saves and split lines (debugger D6, T1849 F11
//! and F16): the stopped frame's and a caller's bands stay on their text
//! through insertions, deletions, saves and later refreshes, and one row
//! wide when their line is split. Also the harness
//! `debug_gutter_pending_verification_tests.rs` uses.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_dap::{
    AdapterSpec, AdapterTransport, HostOs, Message, SessionState, SystemClock,
    fake::{FakeAdapter, FakeLauncher, Line, Script},
    protocol::{Breakpoint, BreakpointBody, EventMessage, event},
};
use fe_editor::decorations::Tint;
use fe_gpui::{
    BreakpointMark, BreakpointState, EditorSettings, EditorTheme, EditorView, FrameArrow,
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    debug_breakpoints::BreakpointStore,
    debug_session::{DebugHost, Debugger, set_debug_host},
    task_commands::TestFiles,
    task_defs::ProjectFiles,
};

/// The project's files, read from disk.
struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

/// The program the fake debugs: `src/main.rs`'s five lines, and the empty
/// row after them, where no code is.
pub(super) const MAIN: &str =
    "let x = 1;\nlet point = 2;\nlet y = add(x, 2);\na + b\nprintln!(y);\n";

pub(super) const LAUNCH: &str = r#"
[[launch]]
name = "main"
adapter = "fake"
program = "${workspaceFolder}/target/debug/main"
"#;

/// A trusted workbench on a project the fake debugs, `src/main.rs` open in
/// its editor.
pub(super) struct Debug<'a> {
    pub(super) _dir: tempfile::TempDir,
    pub(super) main: PathBuf,
    pub(super) workbench: Entity<Workbench>,
    pub(super) editor: Entity<EditorView>,
    pub(super) launcher: Arc<FakeLauncher>,
    pub(super) cx: &'a mut VisualTestContext,
}

/// `script`, its program spinning on its last line until paused: a session
/// that runs on until it's stopped.
pub(super) fn spinning(mut script: Script) -> Script {
    let last = script.program.lines.len() - 1;
    script.program.lines[last] = Line {
        spin: true,
        ..script.program.lines[last].clone()
    };
    script
}

pub(super) fn debug(cx: &mut TestAppContext, spin: bool) -> Debug<'_> {
    debug_with(cx, move |script| match spin {
        true => spinning(script),
        false => script,
    })
}

/// [`debug`] with the fake's script made by `script` from the default,
/// whose program is `src/main.rs`.
pub(super) fn debug_with(
    cx: &mut TestAppContext,
    script: impl FnOnce(Script) -> Script,
) -> Debug<'_> {
    // The session's thread wakes the workbench's event loop.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let main = root.join("src").join("main.rs");
    fs::create_dir_all(main.parent().unwrap()).unwrap();
    fs::write(&main, MAIN).unwrap();
    fs::create_dir_all(root.join(".tenmoku")).unwrap();
    fs::write(root.join(".tenmoku/launch.toml"), LAUNCH).unwrap();
    let mut base = Script::default();
    base.program.path = main.to_string_lossy().into_owned();
    let script = script(base);
    let launcher = Arc::new(FakeLauncher::new(script));
    let host = DebugHost {
        launcher: Some(launcher.clone()),
        clock: Arc::new(SystemClock),
        specs: vec![AdapterSpec {
            types: vec!["fake".into()],
            languages: vec!["rust".into()],
            transport: AdapterTransport::Port(1),
            ..AdapterSpec::new("fake", "fake")
        }],
        ptrace_scope: Arc::new(|| Some("0\n".to_owned())),
        login: None,
        os: HostOs::Linux,
    };
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_debug_host(cx, host);
        cx.set_global(TestFiles(Arc::new(Disk)));
    });
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = main.clone();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("main.rs open");
    Debug {
        _dir: dir,
        main,
        workbench,
        editor,
        launcher,
        cx,
    }
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

impl Debug<'_> {
    pub(super) fn draw(&mut self) {
        draw(self.cx);
    }

    /// Draws until `done` holds. Panics after 30 s, naming `what`.
    pub(super) fn until(&mut self, what: &str, done: impl Fn(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.draw();
            if done(self) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    pub(super) fn store(&mut self) -> Entity<BreakpointStore> {
        let workbench = self.workbench.clone();
        self.cx
            .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.breakpoint_store(cx)))
            .expect("rooted")
    }

    pub(super) fn debugger(&mut self) -> Entity<Debugger> {
        let workbench = self.workbench.clone();
        self.cx
            .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.debugger(cx)))
            .expect("the window's debugger")
    }

    pub(super) fn state(&mut self) -> Option<SessionState> {
        let debugger = self.debugger();
        debugger.read_with(self.cx, |debugger, _| debugger.state())
    }

    pub(super) fn wait_state(&mut self, what: &str, done: impl Fn(&SessionState) -> bool) {
        self.until(what, |debug| debug.state().as_ref().is_some_and(&done));
    }

    pub(super) fn start(&mut self) {
        let workbench = self.workbench.clone();
        self.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.rerun_debug_named("main".to_owned(), window, cx)
            });
        });
        self.draw();
    }

    pub(super) fn fake(&mut self) -> FakeAdapter {
        let launcher = self.launcher.clone();
        self.until("the launch", |_| !launcher.launched().is_empty());
        launcher.wait_for_launch(0)
    }

    pub(super) fn toggle(&mut self, row: u32) {
        let (store, path) = (self.store(), self.main.clone());
        store.update(self.cx, |store, cx| store.toggle(&path, row, cx));
        self.draw();
    }

    /// The editor's marks, by row.
    pub(super) fn marks(&mut self) -> Vec<BreakpointMark> {
        self.editor
            .read_with(self.cx, |editor, _| editor.breakpoint_marks())
    }

    /// The debugger's bands the editor's last frame laid out: each one's
    /// buffer row and tint.
    pub(super) fn bands(&mut self) -> Vec<(u32, Tint)> {
        self.editor.update(self.cx, |editor, _| {
            let bands = editor.drawn_row_backgrounds();
            let text = editor.model().text();
            let starts: Vec<usize> = (0..text.row_count())
                .map(|row| text.row_start(row))
                .collect();
            let mut rows = Vec::new();
            for (bounds, tint, _) in bands {
                if !matches!(tint, Tint::DebugCurrent | Tint::DebugFrame) {
                    continue;
                }
                let row = (starts.iter()).position(|&start| {
                    (editor.caret_bounds_at(start))
                        .is_some_and(|caret| caret.origin.y == bounds.origin.y)
                });
                rows.push((row.expect("a band on a row") as u32, tint));
            }
            rows.sort_by_key(|(row, _)| *row);
            rows
        })
    }
}

impl Debug<'_> {
    /// The marks' breakpoints, without the frames' arrows: `(row, state)`.
    pub(super) fn breakpoints(&mut self) -> Vec<(u32, BreakpointState)> {
        (self.marks().into_iter())
            .filter_map(|mark| Some((mark.row, mark.breakpoint?)))
            .collect()
    }

    /// Replaces `range` of the editor's text with `text`, as typing would.
    pub(super) fn edit(&mut self, range: std::ops::Range<usize>, text: &str) {
        let editor = self.editor.clone();
        editor.update(self.cx, |editor, cx| {
            let model = editor.model_mut();
            model.select_range(range, false);
            model.insert(text);
            cx.notify();
        });
        self.draw();
    }

    /// Where buffer row `row` starts in the editor's text.
    pub(super) fn row_start(&mut self, row: u32) -> usize {
        self.editor
            .read_with(self.cx, |editor, _| editor.model().text().row_start(row))
    }
}

/// The adapter moves breakpoint `id` to `line` (from 1).
pub(super) fn relocate(fake: &FakeAdapter, id: i64, line: i64) {
    let body = BreakpointBody {
        reason: "changed".into(),
        breakpoint: Breakpoint {
            verified: true,
            id: Some(id),
            line: Some(line),
            ..Default::default()
        },
        ..Default::default()
    };
    fake.send(Message::Event(EventMessage::new::<event::BreakpointEvent>(
        900 + id,
        &body,
    )));
}

/// Saves the editor, as `editor.save` does.
fn save(debug: &mut Debug<'_>) {
    let editor = debug.editor.clone();
    debug
        .cx
        .update(|window, cx| editor.update(cx, |editor, cx| editor.save(window, cx)))
        .detach();
    debug.draw();
    let dirty = editor.read_with(debug.cx, |editor, _| editor.model().buffer().is_dirty());
    assert!(!dirty, "saved");
}

/// A stop on row 2 with a caller selected on row 1.
fn stopped_with_a_caller(cx: &mut TestAppContext) -> Debug<'_> {
    let mut debug = debug(cx, true);
    debug.toggle(2);
    debug.start();
    debug.wait_state("the stop", |state| *state == SessionState::Stopped);
    debug.until("the band", |debug| {
        debug.bands() == [(2, Tint::DebugCurrent)]
    });
    debug.select_caller(1);
    assert_eq!(
        debug.bands(),
        [(1, Tint::DebugFrame), (2, Tint::DebugCurrent)]
    );
    debug
}

impl Debug<'_> {
    /// D9 selects the caller on row `row` of the file on disk.
    fn select_caller(&mut self, row: u32) {
        let (workbench, main) = (self.workbench.clone(), self.main.clone());
        workbench.update(self.cx, |workbench, cx| {
            workbench.band_debug_frame(Some((&main, row)), cx)
        });
        self.draw();
    }

    /// The frames' arrows: `(row, arrow)`.
    fn arrows(&mut self) -> Vec<(u32, FrameArrow)> {
        (self.marks().into_iter())
            .filter_map(|mark| Some((mark.row, mark.frame?)))
            .collect()
    }

    /// A refresh the frames didn't cause: an unrelated breakpoint added.
    fn unrelated_refresh(&mut self, row: u32) {
        self.toggle(row);
    }
}

/// F11: both bands and arrows stay on their text through an insertion, a
/// deletion, each save, and refreshes after.
#[gpui::test]
fn frames_keep_their_place_through_edits_and_saves(cx: &mut TestAppContext) {
    let mut debug = stopped_with_a_caller(cx);
    // Insert a line above both, and save: the store's baseline moves.
    debug.edit(0..0, "// new\n");
    assert_eq!(
        debug.bands(),
        [(2, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
    save(&mut debug);
    debug.unrelated_refresh(5);
    assert_eq!(
        debug.bands(),
        [(2, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
    assert_eq!(
        debug.arrows(),
        [(2, FrameArrow::Caller), (3, FrameArrow::Current)]
    );
    // And a deletion of it again.
    let end = debug.row_start(1);
    debug.edit(0..end, "");
    save(&mut debug);
    debug.unrelated_refresh(5);
    assert_eq!(
        debug.bands(),
        [(1, Tint::DebugFrame), (2, Tint::DebugCurrent)]
    );
    assert_eq!(
        debug.arrows(),
        [(1, FrameArrow::Caller), (2, FrameArrow::Current)]
    );
}

/// F16: a line split inside a frame's line leaves its band on one row, the
/// arrow's, without a breakpoint row moving to say so.
#[gpui::test]
fn splitting_a_frames_line_leaves_its_band_on_one_row(cx: &mut TestAppContext) {
    let mut debug = stopped_with_a_caller(cx);
    // Inside the caller's line.
    let at = debug.row_start(1) + 3;
    debug.edit(at..at, "\n");
    assert_eq!(
        debug.bands(),
        [(1, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
    // Inside the stopped line, now row 3.
    let at = debug.row_start(3) + 3;
    debug.edit(at..at, "\n");
    assert_eq!(
        debug.bands(),
        [(1, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
    assert_eq!(
        debug.arrows(),
        [(1, FrameArrow::Caller), (3, FrameArrow::Current)]
    );
    save(&mut debug);
    debug.unrelated_refresh(6);
    assert_eq!(
        debug.bands(),
        [(1, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
}

/// F6: a caller keeps its place when another caller, or none, is selected
/// after an edit and a save, and it's selected again.
#[gpui::test]
fn a_caller_selected_again_after_a_save_is_where_its_text_is(cx: &mut TestAppContext) {
    let mut debug = stopped_with_a_caller(cx);
    debug.edit(0..0, "// new\n");
    save(&mut debug);
    assert_eq!(
        debug.bands(),
        [(2, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
    // Another caller (row 0 of the file as saved), then none, then the
    // first again (row 1 as it was selected).
    debug.select_caller(0);
    assert_eq!(
        debug.bands(),
        [(0, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
    let (workbench, cx_) = (debug.workbench.clone(), &mut *debug.cx);
    workbench.update(cx_, |workbench, cx| workbench.band_debug_frame(None, cx));
    debug.draw();
    assert_eq!(debug.bands(), [(3, Tint::DebugCurrent)]);
    debug.select_caller(1);
    assert_eq!(
        debug.bands(),
        [(2, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
    assert_eq!(
        debug.arrows(),
        [(2, FrameArrow::Caller), (3, FrameArrow::Current)]
    );
}
