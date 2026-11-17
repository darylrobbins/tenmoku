//! Show Memory Usage ([`crate::memory_report`]): the report tab, rows
//! counted once a buffer, a parse's tree, the process's totals, rows gone
//! with their tab, and the language servers' processes with rust-analyzer's
//! own breakdown and a Restart lens.

use std::{cell::RefCell, path::Path, rc::Rc};

use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_lsp::fake::{Reply, Scripted};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::PaneItem as _;
use serde_json::json;

use crate::memory_report::{MemoryReport, module};
use crate::tests::lsp::Lsp;
use crate::{Launch, Workbench};

const RUST: &str = "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n";

/// A workbench on a folder of `a.txt`, `b.txt` and `main.rs`, none open.
fn workbench(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<Workbench>, VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a".repeat(4096)).unwrap();
    std::fs::write(dir.path().join("b.txt"), "b".repeat(8192)).unwrap();
    std::fs::write(dir.path().join("main.rs"), RUST).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, _| window.activate_window());
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut vcx);
    (dir, workbench, vcx)
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

fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("an editor is shown")
}

fn report(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> MemoryReport {
    workbench.read_with(cx, MemoryReport::collect)
}

/// `module`'s row's bytes and count.
fn row(report: &MemoryReport, module: &str) -> (Option<i64>, Option<u64>) {
    let row = report.row(module).expect("the report has the row");
    (row.bytes, row.count)
}

#[gpui::test]
fn show_memory_usage_opens_a_read_only_report_tab(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = workbench(cx);
    let cx = &mut vcx;
    open(&workbench, &dir.path().join("a.txt"), cx);
    run("memory.show", cx);
    let editor = active_editor(&workbench, cx);
    let (read_only, title, text) = editor.read_with(cx, |editor, cx| {
        let text = editor.model().text().to_string();
        (editor.is_read_only(), editor.tab_title(cx), text)
    });
    assert!(read_only);
    assert_eq!(title, "Memory Usage");
    assert!(text.starts_with("Memory Usage\n"), "{text}");
    for label in [
        "Buffer text",
        "Undo history",
        "Syntax trees",
        "Unattributed",
    ] {
        assert!(text.contains(label), "no {label} row in:\n{text}");
    }
    // A fresh snapshot each time, in a tab of its own.
    run("memory.show", cx);
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len());
    assert_eq!(tabs, 3);
}

#[gpui::test]
fn a_buffer_in_two_editors_is_counted_once(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = workbench(cx);
    let cx = &mut vcx;
    open(&workbench, &dir.path().join("b.txt"), cx);
    let one = report(&workbench, cx);
    run("pane.split_right", cx);
    let editors: Vec<Entity<EditorView>> = workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .flat_map(|pane| pane.read(cx).tabs().to_vec())
            .filter_map(|tab| tab.editor().cloned())
            .collect()
    });
    assert_eq!(editors.len(), 2);
    let shared = editors[0].read_with(cx, |first, cx| {
        first
            .model()
            .shares_buffer_with(editors[1].read(cx).model())
    });
    assert!(shared, "the split shows the same buffer");
    let two = report(&workbench, cx);
    assert_eq!(
        row(&two, module::BUFFER_TEXT),
        row(&one, module::BUFFER_TEXT)
    );
    assert_eq!(row(&two, module::BUFFER_TEXT).1, Some(1));
    assert_eq!(row(&two, module::UNDO), row(&one, module::UNDO));
    let bytes = row(&two, module::BUFFER_TEXT).0.unwrap();
    assert!(bytes >= 8192, "{bytes} bytes for 8 KiB of text");
}

#[gpui::test]
fn a_parsed_file_reports_a_syntax_tree_row(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = workbench(cx);
    let cx = &mut vcx;
    open(&workbench, &dir.path().join("a.txt"), cx);
    assert_eq!(
        row(&report(&workbench, cx), module::SYNTAX_TREES),
        (Some(0), Some(0))
    );
    open(&workbench, &dir.path().join("main.rs"), cx);
    let editor = active_editor(&workbench, cx);
    for _ in 0..100 {
        if editor.read_with(cx, |editor, _| {
            editor.syntax().is_some_and(|s| s.tree().is_some())
        }) {
            break;
        }
        draw(cx);
    }
    let (bytes, nodes) = row(&report(&workbench, cx), module::SYNTAX_TREES);
    let nodes = nodes.expect("a node count");
    assert!(nodes > 10, "{nodes} nodes for main.rs");
    assert_eq!(
        bytes,
        Some((nodes as usize * fe_syntax::TREE_BYTES_PER_NODE) as i64)
    );
    assert!(
        report(&workbench, cx)
            .row(module::SYNTAX_TREES)
            .unwrap()
            .estimate
    );
}

#[gpui::test]
fn the_report_has_process_totals_and_an_unattributed_row(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = workbench(cx);
    let cx = &mut vcx;
    open(&workbench, &dir.path().join("a.txt"), cx);
    let report = report(&workbench, cx);
    let ids: Vec<&str> = report.rows.iter().map(|row| row.module).collect();
    assert_eq!(ids[..4], module::PROCESS);
    assert_eq!(ids.last(), Some(&module::UNATTRIBUTED));
    let process = report.process;
    assert_eq!(
        row(&report, module::RESIDENT).0,
        process.resident.map(|b| b as i64)
    );
    assert_eq!(
        row(&report, module::MALLOC).0,
        process.malloc_in_use.map(|b| b as i64)
    );
    if cfg!(any(target_os = "macos", target_os = "linux", windows)) {
        assert!(process.resident.is_some_and(|resident| resident > 0));
    }
    if cfg!(target_os = "macos") {
        assert!(process.footprint.is_some() && process.graphics.is_some());
    }
    if cfg!(any(
        target_os = "macos",
        all(target_os = "linux", target_env = "gnu")
    )) {
        let malloc = process.malloc_in_use.expect("malloc in use") as i64;
        let attributed: i64 = (report.rows.iter())
            .filter(|row| !module::PROCESS.contains(&row.module))
            .filter(|row| row.module != module::UNATTRIBUTED)
            .filter_map(|row| row.bytes)
            .sum();
        assert_eq!(
            row(&report, module::UNATTRIBUTED).0,
            Some(malloc - attributed)
        );
    }
    // A row the platform can't measure says so.
    let text = report.to_text();
    let minimap = (text.lines())
        .find(|line| line.starts_with("Minimap tiles"))
        .expect("a minimap row");
    assert!(minimap.contains("not measured"), "{minimap}");
    let first = text
        .lines()
        .position(|line| line.starts_with("Physical footprint"));
    let last = text
        .lines()
        .position(|line| line.starts_with("Unattributed"));
    assert!(first < last && first.is_some(), "{text}");
}

#[gpui::test]
fn a_closed_tab_drops_its_rows(cx: &mut TestAppContext) {
    let (dir, workbench, mut vcx) = workbench(cx);
    let cx = &mut vcx;
    open(&workbench, &dir.path().join("a.txt"), cx);
    let a = report(&workbench, cx);
    open(&workbench, &dir.path().join("b.txt"), cx);
    let both = report(&workbench, cx);
    assert_eq!(row(&both, module::BUFFER_TEXT).1, Some(2));
    run("workbench.close_tab", cx);
    let after = report(&workbench, cx);
    assert_eq!(
        row(&after, module::BUFFER_TEXT),
        row(&a, module::BUFFER_TEXT)
    );
    assert_eq!(row(&after, module::UNDO), row(&a, module::UNDO));
}

#[gpui::test]
fn language_servers_are_listed_with_their_processes_and_a_restart_lens(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    // The fake runs in this process: its memory is this test's, read by pid.
    let pid = std::process::id();
    s.fake.set_pid(Some(pid));
    s.fake.set_reply(
        "rust-analyzer/memoryUsage",
        Scripted::now(Reply::Result(json!("Database\n  12.5mb ItemTree\n"))),
    );
    s.open("src/main.rs");
    s.fake.wait_initialized();
    s.run("memory.show");
    s.until("the report", |s| {
        let editor = (s.workbench).read_with(&s.cx, |w, cx| w.active_editor(cx).cloned());
        editor.is_some_and(|editor| {
            editor.read_with(&s.cx, |editor, cx| editor.tab_title(cx) == "Memory Usage")
        })
    });
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    let text = editor.read_with(&s.cx, |editor, _| editor.model().text().to_string());
    let line = (text.lines())
        .position(|line| line.starts_with(&format!("rust-analyzer (pid {pid}): resident ")))
        .unwrap_or_else(|| panic!("no server row in:\n{text}"));
    let server = text.lines().nth(line).unwrap();
    if cfg!(any(target_os = "macos", target_os = "linux", windows)) {
        assert!(!server.contains("resident not measured"), "{server}");
    }
    assert!(server.contains("1 documents"), "{server}");
    assert!(text.contains("  rust-analyzer/memoryUsage:\n    Database\n      12.5mb ItemTree\n"));
    // A Restart lens over the server's row, naming it.
    let lenses = editor.read_with(&s.cx, |editor, _| {
        let snapshot = editor.model().snapshot().clone();
        (editor.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .collect::<Vec<_>>()
    });
    assert_eq!(lenses.len(), 1);
    assert_eq!(lenses[0].row as usize, line);
    let item = &lenses[0].items[0];
    let invocation = item.invocation.as_ref().expect("a click target");
    assert_eq!(item.title, "Restart");
    assert_eq!(invocation.command.as_str(), "lsp.restart");
    // Named by its instance and its manager, not its root, which needn't be
    // UTF-8.
    let server = (s.workbench)
        .read_with(&s.cx, MemoryReport::collect)
        .servers[0]
        .clone();
    assert_eq!(
        invocation.args,
        Value::Object(
            [
                ("server".to_owned(), Value::from("rust-analyzer")),
                ("instance".to_owned(), Value::from(server.instance)),
                ("generation".to_owned(), Value::from(server.generation)),
            ]
            .into_iter()
            .collect()
        )
    );
    // What `tenmoku --perf` prints, the same breakdown and all.
    let printed = Rc::new(RefCell::new(None));
    s.cx.update(|_, cx| {
        let report = MemoryReport::collect_with_breakdowns(s.workbench.read(cx), cx);
        let printed = printed.clone();
        cx.spawn(async move |_| *printed.borrow_mut() = Some(report.await.to_text()))
            .detach();
    });
    s.until("the perf report", |_| printed.borrow().is_some());
    let printed = printed.borrow_mut().take().unwrap();
    assert!(
        printed.contains("  rust-analyzer/memoryUsage:\n    Database\n"),
        "{printed}"
    );
    // Clicked, it restarts that server; clicked again, that instance has
    // gone, and nothing else restarts.
    let invocation = item.invocation.clone().expect("a click target");
    let click = |s: &mut Lsp| {
        let invocation = invocation.clone();
        s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    };
    click(&mut s);
    s.until("the restart", |s| s.fake.launches() == 2);
    s.fake.wait_initialized_at(2);
    click(&mut s);
    s.draw();
    assert_eq!(s.fake.launches(), 2);
}

/// The Restart lens of `s`'s report on its first server, once the report
/// tab shows.
fn report_restart(s: &mut Lsp) -> Invocation {
    s.run("memory.show");
    s.until("the report", |s| {
        let editor = (s.workbench).read_with(&s.cx, |w, cx| w.active_editor(cx).cloned());
        editor.is_some_and(|editor| {
            editor.read_with(&s.cx, |editor, cx| editor.tab_title(cx) == "Memory Usage")
        })
    });
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    editor.read_with(&s.cx, |editor, _| {
        let snapshot = editor.model().snapshot().clone();
        let lenses = (editor.lens_providers().iter())
            .flat_map(|provider| provider.borrow_mut().lenses(&snapshot))
            .collect::<Vec<_>>();
        lenses[0].items[0]
            .invocation
            .clone()
            .expect("a click target")
    })
}

#[gpui::test]
fn a_restart_lens_from_before_a_root_change_restarts_nothing(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    s.fake.wait_initialized();
    let restart = report_restart(&mut s);
    // The same folder again, by another spelling: a new manager, whose
    // first server is numbered as the old one was.
    let root = s.root.join("src/..");
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| workbench.update(cx, |w, cx| w.set_root(root, window, cx)));
    s.draw();
    s.set_trust(true);
    s.open("src/lib.rs");
    s.until("the new manager's server", |s| s.fake.launches() == 2);
    s.fake.wait_initialized_at(2);
    let now = (s.workbench)
        .read_with(&s.cx, MemoryReport::collect)
        .servers[0]
        .clone();
    assert_eq!(Some(now.instance), restart.args["instance"].as_u64());
    assert_ne!(Some(now.generation), restart.args["generation"].as_u64());
    s.cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(restart)), cx));
    s.draw();
    s.draw();
    assert_eq!(s.fake.launches(), 2, "the new root's server didn't restart");
}
