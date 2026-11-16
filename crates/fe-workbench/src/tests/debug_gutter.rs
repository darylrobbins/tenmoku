//! The debugger in the editors (`debug_gutter.rs`, debugger card D6)
//! against fe-dap's fake adapter, which the host's launcher starts
//! in-process: each breakpoint's glyph as the store and the session's
//! verification have it, the stopped frame's band and arrow, a caller
//! frame's through D9's seam, and the line-number column's menu.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_dap::{
    AdapterSpec, AdapterTransport, HostOs, Message, SessionState, SystemClock,
    fake::{FakeAdapter, FakeLauncher, Fault, FaultKind, Line, Script, Trigger},
    protocol::{Breakpoint, BreakpointBody, EventMessage, event},
};
use fe_editor::decorations::Tint;
use fe_gpui::{
    BreakpointGlyph, BreakpointMark, BreakpointState, EditorSettings, EditorTheme, EditorView,
    FrameArrow, commands::RunCommand,
};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, Pixels, Point, TestAppContext,
    VisualTestContext, point,
};
use kiln_l10n::tr;

use crate::{
    Launch, Workbench,
    debug_breakpoints::BreakpointStore,
    debug_session::{DebugHost, Debugger, DebuggerEvent, set_debug_host},
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
const MAIN: &str = "let x = 1;\nlet point = 2;\nlet y = add(x, 2);\na + b\nprintln!(y);\n";

const LAUNCH: &str = r#"
[[launch]]
name = "main"
adapter = "fake"
program = "${workspaceFolder}/target/debug/main"
"#;

/// A trusted workbench on a project the fake debugs, `src/main.rs` open in
/// its editor.
struct Debug<'a> {
    _dir: tempfile::TempDir,
    main: PathBuf,
    workbench: Entity<Workbench>,
    editor: Entity<EditorView>,
    launcher: Arc<FakeLauncher>,
    cx: &'a mut VisualTestContext,
}

/// `script`, its program spinning on its last line until paused: a session
/// that runs on until it's stopped.
fn spinning(mut script: Script) -> Script {
    let last = script.program.lines.len() - 1;
    script.program.lines[last] = Line {
        spin: true,
        ..script.program.lines[last].clone()
    };
    script
}

fn debug(cx: &mut TestAppContext, spin: bool) -> Debug<'_> {
    debug_with(cx, move |script| match spin {
        true => spinning(script),
        false => script,
    })
}

/// [`debug`] with the fake's script made by `script` from the default,
/// whose program is `src/main.rs`.
fn debug_with(cx: &mut TestAppContext, script: impl FnOnce(Script) -> Script) -> Debug<'_> {
    // The session's thread wakes the workbench's event loop.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let main = root.join("src").join("main.rs");
    fs::create_dir_all(main.parent().unwrap()).unwrap();
    fs::write(&main, MAIN).unwrap();
    fs::create_dir_all(root.join(".tenmoku")).unwrap();
    fs::write(root.join(".tenmoku/launch.toml"), LAUNCH).unwrap();
    // Its Rust file would start a real rust-analyzer that outlives the test.
    let settings = fe_config::project_settings_path(&root);
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();
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

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

impl Debug<'_> {
    fn draw(&mut self) {
        draw(self.cx);
    }

    /// Draws until `done` holds. Panics after 30 s, naming `what`.
    fn until(&mut self, what: &str, done: impl Fn(&mut Self) -> bool) {
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

    fn store(&mut self) -> Entity<BreakpointStore> {
        let workbench = self.workbench.clone();
        self.cx
            .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.breakpoint_store(cx)))
            .expect("rooted")
    }

    fn debugger(&mut self) -> Entity<Debugger> {
        let workbench = self.workbench.clone();
        self.cx
            .update(|_, cx| workbench.update(cx, |workbench, cx| workbench.debugger(cx)))
            .expect("the window's debugger")
    }

    fn state(&mut self) -> Option<SessionState> {
        let debugger = self.debugger();
        debugger.read_with(self.cx, |debugger, _| debugger.state())
    }

    fn wait_state(&mut self, what: &str, done: impl Fn(&SessionState) -> bool) {
        self.until(what, |debug| debug.state().as_ref().is_some_and(&done));
    }

    fn start(&mut self) {
        let workbench = self.workbench.clone();
        self.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.rerun_debug_named("main".to_owned(), window, cx)
            });
        });
        self.draw();
    }

    fn fake(&mut self) -> FakeAdapter {
        let launcher = self.launcher.clone();
        self.until("the launch", |_| !launcher.launched().is_empty());
        launcher.wait_for_launch(0)
    }

    fn toggle(&mut self, row: u32) {
        let (store, path) = (self.store(), self.main.clone());
        store.update(self.cx, |store, cx| store.toggle(&path, row, cx));
        self.draw();
    }

    /// The editor's marks, by row.
    fn marks(&mut self) -> Vec<BreakpointMark> {
        self.editor
            .read_with(self.cx, |editor, _| editor.breakpoint_marks())
    }

    /// The glyphs the editor's last frame drew over its line numbers.
    fn glyphs(&mut self) -> Vec<(u32, BreakpointGlyph)> {
        self.editor.read_with(self.cx, |editor, _| {
            (editor.drawn_gutter().expect("drawn").breakpoints.iter())
                .map(|drawn| (drawn.row, drawn.glyph))
                .collect()
        })
    }

    /// The debugger's bands the editor's last frame laid out: each one's
    /// buffer row and tint.
    fn bands(&mut self) -> Vec<(u32, Tint)> {
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

    /// The middle of buffer row `row`'s line number.
    fn number_at(&mut self, row: u32) -> Point<Pixels> {
        self.editor.update(self.cx, |editor, _| {
            let drawn = editor.drawn_gutter().expect("drawn").clone();
            let column = drawn.columns[0].union(&drawn.columns[1]);
            let offset = editor.model().text().row_start(row);
            let y = editor.caret_bounds_at(offset).expect("shown").center().y;
            point(column.center().x, y)
        })
    }

    /// Right-clicks row `row`'s line number: its menu's titles.
    fn menu(&mut self, row: u32) -> Vec<String> {
        let at = self.number_at(row);
        self.cx
            .simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        self.cx
            .simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
        self.draw();
        let titles = self
            .editor
            .read_with(self.cx, |editor, _| editor.breakpoint_menu())
            .expect("a menu");
        titles.into_iter().map(|title| title.to_string()).collect()
    }

    /// Runs workbench command `id` as a menu item would.
    fn run(&mut self, id: &str) {
        let run = RunCommand(
            Invocation::of(id)
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        self.draw();
    }

    fn choose(&mut self, ix: usize) {
        let editor = self.editor.clone();
        self.cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.choose_breakpoint_menu_item(ix, window, cx)
            })
        });
        self.draw();
    }

    /// The store's breakpoints of `src/main.rs`, as (row, enabled).
    fn stored(&mut self) -> Vec<(u32, bool)> {
        let (store, path) = (self.store(), self.main.clone());
        store.read_with(self.cx, |store, _| {
            (store.breakpoints(&path).into_iter())
                .map(|breakpoint| (breakpoint.line - 1, breakpoint.enabled))
                .collect()
        })
    }
}

fn mark(
    row: u32,
    breakpoint: Option<BreakpointState>,
    frame: Option<FrameArrow>,
) -> BreakpointMark {
    BreakpointMark {
        row,
        breakpoint,
        frame,
        target: None,
    }
}

/// C3: with no session every enabled breakpoint is a dot and a disabled
/// one grey; a session's adapter leaving one unbound makes it a ring with
/// the adapter's message, and one it moves draws where it says, for the
/// session's life only.
#[gpui::test]
fn states_follow_verification_events(cx: &mut TestAppContext) {
    let mut debug = debug(cx, true);
    // Row 2 has code, row 5 (line 6, after the program) none.
    debug.toggle(2);
    debug.toggle(3);
    debug.toggle(5);
    let store = debug.store();
    let path = debug.main.clone();
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 3, false, cx));
    debug.draw();
    let enabled = Some(BreakpointState::Enabled);
    let disabled = Some(BreakpointState::Disabled);
    assert_eq!(
        debug.marks(),
        [
            mark(2, enabled.clone(), None),
            mark(3, disabled.clone(), None),
            mark(5, enabled.clone(), None),
        ],
        "no session: enabled and disabled"
    );
    assert_eq!(
        debug.glyphs(),
        [
            (2, BreakpointGlyph::Dot),
            (3, BreakpointGlyph::Disabled),
            (5, BreakpointGlyph::Dot),
        ]
    );

    // It stops at row 2's, the first with code.
    debug.start();
    let fake = debug.fake();
    let unbound = Some(BreakpointState::Unverified {
        message: Some("no code at this line".into()),
    });
    let current = Some(FrameArrow::Current);
    debug.until("the adapter's verification", |debug| {
        debug.marks()
            == [
                mark(2, enabled.clone(), current),
                mark(3, disabled.clone(), None),
                mark(5, unbound.clone(), None),
            ]
    });
    assert_eq!(
        debug.glyphs(),
        [
            (2, BreakpointGlyph::Dot),
            (2, BreakpointGlyph::CurrentArrow),
            (3, BreakpointGlyph::Disabled),
            (5, BreakpointGlyph::Ring),
        ]
    );
    // Its tooltip and name carry the adapter's message.
    let label = debug
        .editor
        .read_with(debug.cx, |editor, _| editor.breakpoint_row_label(5));
    let message = "no code at this line";
    let want = tr!("gpui-debug-breakpoint-unbound", line = 6, message = message);
    assert_eq!(label, want);
    assert_eq!(
        label.as_ref(),
        "Breakpoint, line 6, not bound: no code at this line"
    );
    let label = debug
        .editor
        .read_with(debug.cx, |editor, _| editor.breakpoint_row_label(3));
    assert_eq!(label, tr!("gpui-debug-breakpoint-disabled", line = 4));

    // The adapter moves line 3's breakpoint to line 5: drawn there for the
    // session's life.
    let debugger = debug.debugger();
    let main = debug.main.clone();
    let id = debugger.read_with(debug.cx, |debugger, _| {
        (debugger.verification(&main).iter())
            .find(|verification| verification.line == 3)
            .and_then(|verification| verification.id)
            .expect("line 3's id")
    });
    let body = BreakpointBody {
        reason: "changed".into(),
        breakpoint: Breakpoint {
            verified: true,
            id: Some(id),
            line: Some(5),
            ..Default::default()
        },
        ..Default::default()
    };
    fake.send(Message::Event(EventMessage::new::<event::BreakpointEvent>(
        900, &body,
    )));
    debug.until("the moved breakpoint", |debug| {
        debug.marks()
            == [
                mark(2, None, current),
                mark(3, disabled.clone(), None),
                BreakpointMark {
                    target: Some(2),
                    ..mark(4, enabled.clone(), None)
                },
                mark(5, unbound.clone(), None),
            ]
    });
    // Not persisted: the store still has it on row 2.
    assert_eq!(debug.stored(), [(2, true), (3, false), (5, true)]);
    // A click on the moved one takes it away: the one from row 2.
    let at = debug.number_at(4);
    debug
        .cx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    debug
        .cx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    debug.draw();
    assert_eq!(debug.stored(), [(3, false), (5, true)]);
    debug.toggle(2);

    // The session ends: every breakpoint back on its own row, a dot.
    debug.run("debug.stop");
    debug.wait_state("the end", SessionState::is_ended);
    debug.until("plain marks again", |debug| {
        debug.marks()
            == [
                mark(2, enabled.clone(), None),
                mark(3, disabled.clone(), None),
                mark(5, enabled.clone(), None),
            ]
    });
}

/// C4: a stop bands the top frame's row and draws the filled arrow there,
/// over its breakpoint's dot; D9's seam bands a caller frame's row with
/// the outlined arrow; both go when the session runs on. The band draws
/// without a gutter too.
#[gpui::test]
fn a_stop_bands_the_stopped_row_and_draws_the_arrow(cx: &mut TestAppContext) {
    let mut debug = debug(cx, true);
    debug.toggle(2);
    debug.start();
    debug.wait_state("the stop", |state| *state == SessionState::Stopped);
    debug.until("the band", |debug| {
        debug.bands() == [(2, Tint::DebugCurrent)]
    });
    assert_eq!(
        debug.marks(),
        [mark(
            2,
            Some(BreakpointState::Enabled),
            Some(FrameArrow::Current)
        )]
    );
    assert_eq!(
        debug.glyphs(),
        [
            (2, BreakpointGlyph::Dot),
            (2, BreakpointGlyph::CurrentArrow)
        ]
    );

    // D9 selects a caller frame on row 0.
    let (workbench, main) = (debug.workbench.clone(), debug.main.clone());
    workbench.update(debug.cx, |workbench, cx| {
        workbench.band_debug_frame(Some((&main, 0)), cx)
    });
    debug.draw();
    assert_eq!(
        debug.bands(),
        [(0, Tint::DebugFrame), (2, Tint::DebugCurrent)]
    );
    assert_eq!(
        debug.glyphs(),
        [
            (0, BreakpointGlyph::CallerArrow),
            (2, BreakpointGlyph::Dot),
            (2, BreakpointGlyph::CurrentArrow),
        ]
    );
    // Opaque, in the theme's colours.
    let theme = debug
        .editor
        .read_with(debug.cx, |editor, _| editor.theme().clone());
    let colours: Vec<_> = debug.editor.read_with(debug.cx, |editor, _| {
        (editor.drawn_row_backgrounds().into_iter())
            .filter(|(_, tint, _)| matches!(tint, Tint::DebugCurrent | Tint::DebugFrame))
            .map(|(_, tint, colour)| (tint, colour))
            .collect()
    });
    assert_eq!(
        colours,
        [
            (Tint::DebugFrame, theme.debug_frame_line),
            (Tint::DebugCurrent, theme.debug_current_line)
        ]
    );

    // Without a gutter, the bands still draw; the marks don't.
    let editor = debug.editor.clone();
    editor.update(debug.cx, |editor, cx| editor.set_gutter(false, cx));
    debug.draw();
    assert_eq!(
        debug.bands(),
        [(0, Tint::DebugFrame), (2, Tint::DebugCurrent)]
    );
    assert_eq!(debug.glyphs(), []);
    editor.update(debug.cx, |editor, cx| editor.set_gutter(true, cx));

    // Continue (`debug.start` while stopped): it runs on, spinning, and
    // the bands and arrows go.
    debug.run("debug.start");
    debug.wait_state("running on", |state| *state == SessionState::Running);
    debug.until("no bands", |debug| debug.bands().is_empty());
    assert_eq!(debug.glyphs(), [(2, BreakpointGlyph::Dot)]);
}

/// C5: a right click on a line number offers Add Breakpoint, then Disable
/// Breakpoint and Remove Breakpoint, or Enable Breakpoint once disabled;
/// the menu and its items are named for assistive technology.
#[gpui::test]
fn the_context_menu_disables_enables_and_removes(cx: &mut TestAppContext) {
    let mut debug = debug(cx, false);
    let (add, disable, enable, remove) = (
        tr!("workbench-debug-menu-add-breakpoint").to_string(),
        tr!("workbench-debug-menu-disable-breakpoint").to_string(),
        tr!("workbench-debug-menu-enable-breakpoint").to_string(),
        tr!("workbench-debug-menu-remove-breakpoint").to_string(),
    );
    assert_eq!(add, "Add Breakpoint");

    assert_eq!(debug.menu(1), std::slice::from_ref(&add));
    // Named: the menu, and each item by its title.
    let nodes = debug.cx.update(|window, cx| fe_ui::nodes(window, cx));
    let named = |role: fe_ui::Role, label: &str| {
        (nodes.iter()).any(|node| node.role == role && node.label.as_ref() == label)
    };
    assert!(named(
        fe_ui::Role::Menu,
        tr!("gpui-debug-breakpoint-menu").as_ref()
    ));
    assert!(named(fe_ui::Role::MenuItem, &add));
    debug.choose(0);
    assert_eq!(debug.stored(), [(1, true)]);
    assert_eq!(debug.glyphs(), [(1, BreakpointGlyph::Dot)]);

    assert_eq!(debug.menu(1), [disable.clone(), remove.clone()]);
    let nodes = debug.cx.update(|window, cx| fe_ui::nodes(window, cx));
    for title in [&disable, &remove] {
        assert!(
            (nodes.iter())
                .any(|node| node.role == fe_ui::Role::MenuItem
                    && node.label.as_ref() == title.as_str()),
            "{title}"
        );
    }
    debug.choose(0);
    assert_eq!(debug.stored(), [(1, false)]);
    assert_eq!(debug.glyphs(), [(1, BreakpointGlyph::Disabled)]);

    assert_eq!(debug.menu(1), [enable.clone(), remove.clone()]);
    debug.choose(0);
    assert_eq!(debug.stored(), [(1, true)]);

    assert_eq!(debug.menu(1), [disable, remove]);
    debug.choose(1);
    assert_eq!(debug.stored(), []);
    assert_eq!(debug.glyphs(), []);

    // A plain click on the number toggles, the caret staying put.
    let caret = debug.editor.read_with(debug.cx, |editor, _| {
        editor.model().primary_selection().range
    });
    let at = debug.number_at(3);
    debug
        .cx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    debug
        .cx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    debug.draw();
    assert_eq!(debug.stored(), [(3, true)]);
    let after = debug.editor.read_with(debug.cx, |editor, _| {
        editor.model().primary_selection().range
    });
    assert_eq!(after, caret);
}

impl Debug<'_> {
    /// The marks' breakpoints, without the frames' arrows: `(row, state)`.
    fn breakpoints(&mut self) -> Vec<(u32, BreakpointState)> {
        (self.marks().into_iter())
            .filter_map(|mark| Some((mark.row, mark.breakpoint?)))
            .collect()
    }

    /// Replaces `range` of the editor's text with `text`, as typing would.
    fn edit(&mut self, range: std::ops::Range<usize>, text: &str) {
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
    fn row_start(&mut self, row: u32) -> usize {
        self.editor
            .read_with(self.cx, |editor, _| editor.model().text().row_start(row))
    }

    /// The adapter's id for the breakpoint sent on `line`, once it's said.
    fn adapter_id(&mut self, line: u32) -> i64 {
        let (debugger, main) = (self.debugger(), self.main.clone());
        let id = |debug: &mut Self| {
            debugger.read_with(debug.cx, |debugger, _| {
                (debugger.verification(&main).iter())
                    .find(|said| said.line == line)
                    .and_then(|said| said.id)
            })
        };
        self.until("the adapter's id", |debug| id(debug).is_some());
        id(self).unwrap()
    }

    /// Clicks buffer row `row`'s line number.
    fn click(&mut self, row: u32) {
        let at = self.number_at(row);
        self.cx
            .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
        self.cx
            .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        self.draw();
    }
}

/// The adapter moves breakpoint `id` to `line` (from 1).
fn relocate(fake: &FakeAdapter, id: i64, line: i64) {
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

/// R1 F1: two breakpoints joined onto one row by an edit don't shift what
/// the adapter said of the others: each keeps its own verification, by its
/// line on disk.
#[gpui::test]
fn merged_rows_keep_each_breakpoints_verification(cx: &mut TestAppContext) {
    let mut debug = debug(cx, true);
    // Rows 3 and 4 have code; row 5 (line 6) none.
    for row in [3, 4, 5] {
        debug.toggle(row);
    }
    debug.start();
    let unbound = BreakpointState::Unverified {
        message: Some("no code at this line".into()),
    };
    debug.until("the verification", |debug| {
        debug.breakpoints()
            == [
                (3, BreakpointState::Enabled),
                (4, BreakpointState::Enabled),
                (5, unbound.clone()),
            ]
    });
    // Join rows 3 and 4: the unbound one moves up to row 4 and stays
    // unbound with its message.
    let end = debug.row_start(4) - 1;
    debug.edit(end..end + 1, " ");
    debug.until("the joined rows", |debug| {
        debug.breakpoints() == [(3, BreakpointState::Enabled), (4, unbound.clone())]
    });
}

/// R1 F2: a breakpoint the adapter moved stays on the line it reported as
/// edits move that line, whatever they do between it and the line it was
/// set on, through later refreshes.
#[gpui::test]
fn a_moved_breakpoint_follows_its_reported_line_through_edits(cx: &mut TestAppContext) {
    let mut debug = debug(cx, true);
    debug.toggle(1);
    debug.start();
    let fake = debug.fake();
    let id = debug.adapter_id(2);
    relocate(&fake, id, 5);
    debug.until("the moved breakpoint", |debug| {
        debug.breakpoints() == [(4, BreakpointState::Enabled)]
    });
    // A row inserted between the two: the reported line is row 5 now.
    let at = debug.row_start(3);
    debug.edit(at..at, "let z = 0;\n");
    debug.until("the shifted line", |debug| {
        debug.breakpoints() == [(5, BreakpointState::Enabled)]
    });
    // An unrelated refresh keeps it there.
    let workbench = debug.workbench.clone();
    workbench.update(debug.cx, |workbench, cx| {
        workbench.band_debug_frame(None, cx)
    });
    debug.draw();
    assert_eq!(debug.breakpoints(), [(5, BreakpointState::Enabled)]);
    // A row deleted between them: back to row 4.
    let (start, end) = (debug.row_start(3), debug.row_start(4));
    debug.edit(start..end, "");
    debug.until("the line moved back", |debug| {
        debug.breakpoints() == [(4, BreakpointState::Enabled)]
    });
}

/// A session where the adapter moved row 1's breakpoint onto row 4, which
/// has a disabled one.
fn moved_onto_a_disabled_one(cx: &mut TestAppContext) -> Debug<'_> {
    let mut debug = debug(cx, true);
    debug.toggle(1);
    debug.toggle(4);
    let (store, path) = (debug.store(), debug.main.clone());
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 4, false, cx));
    debug.start();
    let fake = debug.fake();
    let id = debug.adapter_id(2);
    relocate(&fake, id, 5);
    debug.until("the moved breakpoint", |debug| {
        debug.breakpoints() == [(4, BreakpointState::Enabled)]
    });
    debug
}

/// R1 F3: one the adapter moved onto a disabled one's row shows as the
/// first there, and the row's menu is that one's.
#[gpui::test]
fn a_moved_breakpoints_row_offers_its_menu(cx: &mut TestAppContext) {
    let mut debug = moved_onto_a_disabled_one(cx);
    let disable = tr!("workbench-debug-menu-disable-breakpoint").to_string();
    let remove = tr!("workbench-debug-menu-remove-breakpoint").to_string();
    assert_eq!(debug.menu(4), [disable, remove]);
    debug.choose(0);
    assert_eq!(debug.stored(), [(1, false), (4, false)]);
}

/// R1 F3: a click on that row removes the moved one, not the one under it.
#[gpui::test]
fn a_click_on_a_moved_breakpoints_row_removes_it(cx: &mut TestAppContext) {
    let mut debug = moved_onto_a_disabled_one(cx);
    debug.click(4);
    assert_eq!(debug.stored(), [(4, false)]);
}

/// R1 F4, F9: a caller frame is banded only in a stop, not on the stopped
/// row itself, and goes when that stop is retired for another.
#[gpui::test]
fn a_caller_frame_lasts_one_stop_and_yields_to_the_current_row(cx: &mut TestAppContext) {
    let mut debug = debug(cx, true);
    let (workbench, main) = (debug.workbench.clone(), debug.main.clone());
    // No stop: nothing to band.
    workbench.update(debug.cx, |workbench, cx| {
        workbench.band_debug_frame(Some((&main, 0)), cx)
    });
    debug.draw();
    assert_eq!(debug.bands(), []);

    debug.toggle(2);
    debug.start();
    debug.wait_state("the stop", |state| *state == SessionState::Stopped);
    debug.until("the band", |debug| {
        debug.bands() == [(2, Tint::DebugCurrent)]
    });
    // A caller on the stopped row (a recursive call): the current row's.
    workbench.update(debug.cx, |workbench, cx| {
        workbench.band_debug_frame(Some((&main, 2)), cx)
    });
    debug.draw();
    assert_eq!(debug.bands(), [(2, Tint::DebugCurrent)]);
    assert_eq!(
        debug.marks(),
        [mark(
            2,
            Some(BreakpointState::Enabled),
            Some(FrameArrow::Current)
        )]
    );
    workbench.update(debug.cx, |workbench, cx| {
        workbench.band_debug_frame(Some((&main, 0)), cx)
    });
    debug.draw();
    assert_eq!(
        debug.bands(),
        [(0, Tint::DebugFrame), (2, Tint::DebugCurrent)]
    );
    // The stop retired for another while stopped (D7's State(Stopped)).
    let debugger = debug.debugger();
    debugger.update(debug.cx, |_, cx| {
        cx.emit(DebuggerEvent::State(SessionState::Stopped))
    });
    debug.draw();
    assert!(!debug.bands().contains(&(0, Tint::DebugFrame)));
    assert!(
        (debug.marks().iter()).all(|mark| mark.frame != Some(FrameArrow::Caller)),
        "{:?}",
        debug.marks()
    );
}

/// R1 F5: Save As shows the new file's breakpoints, none here, not the
/// old file's.
#[gpui::test]
fn save_as_shows_the_new_files_breakpoints(cx: &mut TestAppContext) {
    let mut debug = debug(cx, false);
    debug.toggle(2);
    assert_eq!(debug.breakpoints(), [(2, BreakpointState::Enabled)]);
    let other = debug.main.with_file_name("other.rs");
    let (workbench, editor) = (debug.workbench.clone(), debug.editor.clone());
    workbench.update_in(debug.cx, |workbench, window, cx| {
        workbench.save_editor_as(&editor, other.clone(), window, cx)
    });
    debug.until("the save", |debug| {
        debug.editor.read_with(debug.cx, |editor, _| {
            editor.model().path() == Some(other.as_path())
        })
    });
    debug.until("no breakpoints", |debug| debug.breakpoints().is_empty());
}

/// R1 F6: a stop the adapter reports by a symlinked path bands the file's
/// editor.
#[cfg(unix)]
#[gpui::test]
fn a_stop_by_a_symlinked_path_bands_the_file(cx: &mut TestAppContext) {
    let link = tempfile::tempdir().unwrap();
    let mut debug = debug_with(cx, |mut script| {
        let real = PathBuf::from(&script.program.path);
        let linked = link.path().join("src");
        std::os::unix::fs::symlink(real.parent().unwrap(), &linked).unwrap();
        script.program.path = linked.join("main.rs").to_string_lossy().into_owned();
        script.stop_on_entry = true;
        script
    });
    debug.start();
    debug.wait_state("the stop", |state| *state == SessionState::Stopped);
    debug.until("the band", |debug| {
        debug.bands() == [(0, Tint::DebugCurrent)]
    });
    assert_eq!(debug.marks(), [mark(0, None, Some(FrameArrow::Current))]);
}

/// R1 F7: an edit while the menu is open closes it, so no item acts on a
/// row the text has moved.
#[gpui::test]
fn an_edit_closes_the_menu(cx: &mut TestAppContext) {
    let mut debug = debug(cx, false);
    debug.toggle(3);
    let disable = tr!("workbench-debug-menu-disable-breakpoint").to_string();
    let remove = tr!("workbench-debug-menu-remove-breakpoint").to_string();
    assert_eq!(debug.menu(3), [disable, remove]);
    // A row inserted above: the breakpoint is on row 4 now.
    debug.edit(0..0, "let w = 0;\n");
    let open = debug
        .editor
        .read_with(debug.cx, |editor, _| editor.breakpoint_menu());
    assert_eq!(open, None);
    debug.choose(1);
    assert_eq!(debug.breakpoints(), [(4, BreakpointState::Enabled)]);
}

/// R1 F8: while a session hasn't said what it made of a breakpoint (its
/// setBreakpoints unanswered), the breakpoint is a ring, not a dot.
#[gpui::test]
fn an_unanswered_breakpoint_is_a_ring(cx: &mut TestAppContext) {
    let mut debug = debug_with(cx, |script| Script {
        faults: vec![Fault::new(
            "setBreakpoints",
            FaultKind::ReplyAfter(Trigger::Manual),
        )],
        ..spinning(script)
    });
    debug.start();
    let fake = debug.fake();
    debug.wait_state("running", |state| *state == SessionState::Running);
    debug.toggle(4);
    let pending = BreakpointState::Unverified { message: None };
    debug.until("the pending ring", |debug| {
        debug.breakpoints() == [(4, pending.clone())]
    });
    assert_eq!(debug.glyphs(), [(4, BreakpointGlyph::Ring)]);
    fake.release();
    debug.until("the answer", |debug| {
        debug.breakpoints() == [(4, BreakpointState::Enabled)]
    });
}

/// R2 F10: right after an edit between a moved breakpoint's own line and
/// the line the adapter put it on, before anything refreshes, the row it's
/// drawn on still acts on it.
#[gpui::test]
fn a_moved_breakpoint_acts_on_its_own_row_right_after_an_edit(cx: &mut TestAppContext) {
    let mut debug = debug(cx, true);
    debug.toggle(1);
    debug.start();
    let fake = debug.fake();
    let id = debug.adapter_id(2);
    relocate(&fake, id, 5);
    debug.until("the moved breakpoint", |debug| {
        debug.breakpoints() == [(4, BreakpointState::Enabled)]
    });
    let at = debug.row_start(3);
    debug.edit(at..at, "let z = 0;\n");
    assert_eq!(debug.breakpoints(), [(5, BreakpointState::Enabled)]);
    let disable = tr!("workbench-debug-menu-disable-breakpoint").to_string();
    let remove = tr!("workbench-debug-menu-remove-breakpoint").to_string();
    assert_eq!(debug.menu(5), [disable, remove]);
    debug.choose(1);
    assert_eq!(debug.stored(), []);
    // And a click, the same way.
    debug.toggle(1);
    debug.until("moved again", |debug| {
        debug.breakpoints() == [(1, BreakpointState::Enabled)]
            || debug.breakpoints() == [(5, BreakpointState::Enabled)]
    });
    let id = debug.adapter_id(2);
    relocate(&fake, id, 5);
    debug.until("moved", |debug| {
        debug.breakpoints() == [(5, BreakpointState::Enabled)]
    });
    let at = debug.row_start(3);
    debug.edit(at..at, "let v = 0;\n");
    assert_eq!(debug.breakpoints(), [(6, BreakpointState::Enabled)]);
    debug.click(6);
    assert_eq!(debug.stored(), []);
}

/// R2 F11: the stopped frame's band and arrow, and a caller's, stay on
/// their lines' text through an edit above them and the refreshes after.
#[gpui::test]
fn frames_stay_on_their_lines_through_edits(cx: &mut TestAppContext) {
    let mut debug = debug(cx, true);
    debug.toggle(2);
    debug.start();
    debug.wait_state("the stop", |state| *state == SessionState::Stopped);
    debug.until("the band", |debug| {
        debug.bands() == [(2, Tint::DebugCurrent)]
    });
    debug.edit(0..0, "// a new first line\n");
    let (workbench, main) = (debug.workbench.clone(), debug.main.clone());
    // D9 selects the caller on line 1, now row 1; that refreshes too.
    workbench.update(debug.cx, |workbench, cx| {
        workbench.band_debug_frame(Some((&main, 0)), cx)
    });
    debug.draw();
    assert_eq!(
        debug.bands(),
        [(1, Tint::DebugFrame), (3, Tint::DebugCurrent)]
    );
    let arrows: Vec<(u32, FrameArrow)> = (debug.marks().into_iter())
        .filter_map(|mark| Some((mark.row, mark.frame?)))
        .collect();
    assert_eq!(arrows, [(1, FrameArrow::Caller), (3, FrameArrow::Current)]);
    assert_eq!(debug.breakpoints(), [(3, BreakpointState::Enabled)]);
}

/// A session with row 1's breakpoint verified, whose next four
/// setBreakpoints (the 2nd to the 5th sends) the fake answers as `fault`
/// says.
fn verified_then(cx: &mut TestAppContext, fault: FaultKind) -> (Debug<'_>, FakeAdapter) {
    let mut debug = debug_with(cx, move |script| Script {
        faults: (2..=5)
            .map(|nth| Fault::new("setBreakpoints", fault.clone()).nth(nth))
            .collect(),
        ..spinning(script)
    });
    debug.toggle(1);
    debug.start();
    let fake = debug.fake();
    debug.until("the verification", |debug| {
        debug.breakpoints() == [(1, BreakpointState::Enabled)]
    });
    (debug, fake)
}

/// R2 F12: disabled and enabled again while the session's answer is held,
/// a breakpoint is a ring until the answer comes, not the old one's dot.
#[gpui::test]
fn an_enabled_again_breakpoint_waits_for_its_answer(cx: &mut TestAppContext) {
    let (mut debug, fake) = verified_then(cx, FaultKind::ReplyAfter(Trigger::Manual));
    let (store, path) = (debug.store(), debug.main.clone());
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, false, cx));
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, true, cx));
    debug.draw();
    let pending = BreakpointState::Unverified { message: None };
    assert_eq!(debug.breakpoints(), [(1, pending.clone())]);
    // Removed and added again: still waiting.
    debug.toggle(1);
    debug.toggle(1);
    assert_eq!(debug.breakpoints(), [(1, pending)]);
    fake.release();
    debug.until("the answer", |debug| {
        debug.breakpoints() == [(1, BreakpointState::Enabled)]
    });
}

/// R2 F12: a replacement the adapter refuses leaves the breakpoint a ring.
#[gpui::test]
fn a_refused_replacement_leaves_a_ring(cx: &mut TestAppContext) {
    let (mut debug, fake) = verified_then(cx, FaultKind::Fail("refused".into()));
    let (store, path) = (debug.store(), debug.main.clone());
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, false, cx));
    store.update(debug.cx, |store, cx| store.set_enabled(&path, 1, true, cx));
    debug.until("both refused", |_| {
        (fake.sent().iter())
            .filter(|m| matches!(m, Message::Response(r) if r.command == "setBreakpoints" && !r.success))
            .count()
            == 2
    });
    debug.draw();
    assert_eq!(
        debug.breakpoints(),
        [(1, BreakpointState::Unverified { message: None })]
    );
}

mod frames_after_save;
mod pending_verification;
