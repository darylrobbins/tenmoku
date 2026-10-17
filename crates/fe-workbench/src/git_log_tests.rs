//! The Git Log panel, listing HEAD's or every branch's commits with their
//! graph from `git.log`, `git.log_all` and `:Gclog`, its keys and its
//! pages, on repositories built with the `git` CLI in temporary
//! directories.

use std::{collections::HashMap, fs, io::Write as _, path::Path, process::Command, process::Stdio};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_git::graph::{GraphLayout, GraphRow};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use super::{GitLogPanel, PAGE};
use crate::{DockPanel, Launch, Workbench};

/// `git` in `dir`, with the developer's own config shut out.
fn git_command(dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    command
}

/// Runs `git` in `dir` and gives its output.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = git_command(dir, args).output().expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn commit(root: &Path, file: &str, text: &str, summary: &str) {
    fs::write(root.join(file), text).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", summary]);
}

/// A repository with a tag on its first commit, a topic branch merged into
/// `main`, and a side branch off `main` that isn't, `main` checked out one
/// commit past it.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    commit(root, "a.rs", "one\n", "init");
    git(root, &["tag", "v1"]);
    git(root, &["checkout", "-q", "-b", "topic"]);
    commit(root, "t.rs", "t\n", "topic work");
    git(root, &["checkout", "-q", "main"]);
    commit(root, "a.rs", "two\n", "main work");
    git(
        root,
        &["merge", "-q", "--no-ff", "topic", "-m", "merge topic"],
    );
    git(root, &["checkout", "-q", "-b", "side"]);
    commit(root, "s.rs", "s\n", "side work");
    git(root, &["checkout", "-q", "main"]);
    commit(root, "a.rs", "three\n", "after");
    dir
}

/// A repository of 250 commits on `main`, two of them on a branch merged
/// into it near the first page's end, made with `git fast-import`.
fn long_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let mut stream = String::new();
    let mut add = |branch: &str, mark: usize, from: Option<usize>, merge: Option<usize>| {
        let time = 1_700_000_000 + mark;
        stream += &format!("commit refs/heads/{branch}\nmark :{mark}\n");
        stream += &format!("committer t <t@t> {time} +0000\ndata 5\nc{mark:04}\n");
        if let Some(from) = from {
            stream += &format!("from :{from}\n");
        }
        if let Some(merge) = merge {
            stream += &format!("merge :{merge}\n");
        }
        stream += "\n";
    };
    // `main` from 1 to 248; `topic`, 1001 and 1002, off 40 and merged by 52.
    for mark in 1..=248 {
        match mark {
            41 => {
                add("topic", 1001, Some(40), None);
                add("topic", 1002, Some(1001), None);
                add("main", mark, Some(40), None);
            }
            52 => add("main", mark, Some(51), Some(1002)),
            _ => add("main", mark, (mark > 1).then(|| mark - 1), None),
        }
    }
    let mut child = git_command(root, &["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("run git fast-import");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stream.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    fs::write(root.join("c.rs"), "c\n").unwrap();
    dir
}

/// The log `git log` lists for `scope_args`, as the panel reads it: each
/// commit's id, parents and summary, in topo order.
fn cli_log(root: &Path, scope_args: &[&str]) -> Vec<(String, Vec<String>, String)> {
    let mut args = vec!["log", "--topo-order", "--format=%H%x1f%P%x1f%s"];
    args.extend(scope_args);
    args.extend(["HEAD", "--"]);
    let out = git(root, &args);
    out.lines()
        .map(|line| {
            let mut fields = line.split('\x1f');
            let id = fields.next().unwrap().to_owned();
            let parents = fields.next().unwrap();
            let parents = parents.split_whitespace().map(str::to_owned).collect();
            (id, parents, fields.next().unwrap().to_owned())
        })
        .collect()
}

/// `fe_git::graph`'s layout of `log`, each row with its commit's id.
fn layout(log: &[(String, Vec<String>, String)]) -> Vec<(String, GraphRow)> {
    let mut layout = GraphLayout::new();
    (log.iter())
        .map(|(id, parents, _)| (id.clone(), layout.push(id, parents)))
        .collect()
}

/// A window of a workbench rooted at `root`, focused, with `file` open and
/// vim on.
fn window(
    cx: &mut TestAppContext,
    root: &Path,
    file: &str,
) -> (Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    open(&workbench, &root.join(file), cx);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

/// Opens `path` in the active pane.
fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs workbench command `id` as an unbound key would, leaving what it
/// starts in the background to run.
fn send(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
}

fn dispatch(id: &str, cx: &mut VisualTestContext) {
    send(id, cx);
    draw(cx);
}

/// Types `:{line}` and Enter.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor is active")
}

fn focus_on(focus: &gpui::FocusHandle, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(focus, cx));
    draw(cx);
}

fn focused(focus: &gpui::FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| focus.contains_focused(window, cx))
}

/// `editor`'s vim message, emptied.
fn message(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            let message = editor.vim_state()?.message().map(str::to_owned);
            editor.set_vim_message("", cx);
            message.filter(|message| !message.is_empty())
        })
    })
}

/// What the window's shown toasts say, top first.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<GitLogPanel> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.dock().read(cx).git_log().cloned()).expect("the Git Log panel was made")
    })
}

/// Whether the dock shows, the panel it shows and its title row.
fn dock(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (bool, DockPanel, String) {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let title = dock.state().title(cx).unwrap_or_default().to_string();
        (dock.is_visible(), dock.active(), title)
    })
}

type Rows = Vec<(Vec<String>, Vec<String>)>;

fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Rows {
    panel(workbench, cx).read_with(cx, |panel, _| panel.rows())
}

fn graph(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, GraphRow)> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.graph())
}

fn selected(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<usize> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.selected())
}

fn panel_message(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<&'static str> {
    panel(workbench, cx).read_with(cx, |panel, _| panel.shown_message())
}

fn panel_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let panel = panel(workbench, cx);
    let focus = panel.read_with(cx, |panel, cx| panel.focus_handle(cx));
    focused(&focus, cx)
}

fn tab_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len())
}

/// The rows of `log`, each with the labels `labels` gives its summary.
fn expected(log: &[(String, Vec<String>, String)], labels: &[(&str, &str)]) -> Rows {
    let labels: HashMap<&str, &str> = labels.iter().copied().collect();
    (log.iter())
        .map(|(id, _, summary)| {
            let label = labels.get(summary.as_str());
            let label = label.map(|label| label.to_string()).into_iter().collect();
            let texts = vec![format!("{}  {summary}", &id[..7]), "t, just now".to_owned()];
            (label, texts)
        })
        .collect()
}

#[gpui::test]
fn lists(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, mut cx) = window(cx, root, "a.rs");
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    let editor_focus = cx.update(|_, cx| editor.focus_handle(cx));

    // HEAD's history.
    let head_log = cli_log(root, &[]);
    let summaries: Vec<&str> = head_log.iter().map(|(.., s)| s.as_str()).collect();
    assert_eq!(summaries.first(), Some(&"after"));
    assert_eq!(summaries.len(), 5);
    assert!(!summaries.contains(&"side work"));
    let labels = [
        ("after", "main"),
        ("topic work", "topic"),
        ("init", "tag v1"),
    ];
    let head_rows = expected(&head_log, &labels);
    let shown = (true, DockPanel::GitLog, "Git Log: main".to_owned());

    dispatch("git.log", cx);
    assert_eq!(dock(&workbench, cx), shown);
    assert_eq!(rows(&workbench, cx), head_rows);
    assert_eq!(graph(&workbench, cx), layout(&head_log));
    assert_eq!(panel_message(&workbench, cx), None);
    assert_eq!(selected(&workbench, cx), Some(0));
    assert!(panel_focused(&workbench, cx));
    // On the rail once it has shown.
    let on_rail = workbench.read_with(cx, |workbench, cx| {
        let state = workbench.dock().read(cx).state();
        state.on_rail(DockPanel::GitLog.id(), cx)
    });
    assert!(on_rail);

    // Every branch's.
    let all_log = cli_log(root, &["--branches", "--remotes", "--tags"]);
    assert_eq!(all_log.len(), 6);
    let labels = [labels.as_slice(), &[("side work", "side")]].concat();
    let all_rows = expected(&all_log, &labels);
    let all = (true, DockPanel::GitLog, "Git Log: All Branches".to_owned());
    dispatch("git.log_all", cx);
    assert_eq!(dock(&workbench, cx), all);
    assert_eq!(rows(&workbench, cx), all_rows);
    assert_eq!(graph(&workbench, cx), layout(&all_log));

    // Typed.
    for (line, shown, rows_shown, log) in [
        ("Gclog", &shown, &head_rows, &head_log),
        ("Gclog --all", &all, &all_rows, &all_log),
    ] {
        focus_on(&editor_focus, cx);
        dispatch("dock.toggle", cx);
        assert!(!dock(&workbench, cx).0, ":{line}");
        ex(line, cx);
        assert_eq!(&dock(&workbench, cx), shown, ":{line}");
        assert_eq!(&rows(&workbench, cx), rows_shown, ":{line}");
        assert_eq!(graph(&workbench, cx), layout(log), ":{line}");
        assert!(panel_focused(&workbench, cx), ":{line}");
        assert_eq!(message(&editor, cx), None, ":{line}");
    }

    for (line, answer) in [
        ("Gclog foo", "Not supported yet: :Gclog foo"),
        ("Gclog --all foo", "Not supported yet: :Gclog --all foo"),
        ("1Gclog --all", "Not supported yet: :1Gclog --all"),
    ] {
        focus_on(&editor_focus, cx);
        ex(line, cx);
        assert_eq!(message(&editor, cx).as_deref(), Some(answer), ":{line}");
    }

    // `:0Gclog` is still the file's history.
    focus_on(&editor_focus, cx);
    ex("0Gclog", cx);
    let history = (true, DockPanel::History, "History: a.rs".to_owned());
    assert_eq!(dock(&workbench, cx), history);
    assert_eq!(message(&editor, cx), None);

    // The palette lists both.
    focus_on(&editor_focus, cx);
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        assert_eq!(layer.modal(), Some(COMMAND_PALETTE));
        let view = layer.modal_view().unwrap().clone();
        view.downcast::<Picker<CommandPalette>>().unwrap()
    });
    for (id, title) in [
        ("git.log", "Git: Log"),
        ("git.log_all", "Git: Log (All Branches)"),
    ] {
        let listed = picker.read_with(cx, |picker, _| {
            (picker.delegate.matches().iter())
                .any(|entry| entry.id.as_str() == id && entry.title == title)
        });
        assert!(listed, "{id}");
    }
}

#[gpui::test]
fn keys_and_pages(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, mut cx) = window(cx, dir.path(), "a.rs");
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    let editor_focus = cx.update(|_, cx| editor.focus_handle(cx));
    dispatch("git.log", cx);
    assert!(panel_focused(&workbench, cx));

    // With vim on.
    for (keystrokes, row) in [
        ("j", 1),
        ("j j", 3),
        ("k", 2),
        ("g g", 0),
        ("shift-g", 4),
        ("g k", 3),
        ("g g", 0),
        // Enter and `o` only select.
        ("j enter", 1),
        ("o", 1),
    ] {
        press(keystrokes, cx);
        assert_eq!(selected(&workbench, cx), Some(row), "{keystrokes}");
    }
    assert_eq!(tab_count(&workbench, cx), 1);
    press("q", cx);
    assert!(!dock(&workbench, cx).0);
    assert!(focused(&editor_focus, cx));

    // With vim off, the letters do nothing.
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, false));
    focus_on(&editor_focus, cx);
    dispatch("git.log", cx);
    assert!(panel_focused(&workbench, cx));
    press("j shift-g o q", cx);
    assert_eq!(selected(&workbench, cx), Some(0));
    assert!(dock(&workbench, cx).0);
    assert!(panel_focused(&workbench, cx));
    press("down down", cx);
    assert_eq!(selected(&workbench, cx), Some(2));
    press("up", cx);
    assert_eq!(selected(&workbench, cx), Some(1));
    press("enter", cx);
    assert_eq!(selected(&workbench, cx), Some(1));
    assert_eq!(tab_count(&workbench, cx), 1);

    // Pages: the first 200, then the next once near the end, laid out as
    // one graph.
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    let long = long_fixture();
    let log = cli_log(long.path(), &[]);
    assert_eq!(log.len(), 250);
    let whole = layout(&log);
    // The topic branch's lane is open across the pages' boundary.
    assert_eq!(
        whole[PAGE].1.top.len(),
        2,
        "{:?}",
        &whole[PAGE - 4..PAGE + 4]
    );
    open(&workbench, &long.path().join("c.rs"), cx);
    dispatch("git.log", cx);
    assert_eq!(rows(&workbench, cx).len(), PAGE);
    assert_eq!(graph(&workbench, cx), whole[..PAGE]);
    press(&["j"; 150].join(" "), cx);
    assert_eq!(selected(&workbench, cx), Some(150));
    assert_eq!(rows(&workbench, cx).len(), PAGE);
    // A commit and a reset meanwhile don't shift the next page: it's read
    // from the commits the first was.
    let main = git(long.path(), &["rev-parse", "main"]);
    let main = main.trim();
    git(
        long.path(),
        &["commit", "-q", "--allow-empty", "-m", "between"],
    );
    git(long.path(), &["reset", "-q", "--soft", "HEAD~3"]);
    press(&["j"; 30].join(" "), cx);
    assert_eq!(selected(&workbench, cx), Some(180));
    assert_eq!(rows(&workbench, cx).len(), 250);
    assert_eq!(graph(&workbench, cx), whole);
    let shown = labels_and_summaries(rows(&workbench, cx));
    // The first page's labels, as they were when it was read.
    assert_eq!(shown, expected_long(&log));
    git(long.path(), &["update-ref", "refs/heads/main", main]);

    // `G` goes to the last row loaded and reads no further page; moving
    // from it does.
    dispatch("git.log", cx);
    assert_eq!(rows(&workbench, cx).len(), PAGE);
    press("shift-g", cx);
    draw(cx);
    assert_eq!(selected(&workbench, cx), Some(PAGE - 1));
    assert_eq!(rows(&workbench, cx).len(), PAGE);
    press("k", cx);
    assert_eq!(selected(&workbench, cx), Some(PAGE - 2));
    assert_eq!(rows(&workbench, cx).len(), 250);
    assert_eq!(graph(&workbench, cx), whole);
    // A short page was the last.
    press("shift-g", cx);
    assert_eq!(selected(&workbench, cx), Some(249));
    assert_eq!(rows(&workbench, cx).len(), 250);
    press("g g", cx);
    assert_eq!(selected(&workbench, cx), Some(0));

    // The wheel after `G` reads it, even at the end, where nothing scrolls.
    dispatch("git.log", cx);
    press("shift-g", cx);
    assert_eq!(rows(&workbench, cx).len(), PAGE);
    let column = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).column_bounds()
    });
    let position = column.expect("the dock showed").center();
    cx.simulate_event(gpui::ScrollWheelEvent {
        position,
        delta: gpui::ScrollDelta::Lines(gpui::point(0., -3.)),
        ..Default::default()
    });
    draw(cx);
    assert_eq!(rows(&workbench, cx).len(), 250);
    assert_eq!(selected(&workbench, cx), Some(PAGE - 1));

    // So does scrolling to other rows and back.
    dispatch("git.log", cx);
    press("shift-g", cx);
    assert_eq!(rows(&workbench, cx).len(), PAGE);
    let panel = panel(&workbench, cx);
    let scroll_to = |ix: usize, cx: &mut VisualTestContext| {
        let scroll = panel.read_with(cx, |panel, _| panel.scroll.clone());
        scroll.scroll_to_item(ix, gpui::ScrollStrategy::Top);
        draw(cx);
    };
    scroll_to(100, cx);
    assert_eq!(rows(&workbench, cx).len(), PAGE);
    scroll_to(PAGE - 1, cx);
    assert_eq!(rows(&workbench, cx).len(), 250);
    assert_eq!(selected(&workbench, cx), Some(PAGE - 1));
}

/// Each row's labels and first text, without how long ago it was.
fn labels_and_summaries(rows: Rows) -> Vec<(Vec<String>, String)> {
    (rows.into_iter())
        .map(|(labels, texts)| (labels, texts[0].clone()))
        .collect()
}

/// The long fixture's rows' labels and first texts: `main` on the newest
/// and `topic` on its last commit.
fn expected_long(log: &[(String, Vec<String>, String)]) -> Vec<(Vec<String>, String)> {
    (log.iter())
        .enumerate()
        .map(|(ix, (id, _, summary))| {
            let label = match summary.as_str() {
                _ if ix == 0 => Some("main"),
                "c1002" => Some("topic"),
                _ => None,
            };
            let labels = label.map(str::to_owned).into_iter().collect();
            (labels, format!("{}  {summary}", &id[..7]))
        })
        .collect()
}

#[gpui::test]
fn edges(cx: &mut TestAppContext) {
    // Outside a repository: answered on the vim line, or in a toast.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("c.rs"), "c\n").unwrap();
    let (workbench, mut cx) = window(cx, dir.path(), "c.rs");
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    ex("Gclog", cx);
    assert_eq!(
        message(&editor, cx).as_deref(),
        Some("Not in a git repository")
    );
    let not_repo = Some("Not in a git repository");
    assert_eq!(panel_message(&workbench, cx), not_repo);
    assert_eq!(rows(&workbench, cx), Rows::new());
    dispatch("git.log", cx);
    assert_eq!(toasts(&workbench, cx), ["Not in a git repository"]);

    // An unborn HEAD.
    let unborn = tempfile::tempdir().unwrap();
    git(unborn.path(), &["init", "-q"]);
    fs::write(unborn.path().join("c.rs"), "c\n").unwrap();
    open(&workbench, &unborn.path().join("c.rs"), cx);
    dispatch("git.log", cx);
    let shown = (true, DockPanel::GitLog, "Git Log: main".to_owned());
    assert_eq!(dock(&workbench, cx), shown);
    assert_eq!(panel_message(&workbench, cx), Some("No commits yet"));
    assert_eq!(rows(&workbench, cx), Rows::new());

    // The log is read in the background: a last row says so until it
    // lands.
    let repo = fixture();
    open(&workbench, &repo.path().join("a.rs"), cx);
    send("git.log", cx);
    let loading = vec![(Vec::new(), vec!["Loading…".to_owned()])];
    assert_eq!(rows(&workbench, cx), loading);
    assert_eq!(panel_message(&workbench, cx), None);
    assert_eq!(dock(&workbench, cx).2, "Git Log");
    draw(cx);
    assert_eq!(rows(&workbench, cx).len(), 5);
    assert_eq!(panel_message(&workbench, cx), None);
    assert_eq!(dock(&workbench, cx), shown);

    // A detached HEAD.
    git(repo.path(), &["checkout", "-q", "--detach", "HEAD~1"]);
    dispatch("git.log", cx);
    let detached = (
        true,
        DockPanel::GitLog,
        "Git Log: HEAD (detached)".to_owned(),
    );
    assert_eq!(dock(&workbench, cx), detached);
    let first = rows(&workbench, cx).into_iter().next().unwrap();
    assert_eq!(first.0, ["HEAD"]);
    assert!(first.1[0].ends_with("  merge topic"), "{first:?}");
}
