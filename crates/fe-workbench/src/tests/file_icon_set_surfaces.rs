//! Every surface that shows a file or folder icon asks the installed icon
//! set: installing a set, swapping it for another and resetting it each
//! redraw every surface already on screen with the set's icons, asset paths
//! included.

use std::{cell::RefCell, fs, path::Path, process::Command, rc::Rc, sync::Arc};

use fe_buffer::{Origin, ProposedChange};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::recent::{self, Entry};
use fe_editor::Changeset;
use fe_gpui::commands::RunCommand;
use fe_ui::{Icon, IconRef, IconSet, LucideIcons};
use gpui::{Bounds, Focusable as _, Pixels, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{pane_toolbar::ToolbarRequest, workbench::recent_picker::set_recent_file};

/// Answers every file with `file`, an open folder with `open` and a closed
/// one with `closed`.
#[derive(Clone)]
struct Stub {
    file: IconRef,
    open: IconRef,
    closed: IconRef,
}

impl IconSet for Stub {
    fn file(&self, _: &str) -> IconRef {
        self.file.clone()
    }

    fn folder(&self, expanded: bool) -> IconRef {
        match expanded {
            true => self.open.clone(),
            false => self.closed.clone(),
        }
    }
}

/// Lucide, then a stub of bundled icons, then one of asset paths, then
/// Lucide again by a reset: `None` installs nothing or resets.
fn sets() -> [Option<Stub>; 4] {
    let bundled = Stub {
        file: Icon::Diamond.into(),
        open: Icon::Box.into(),
        closed: Icon::Hash.into(),
    };
    let assets = Stub {
        file: IconRef::Asset("stub/file.svg".into()),
        open: IconRef::Asset("stub/open.svg".into()),
        closed: IconRef::Asset("stub/closed.svg".into()),
    };
    [None, Some(bundled), Some(assets), None]
}

/// What `set` answers for `a.rs` and the closed folder `src`.
fn answers(set: &Option<Stub>) -> (IconRef, IconRef) {
    match set {
        Some(stub) => (stub.file("a.rs"), stub.folder(false)),
        None => (LucideIcons.file("a.rs"), LucideIcons.folder(false)),
    }
}

/// Installs `set`, or resets, and lets what's on screen catch up, with no
/// draw of its own.
fn install(set: &Option<Stub>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| match set {
        Some(stub) => fe_ui::set_icon_set(Arc::new(stub.clone()), cx),
        None => fe_ui::reset_icon_set(cx),
    });
    cx.run_until_parked();
}

/// Installs each of [`sets`] in turn and checks the surface with `check`,
/// which gets the file's and the closed folder's answers.
fn through_every_set(
    cx: &mut VisualTestContext,
    check: impl Fn(&IconRef, &IconRef, &mut VisualTestContext),
) {
    for set in sets() {
        install(&set, cx);
        let (file, folder) = answers(&set);
        check(&file, &folder, cx);
    }
}

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn drawn(selector: String, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(selector.leak())
}

#[track_caller]
fn assert_drawn(selector: String, cx: &mut VisualTestContext) {
    assert!(
        drawn(selector.clone(), cx).is_some(),
        "nothing drawn as {selector}"
    );
}

#[gpui::test]
fn every_surface(cx: &mut TestAppContext) {
    // A repository whose `a.rs` is modified, with a folder beside it.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let a = root.join("a.rs");
    git(root, &["init", "-q"]);
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "\n").unwrap();
    fs::write(&a, "fn one() {}\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(&a, "fn two() {}\n").unwrap();
    let state = tempfile::tempdir().unwrap();
    let recent_file = state.path().join("recent.toml");
    recent::record(&recent_file, Entry::File(&a)).unwrap();

    let (workbench, cx) = open_root(cx, root);
    cx.update(|_, cx| set_recent_file(cx, recent_file));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);

    // The project tree.
    through_every_set(cx, |file, folder, cx| {
        assert_drawn(format!("tree-icon:a.rs:{}", file.name()), cx);
        assert_drawn(format!("tree-icon:src:{}", folder.name()), cx);
    });

    // A tab, and the breadcrumb's file crumb.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(a.clone(), window, cx)));
    draw(cx);
    through_every_set(cx, |file, _, cx| {
        assert_drawn(format!("tab-icon:a.rs:{}", file.name()), cx);
        assert_drawn(format!("crumb-icon:a.rs:{}", file.name()), cx);
    });

    // The toolbar picker, listing the root.
    pane.update(cx, |_, cx| {
        cx.emit(ToolbarRequest::Entries {
            dir: root.to_owned(),
            select: None,
        })
    });
    draw(cx);
    let listed = workbench.read_with(cx, |workbench, cx| {
        (workbench.toolbar_picker()).map(|picker| picker.read(cx).delegate.labels())
    });
    assert!(listed.is_some_and(|labels| labels.len() >= 2), "listed");
    through_every_set(cx, |file, folder, cx| {
        assert_drawn(format!("toolbar-picker-icon:a.rs:{}", file.path()), cx);
        assert_drawn(format!("toolbar-picker-icon:src:{}", folder.path()), cx);
    });

    // The recent picker.
    run("workbench.open_recent", cx);
    let labels = workbench.read_with(cx, |workbench, cx| {
        workbench.recent_picker().read(cx).delegate.labels()
    });
    assert!(!labels.is_empty(), "the recent list is shown");
    through_every_set(cx, |file, _, cx| {
        assert_drawn(format!("recent-icon:a.rs:{}", file.path()), cx);
    });
    cx.simulate_keystrokes("escape");
    draw(cx);

    // A Changes panel file row.
    run("git.show_changes", cx);
    through_every_set(cx, |file, _, cx| {
        assert_drawn(format!("change-icon:a.rs:{}", file.path()), cx);
    });

    // The AI changes panel.
    let ai = Origin::Ai {
        agent: "claude".into(),
        session: "s".into(),
    };
    let editor = workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .find(|editor| editor.read(cx).model().path() == Some(a.as_path()))
            .expect("a.rs is open")
    });
    let (buffer, proposal) = editor.read_with(cx, |editor, _| {
        let buffer = editor.model().buffer();
        let snapshot = buffer.snapshot();
        let proposal =
            ProposedChange::from_text(&snapshot, 0..snapshot.len(), "fn three() {}\n", ai.clone());
        (buffer.id(), proposal)
    });
    let mut changeset = Changeset::new("turn", ai);
    changeset.add_file(a.clone(), buffer, Rc::new(RefCell::new(proposal)));
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_changeset(changeset, window, cx)
    });
    draw(cx);
    through_every_set(cx, |file, _, cx| {
        assert_drawn(format!("ai-changes-icon:a.rs:{}", file.path()), cx);
    });
}
