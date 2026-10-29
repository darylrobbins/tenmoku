//! Cut, copy and paste in the project panel, on a temporary folder: a copy
//! pasted in a folder or beside a file, names that clash, a cut moving its
//! entry with its editors following, a cut refused inside itself, the cut
//! row's mark, the name field's own clipboard keys, and language servers
//! asked about a cut as about a rename.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use fe_gpui::{EditorView, set_vim_enabled};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted};
use fe_project::FsChange;
use gpui::{
    AppContext as _, ClipboardItem, Entity, Focusable as _, TestAppContext, VisualTestContext,
};
use serde_json::json;

use crate::{ProjectPanel, Workbench, tests::lsp::Lsp};

/// Files at `paths` (relative) with their text, in a temporary directory.
fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

/// Every entry under `root`, relative, `/`-separated, sorted; folders end
/// in `/`, files have their text.
fn snapshot(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = fe_project::relative_path(root, &path);
            if path.is_dir() {
                out.push(format!("{rel}/"));
                walk(root, &path, out);
            } else {
                out.push(format!("{rel} = {}", fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// A workbench rooted at `root`, vim on if `vim`, in an active window.
fn open_root<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    vim: bool,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = crate::tests::open_root(cx, root);
    cx.update(|window, cx| {
        window.activate_window();
        set_vim_enabled(cx, vim);
    });
    cx.run_until_parked();
    (workbench, cx)
}

/// Focuses the workbench, as the app does, then the panel through
/// `dock.show_project`.
fn focus_panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ProjectPanel> {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-shift-e");
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    assert!(
        cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)),
        "dock.show_project focuses the panel"
    );
    panel
}

/// Presses `keys`, then runs the executor.
fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

/// The panel's rows as `(depth, name)`.
fn rows(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
    panel.read_with(cx, |panel, _| {
        let tree = panel.tree().expect("the root is open");
        tree.rows(0..tree.row_count())
            .map(|row| (row.depth, row.name.to_string_lossy().into_owned()))
            .collect()
    })
}

/// The selected row's name.
fn selected(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<String> {
    let ix = panel.read_with(cx, |panel, _| panel.selected_row())?;
    Some(rows(panel, cx)[ix].1.clone())
}

/// Selects the entry at `rel`, expanding the folders above it.
fn select(panel: &Entity<ProjectPanel>, root: &Path, rel: &str, cx: &mut VisualTestContext) {
    panel.update(cx, |panel, cx| panel.reveal(root.join(rel), cx));
    cx.run_until_parked();
    let name = rel.rsplit('/').next().unwrap();
    assert_eq!(
        selected(panel, cx).as_deref(),
        Some(name),
        "{:?}",
        rows(panel, cx)
    );
}

/// The entry to paste: whether it was cut, and its path from the root.
fn clipboard(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Option<(bool, PathBuf)> {
    panel.read_with(cx, |panel, _| panel.clipboard_entry())
}

/// The rows the panel drew dimmed as cut, once it's drawn again, each
/// once (the list builds its first row twice, to measure it).
fn drawn_cut(panel: &Entity<ProjectPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    panel.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let mut drawn = panel.read_with(cx, |panel, _| panel.drawn_cut());
    drawn.dedup();
    drawn
}

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    cx.run_until_parked();
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor is open")
}

fn editor_path(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
}

#[gpui::test]
fn copy_paste(cx: &mut TestAppContext) {
    let dir = project(&[
        ("a.txt", "a"),
        ("b.txt", "mine"),
        ("docs/b.txt", "theirs"),
        ("src/lib.rs", "lib"),
    ]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root, false);
    let panel = focus_panel(&workbench, cx);

    // VS Code's keys, with vim off: copied on a file, pasted on a folder.
    select(&panel, root, "a.txt", cx);
    press("cmd-c", cx);
    assert_eq!(clipboard(&panel, cx), Some((false, "a.txt".into())));
    select(&panel, root, "docs", cx);
    press("cmd-v", cx);
    assert_eq!(fs::read_to_string(root.join("docs/a.txt")).unwrap(), "a");
    assert_eq!(selected(&panel, cx).as_deref(), Some("a.txt"), "the copy");
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.selected_row()),
        rows(&panel, cx)
            .iter()
            .position(|(depth, name)| (*depth, name.as_str()) == (1, "a.txt")),
        "docs/a.txt is selected"
    );

    // On a file, beside it: a clash names a copy, then copy 2. The copy
    // can be pasted again and again.
    press("cmd-v", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("a copy.txt"));
    press("cmd-v", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("a copy 2.txt"));
    assert_eq!(clipboard(&panel, cx), Some((false, "a.txt".into())));

    // A clash never overwrites.
    select(&panel, root, "b.txt", cx);
    press("cmd-c", cx);
    select(&panel, root, "docs", cx);
    press("cmd-v", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("b copy.txt"));

    // A folder, with what's in it; pasted with its own row selected, beside
    // itself.
    select(&panel, root, "src", cx);
    press("cmd-c", cx);
    press("cmd-v", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("src copy"));
    select(&panel, root, "docs", cx);
    press("cmd-v", cx);

    assert_eq!(
        snapshot(root),
        [
            "a.txt = a",
            "b.txt = mine",
            "docs/",
            "docs/a copy 2.txt = a",
            "docs/a copy.txt = a",
            "docs/a.txt = a",
            "docs/b copy.txt = mine",
            "docs/b.txt = theirs",
            "docs/src/",
            "docs/src/lib.rs = lib",
            "src copy/",
            "src copy/lib.rs = lib",
            "src/",
            "src/lib.rs = lib",
        ]
    );
    assert_eq!(toasts(&workbench, cx), Vec::<String>::new());

    // vim's `c` and `p` do the same.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    select(&panel, root, "src/lib.rs", cx);
    press("c", cx);
    select(&panel, root, "docs", cx);
    press("p", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("lib.rs"));
    assert_eq!(fs::read_to_string(root.join("docs/lib.rs")).unwrap(), "lib");
}

#[gpui::test]
fn cut_paste(cx: &mut TestAppContext) {
    let dir = project(&[
        ("app/main.rs", "fn main() {}\n"),
        ("app/util/mod.rs", "mod util;\n"),
        ("lib/keep.rs", ""),
        ("lib/notes.txt", "theirs"),
        ("notes.txt", "mine"),
    ]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root, true);
    open_file(&workbench, &root.join("app/main.rs"), cx);
    let main = active_editor(&workbench, cx);
    open_file(&workbench, &root.join("app/util/mod.rs"), cx);
    let util = active_editor(&workbench, cx);
    let panel = focus_panel(&workbench, cx);

    // `x` then `p` on a folder: the file moves, its editor with it.
    select(&panel, root, "app/main.rs", cx);
    press("x", cx);
    assert_eq!(clipboard(&panel, cx), Some((true, "app/main.rs".into())));
    select(&panel, root, "lib", cx);
    press("p", cx);
    assert!(!root.join("app/main.rs").exists());
    assert_eq!(
        fs::read_to_string(root.join("lib/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    assert_eq!(editor_path(&main, cx), Some(root.join("lib/main.rs")));
    assert_eq!(selected(&panel, cx).as_deref(), Some("main.rs"));
    // Nothing is left to paste.
    assert_eq!(clipboard(&panel, cx), None);
    press("p", cx);
    assert!(root.join("lib/main.rs").exists());

    // A name that's taken gets a copy's, and nothing is overwritten.
    select(&panel, root, "notes.txt", cx);
    press("x", cx);
    select(&panel, root, "lib/keep.rs", cx);
    press("p", cx);
    assert_eq!(selected(&panel, cx).as_deref(), Some("notes copy.txt"));

    // A folder moves with what's in it, and its editors follow.
    select(&panel, root, "app", cx);
    press("x", cx);
    select(&panel, root, "lib", cx);
    press("p", cx);
    assert_eq!(
        editor_path(&util, cx),
        Some(root.join("lib/app/util/mod.rs"))
    );
    assert_eq!(
        util.read_with(cx, |util, _| util.model().buffer().text().to_string()),
        "mod util;\n"
    );

    // Not into its own subtree: a toast says why, and nothing moves.
    let before = snapshot(root);
    select(&panel, root, "lib", cx);
    press("x", cx);
    for inside in ["lib", "lib/app/util", "lib/app/util/mod.rs"] {
        select(&panel, root, inside, cx);
        press("p", cx);
        assert_eq!(snapshot(root), before, "{inside}");
        assert_eq!(
            clipboard(&panel, cx),
            Some((true, "lib".into())),
            "{inside}"
        );
        assert_eq!(
            toasts(&workbench, cx),
            ["“lib” can’t be moved into itself or a folder inside it."],
            "{inside}"
        );
        let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
        layer.update(cx, |layer, cx| layer.dismiss_all(cx));
    }

    assert_eq!(
        before,
        [
            "app/",
            "lib/",
            "lib/app/",
            "lib/app/util/",
            "lib/app/util/mod.rs = mod util;\n",
            "lib/keep.rs = ",
            "lib/main.rs = fn main() {}\n",
            "lib/notes copy.txt = mine",
            "lib/notes.txt = theirs",
        ]
        .into_iter()
        .filter(|entry| *entry != "app/")
        .collect::<Vec<_>>()
    );
    // VS Code's keys cut and paste too.
    select(&panel, root, "lib/main.rs", cx);
    press("cmd-x", cx);
    select(&panel, root, "lib/app", cx);
    press("cmd-v", cx);
    assert_eq!(editor_path(&main, cx), Some(root.join("lib/app/main.rs")));
}

#[gpui::test]
fn marks(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", ""), ("b.txt", ""), ("d/e.txt", "")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root, true);
    let panel = focus_panel(&workbench, cx);

    // A cut row is dimmed; a copied one isn't.
    select(&panel, root, "a.txt", cx);
    press("x", cx);
    assert_eq!(drawn_cut(&panel, cx), ["a.txt"]);
    select(&panel, root, "b.txt", cx);
    press("c", cx);
    assert_eq!(drawn_cut(&panel, cx), Vec::<String>::new());
    assert_eq!(clipboard(&panel, cx), Some((false, "b.txt".into())));
    // Another cut replaces the one before.
    select(&panel, root, "a.txt", cx);
    press("x", cx);
    select(&panel, root, "b.txt", cx);
    press("x", cx);
    assert_eq!(drawn_cut(&panel, cx), ["b.txt"]);

    // It survives a refresh, and the tree opened again.
    press("g r", cx);
    assert_eq!(drawn_cut(&panel, cx), ["b.txt"]);
    press("shift-h", cx);
    assert_eq!(drawn_cut(&panel, cx), ["b.txt"]);
    assert_eq!(clipboard(&panel, cx), Some((true, "b.txt".into())));

    // Until it's pasted.
    select(&panel, root, "d", cx);
    press("p", cx);
    assert!(root.join("d/b.txt").exists());
    assert_eq!(drawn_cut(&panel, cx), Vec::<String>::new());

    // Deleted, it's dropped, cut or copied.
    for key in ["x", "c"] {
        select(&panel, root, "a.txt", cx);
        press(key, cx);
        fs::remove_file(root.join("a.txt")).unwrap();
        panel.update(cx, |panel, cx| {
            panel.apply_fs_changes(&[FsChange::Path(root.join("a.txt"))], cx)
        });
        cx.run_until_parked();
        assert_eq!(clipboard(&panel, cx), None, "{key}");
        assert_eq!(drawn_cut(&panel, cx), Vec::<String>::new(), "{key}");
        fs::write(root.join("a.txt"), "").unwrap();
        panel.update(cx, |panel, cx| {
            panel.apply_fs_changes(&[FsChange::Path(root.join("a.txt"))], cx)
        });
        cx.run_until_parked();
        // Back again, it's another file: nothing to paste.
        assert_eq!(clipboard(&panel, cx), None, "{key}");
    }
    // A path inside a folder deleted goes with it.
    select(&panel, root, "d/e.txt", cx);
    press("x", cx);
    fs::remove_dir_all(root.join("d")).unwrap();
    panel.update(cx, |panel, cx| {
        panel.apply_fs_changes(&[FsChange::Path(root.join("d"))], cx)
    });
    cx.run_until_parked();
    assert_eq!(clipboard(&panel, cx), None);
}

#[gpui::test]
fn the_name_field_keeps_its_clipboard_keys(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root, false);
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "a.txt", cx);
    press("cmd-c", cx);

    // In the rename field, cmd-x and -v are the field's: the stem
    // selected is cut, and the text pasted in its place.
    press("f2", cx);
    press("cmd-x", cx);
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("a")
    );
    let field = panel.read_with(cx, |panel, cx| panel.name_field(cx));
    assert_eq!(field.map(|(text, _)| text).as_deref(), Some(".txt"));
    cx.write_to_clipboard(ClipboardItem::new_string("b".into()));
    press("cmd-v", cx);
    let field = panel.read_with(cx, |panel, cx| panel.name_field(cx));
    assert_eq!(field.map(|(text, _)| text).as_deref(), Some("b.txt"));
    press("enter", cx);
    assert_eq!(snapshot(root), ["b.txt = "]);
    // The panel's entry to paste was the copied one, gone with the rename.
    assert_eq!(clipboard(&panel, cx), None);
}

/// A rust-analyzer fake asked for its edits before a `*.rs` file's rename,
/// and told of it after.
fn participant() -> Script {
    participant_for("**/*.rs")
}

/// [`participant`], for the files and folders `glob` matches.
fn participant_for(glob: &str) -> Script {
    let mut result = fe_lsp::fake::initialize_result(None);
    let filters = json!({"filters": [{"scheme": "file", "pattern": {"glob": glob}}]});
    result.capabilities.workspace = Some(
        serde_json::from_value(json!({"fileOperations": {
            "didRename": filters, "willRename": filters,
        }}))
        .unwrap(),
    );
    Script {
        initialize: InitializeReply::Result(Box::new(result)),
        ..Script::default()
    }
}

/// `src/main.rs`, open and focused, which declares `src/util.rs`; an empty
/// `src/sub`. The fake answers moving `util.rs` after `delay` with `mod
/// util;` made `mod sub;` in `main.rs`, and `h` made `g` in `util.rs`
/// itself. `main.rs`'s editor.
fn modules(s: &mut Lsp, delay: Duration) -> Entity<EditorView> {
    let replace = |start: u32, end: u32, text: &str| {
        json!([{"range": {"start": {"line": 0, "character": start},
            "end": {"line": 0, "character": end}}, "newText": text}])
    };
    let reply = Reply::Result(json!({"changes": {
        s.uri("src/main.rs").as_str(): replace(4, 8, "sub"),
        s.uri("src/util.rs").as_str(): replace(7, 8, "g"),
    }}));
    s.fake
        .set_reply("workspace/willRenameFiles", Scripted { reply, delay });
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), MAIN).unwrap();
    fs::write(s.path("src/util.rs"), UTIL).unwrap();
    fs::create_dir(s.path("src/sub")).unwrap();
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let changes = [
        FsChange::Path(s.path("src/util.rs")),
        FsChange::Path(s.path("src/sub")),
    ];
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx)));
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap()
}

const MAIN: &str = "mod util;\nfn main() {}\n";
const UTIL: &str = "pub fn h() {}\n";

/// Cuts `src/util.rs` and pastes it in `src/sub`, with the tree's keys.
fn cut_and_paste(s: &mut Lsp) {
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let workbench = s.workbench.clone();
    for (rel, keys) in [("src/util.rs", "cmd-x"), ("src/sub", "cmd-v")] {
        s.cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        s.cx.simulate_keystrokes("cmd-shift-e");
        let path = s.path(rel);
        s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(path, cx)));
        s.draw();
        s.cx.simulate_keystrokes(keys);
        s.draw();
    }
}

/// The params of the fake's first `method`, request or notification.
fn first(s: &Lsp, method: &str) -> Option<serde_json::Value> {
    (s.fake.records().into_iter()).find_map(|record| match record {
        Record::Request {
            method: m, params, ..
        }
        | Record::Notification { method: m, params }
            if m == method =>
        {
            Some(params)
        }
        _ => None,
    })
}

fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn lsp_toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

#[gpui::test]
fn a_cut_asks_language_servers_as_a_rename_does(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    let editor = modules(&mut s, Duration::ZERO);
    cut_and_paste(&mut s);
    s.until("the move", |s| s.path("src/sub/util.rs").is_file());
    assert!(!s.path("src/util.rs").exists());
    // The edits are made once it's moved: `util.rs`'s at its new path.
    s.until("the edits", |s| {
        text(s, &editor) == "mod sub;\nfn main() {}\n"
    });
    assert_eq!(
        fs::read_to_string(s.path("src/sub/util.rs")).unwrap(),
        "pub fn g() {}\n"
    );
    let (old, new) = (s.uri("src/util.rs"), s.uri("src/sub/util.rs"));
    let files = json!({"files": [{"oldUri": old.as_str(), "newUri": new.as_str()}]});
    assert_eq!(first(&s, "workspace/willRenameFiles"), Some(files.clone()));
    s.fake.wait_for("didRenameFiles", |records| {
        (records.iter()).any(|record| record.is_notification("workspace/didRenameFiles"))
    });
    assert_eq!(first(&s, "workspace/didRenameFiles"), Some(files));
}

/// Holds each save until the test ends.
struct HoldSaves;

impl fe_gpui::pane_item::SaveHook for HoldSaves {
    fn before_save(&self, _: &Entity<EditorView>, cx: &mut gpui::App) -> gpui::Task<()> {
        cx.background_spawn(std::future::pending())
    }
}

#[gpui::test]
fn a_cut_checks_again_once_servers_answer(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, participant(), Script::default());
    let editor = modules(&mut s, Duration::from_secs(5));

    // Its new path taken while the servers are asked: nothing moves, and
    // none of their edits are made.
    cut_and_paste(&mut s);
    s.fake.wait_for("willRenameFiles", |records| {
        (records.iter()).any(|record| record.method() == "workspace/willRenameFiles")
    });
    fs::write(s.path("src/sub/util.rs"), "theirs").unwrap();
    s.clock.wait_for_sleep(Duration::from_secs(5));
    s.clock.advance(Duration::from_secs(5));
    s.draw();
    s.until("the toast", |s| !lsp_toasts(s).is_empty());
    assert_eq!(lsp_toasts(&s), ["“util.rs” couldn’t be moved."]);
    assert_eq!(fs::read_to_string(s.path("src/util.rs")).unwrap(), UTIL);
    assert_eq!(
        fs::read_to_string(s.path("src/sub/util.rs")).unwrap(),
        "theirs"
    );
    s.draw();
    assert_eq!(text(&s, &editor), MAIN);
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.update(&mut s.cx, |layer, cx| layer.dismiss_all(cx));
    fs::remove_file(s.path("src/sub/util.rs")).unwrap();

    // Its file being saved once they answer: the same.
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let gone = [FsChange::Path(s.path("src/sub/util.rs"))];
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&gone, cx)));
    let asked = |records: &[Record]| {
        (records.iter())
            .filter(|record| {
                matches!(record, Record::Request { method, .. } if method == "workspace/willRenameFiles")
            })
            .count()
    };
    cut_and_paste(&mut s);
    s.fake
        .wait_for("willRenameFiles", move |records| asked(records) == 2);
    s.open("src/util.rs");
    let util = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    let saving = s.cx.update(|window, cx| {
        util.update(cx, |util, cx| {
            util.set_save_hook(Some(std::rc::Rc::new(HoldSaves)));
            util.save(window, cx)
        })
    });
    s.draw();
    s.clock.wait_for_sleep(Duration::from_secs(5));
    s.clock.advance(Duration::from_secs(5));
    s.draw();
    s.until("the toast", |s| !lsp_toasts(s).is_empty());
    assert_eq!(
        lsp_toasts(&s),
        ["“util.rs” is being saved. Paste it once it’s saved."]
    );
    assert!(s.path("src/util.rs").is_file());
    assert!(!s.path("src/sub/util.rs").exists());
    assert_eq!(text(&s, &editor), MAIN);
    assert_eq!(text(&s, &util), UTIL);
    drop(saving);
}

#[gpui::test]
fn the_entry_to_paste_outlives_being_ignored(cx: &mut TestAppContext) {
    let dir = project(&[(".gitignore", ""), ("a.log", "a"), ("d/e.txt", "")]);
    let root = dir.path();
    fs::create_dir(root.join(".git")).unwrap();
    let (workbench, cx) = open_root(cx, root, true);
    let panel = focus_panel(&workbench, cx);
    assert!(!panel.read_with(cx, |panel, _| panel.lists_ignored()));

    // Copied, then ignored and hidden on a refresh: it's still there to
    // paste.
    select(&panel, root, "a.log", cx);
    press("c", cx);
    fs::write(root.join(".gitignore"), "a.log\n").unwrap();
    press("g r", cx);
    let names: Vec<String> = rows(&panel, cx).into_iter().map(|(_, name)| name).collect();
    assert!(!names.contains(&"a.log".to_owned()), "{names:?}");
    assert_eq!(clipboard(&panel, cx), Some((false, "a.log".into())));
    // And with ignored entries shown, then hidden again.
    for _ in 0..2 {
        press("shift-h", cx);
        assert_eq!(clipboard(&panel, cx), Some((false, "a.log".into())));
    }
    assert!(!panel.read_with(cx, |panel, _| panel.lists_ignored()));
    select(&panel, root, "d", cx);
    press("p", cx);
    assert_eq!(fs::read_to_string(root.join("d/a.log")).unwrap(), "a");

    // Deleted, hidden as it is, it's dropped.
    fs::remove_file(root.join("a.log")).unwrap();
    panel.update(cx, |panel, cx| {
        panel.apply_fs_changes(&[FsChange::Path(root.join("a.log"))], cx)
    });
    cx.run_until_parked();
    assert_eq!(clipboard(&panel, cx), None);
}

#[gpui::test]
fn a_name_that_isnt_utf8_is_cut_and_pasted(cx: &mut TestAppContext) {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt as _};

    let dir = project(&[("d/e.txt", "")]);
    let root = dir.path();
    let name = OsStr::from_bytes(b"caf\xe9.txt");
    if fs::write(root.join(name), "x").is_err() {
        eprintln!("skipped: {} takes only UTF-8 names", root.display());
        return;
    }
    let (workbench, cx) = open_root(cx, root, true);
    let panel = focus_panel(&workbench, cx);
    panel.update(cx, |panel, cx| panel.reveal(root.join(name), cx));
    cx.run_until_parked();
    assert_eq!(selected(&panel, cx).as_deref(), Some("caf\u{fffd}.txt"));
    press("x", cx);
    select(&panel, root, "d", cx);
    press("p", cx);
    assert_eq!(fs::read_to_string(root.join("d").join(name)).unwrap(), "x");
    assert!(!root.join(name).exists());
    assert_eq!(selected(&panel, cx).as_deref(), Some("caf\u{fffd}.txt"));
    assert_eq!(toasts(&workbench, cx), Vec::<String>::new());
}

#[gpui::test]
fn a_cut_folder_keeps_its_servers_file_operations(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant_for("**"), Script::default());
    s.set_trust(true);
    fs::create_dir_all(s.path("src/pkg")).unwrap();
    fs::write(s.path("src/pkg/a.rs"), "pub fn a() {}\n").unwrap();
    fs::write(s.path("src/pkg/c.rs"), "old\n").unwrap();
    fs::create_dir(s.path("dst")).unwrap();
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let changes = [
        FsChange::Path(s.path("src/pkg")),
        FsChange::Path(s.path("dst")),
    ];
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx)));
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    // Moving `src/pkg`: rename a file in it and edit it after, create one
    // over another and fill it, and edit a file elsewhere, in that order.
    let insert = |text: &str| {
        json!([{"range": {"start": {"line": 0, "character": 0},
            "end": {"line": 0, "character": 0}}, "newText": text}])
    };
    let text_edit = |uri: &str, text: &str| json!({"textDocument": {"uri": uri, "version": null}, "edits": insert(text)});
    let (a, b, c) = (
        s.uri("src/pkg/a.rs"),
        s.uri("src/pkg/b.rs"),
        s.uri("src/pkg/c.rs"),
    );
    let reply = Reply::Result(json!({"documentChanges": [
        {"kind": "rename", "oldUri": a.as_str(), "newUri": b.as_str()},
        text_edit(b.as_str(), "// moved\n"),
        {"kind": "create", "uri": c.as_str(), "options": {"overwrite": true}},
        text_edit(c.as_str(), "new\n"),
        text_edit(s.uri("src/lib.rs").as_str(), "use pkg::b;\n"),
    ]}));
    s.fake.set_reply(
        "workspace/willRenameFiles",
        Scripted {
            reply,
            delay: Duration::ZERO,
        },
    );

    let workbench = s.workbench.clone();
    for (rel, keys) in [("src/pkg", "cmd-x"), ("dst", "cmd-v")] {
        s.cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        s.cx.simulate_keystrokes("cmd-shift-e");
        let path = s.path(rel);
        s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(path, cx)));
        s.draw();
        s.cx.simulate_keystrokes(keys);
        s.draw();
    }
    s.until("the edits", |s| {
        fs::read_to_string(s.path("src/lib.rs")).is_ok_and(|lib| lib.starts_with("use pkg::b;"))
    });
    assert!(!s.path("src/pkg").exists());
    let read = |rel: &str| fs::read_to_string(s.path(rel)).ok();
    assert_eq!(read("dst/pkg/a.rs"), None);
    assert_eq!(
        read("dst/pkg/b.rs").as_deref(),
        Some("// moved\npub fn a() {}\n")
    );
    assert_eq!(read("dst/pkg/c.rs").as_deref(), Some("new\n"));
    assert_eq!(
        read("src/lib.rs").as_deref(),
        Some("use pkg::b;\npub fn f() {}\n")
    );
    assert_eq!(lsp_toasts(&s), Vec::<String>::new());
}

#[gpui::test]
fn a_late_look_never_drops_a_newer_entry(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", ""), ("b.txt", "")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root, true);
    let panel = focus_panel(&workbench, cx);
    let a = root.join("a.txt");
    for (key, cut) in [("c", false), ("x", true)] {
        select(&panel, root, "a.txt", cx);
        press(key, cx);
        // Deleted: a look at the disk starts, and is held...
        fs::remove_file(&a).unwrap();
        let seen = panel.update(cx, |panel, cx| {
            panel.apply_fs_changes(&[FsChange::Path(a.clone())], cx);
            panel.clip_seen()
        });
        // ...while it's made again, and cut or copied again.
        fs::write(&a, "").unwrap();
        panel.update(cx, |panel, cx| {
            panel.apply_fs_changes(&[FsChange::Path(a.clone())], cx)
        });
        select(&panel, root, "a.txt", cx);
        press(key, cx);
        // Its answer, that it was gone, comes too late to count.
        panel.update(cx, |panel, cx| panel.clip_looked(seen, Ok(false), cx));
        assert_eq!(clipboard(&panel, cx), Some((cut, "a.txt".into())), "{key}");
        let marked: &[&str] = if cut { &["a.txt"] } else { &[] };
        assert_eq!(drawn_cut(&panel, cx), marked, "{key}");
        // A newer one would.
        let seen = panel.read_with(cx, |panel, _| panel.clip_seen());
        panel.update(cx, |panel, cx| panel.clip_looked(seen, Ok(false), cx));
        assert_eq!(clipboard(&panel, cx), None, "{key}");
    }
}

#[gpui::test]
fn reopen_keeps_clipboard_after_stale_existence_check(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", ""), ("b.txt", "")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root, true);
    let panel = focus_panel(&workbench, cx);
    let a = root.join("a.txt");
    select(&panel, root, "a.txt", cx);
    press("c", cx);
    // Deleted: a look at the disk starts, and is held...
    fs::remove_file(&a).unwrap();
    let seen = panel.update(cx, |panel, cx| {
        panel.apply_fs_changes(&[FsChange::Path(a.clone())], cx);
        panel.clip_seen()
    });
    // ...while it's made again and the tree is opened anew.
    fs::write(&a, "").unwrap();
    panel.update(cx, |panel, cx| panel.set_show_ignored(true, cx));
    // Its answer, that it was gone, is about the old tree.
    panel.update(cx, |panel, cx| panel.clip_looked(seen, Ok(false), cx));
    cx.run_until_parked();
    assert_eq!(clipboard(&panel, cx), Some((false, "a.txt".into())));
}

#[gpui::test]
fn reopen_drops_clipboard_that_is_still_gone(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", ""), ("b.txt", "")]);
    let root = dir.path();
    let (workbench, cx) = open_root(cx, root, true);
    let panel = focus_panel(&workbench, cx);
    select(&panel, root, "a.txt", cx);
    press("x", cx);
    // Deleted with no watcher batch, so nothing has asked about it yet.
    fs::remove_file(root.join("a.txt")).unwrap();
    panel.update(cx, |panel, cx| panel.set_show_ignored(true, cx));
    cx.run_until_parked();
    assert_eq!(clipboard(&panel, cx), None);
}

#[gpui::test]
fn a_moved_symlinks_target_keeps_its_edits(cx: &mut TestAppContext) {
    let mut s = Lsp::with_scripts(cx, participant(), Script::default());
    s.set_trust(true);
    fs::write(s.path("src/impl.rs"), "orig\n").unwrap();
    std::os::unix::fs::symlink("impl.rs", s.path("src/link.rs")).unwrap();
    fs::create_dir(s.path("dst")).unwrap();
    fs::write(s.path("dst/impl.rs"), "other\n").unwrap();
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let changes = [
        FsChange::Path(s.path("src/impl.rs")),
        FsChange::Path(s.path("src/link.rs")),
        FsChange::Path(s.path("dst")),
    ];
    s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx)));
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());
    // Moving the link, a server overwrites what it led to, then fills it.
    let target = s.uri("src/impl.rs");
    let reply = Reply::Result(json!({"documentChanges": [
        {"kind": "create", "uri": target.as_str(), "options": {"overwrite": true}},
        {"textDocument": {"uri": target.as_str(), "version": null}, "edits": [
            {"range": {"start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 0}}, "newText": "made\n"},
        ]},
    ]}));
    s.fake.set_reply(
        "workspace/willRenameFiles",
        Scripted {
            reply,
            delay: Duration::ZERO,
        },
    );

    let workbench = s.workbench.clone();
    for (rel, keys) in [("src/link.rs", "cmd-x"), ("dst", "cmd-v")] {
        s.cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        s.cx.simulate_keystrokes("cmd-shift-e");
        let path = s.path(rel);
        s.cx.update(|_, cx| panel.update(cx, |panel, cx| panel.reveal(path, cx)));
        s.draw();
        s.cx.simulate_keystrokes(keys);
        s.draw();
    }
    s.until("the edits", |s| {
        fs::read_to_string(s.path("src/impl.rs")).is_ok_and(|text| text == "made\n")
    });
    // The link moved; what it led to, and the file of that name where it
    // went, stayed where they were, each with its own text.
    let link = fs::symlink_metadata(s.path("dst/link.rs")).unwrap();
    assert!(link.file_type().is_symlink());
    assert!(fs::symlink_metadata(s.path("src/link.rs")).is_err());
    assert_eq!(
        fs::read_to_string(s.path("dst/impl.rs")).unwrap(),
        "other\n"
    );
    assert_eq!(lsp_toasts(&s), Vec::<String>::new());
}

#[gpui::test]
fn a_cut_waits_for_work_in_the_folder_it_goes_in(cx: &mut TestAppContext) {
    let dir = project(&[("a.txt", "a"), ("dst/keep.txt", "")]);
    let root = fs::canonicalize(dir.path()).unwrap();
    let (workbench, cx) = open_root(cx, &root, true);
    open_file(&workbench, &root.join("a.txt"), cx);
    let a = active_editor(&workbench, cx);
    let panel = focus_panel(&workbench, cx);
    select(&panel, &root, "a.txt", cx);
    press("x", cx);

    // The folder is being worked on (moved to the Trash, say), with no
    // Save As holding anything: nothing moves.
    let running = super::ops::Running::start(root.join("dst"));
    select(&panel, &root, "dst", cx);
    press("p", cx);
    assert_eq!(
        toasts(&workbench, cx),
        ["“dst” is being worked on. Paste it once that’s done."]
    );
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "a");
    assert!(!root.join("dst/a.txt").exists());
    assert_eq!(clipboard(&panel, cx), Some((true, "a.txt".into())));
    assert_eq!(editor_path(&a, cx), Some(root.join("a.txt")));

    // Once it's done, the paste goes ahead.
    drop(running);
    press("p", cx);
    assert_eq!(fs::read_to_string(root.join("dst/a.txt")).unwrap(), "a");
    assert_eq!(editor_path(&a, cx), Some(root.join("dst/a.txt")));
}

#[gpui::test]
fn a_cut_checks_its_folder_again_once_servers_answer(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, participant(), Script::default());
    let editor = modules(&mut s, Duration::from_secs(5));
    cut_and_paste(&mut s);
    s.fake.wait_for("willRenameFiles", |records| {
        (records.iter()).any(|record| record.method() == "workspace/willRenameFiles")
    });
    // Work starts in `src/sub` while the servers are asked.
    let sub = fs::canonicalize(s.path("src/sub")).unwrap();
    let running = super::ops::Running::start(sub);
    s.clock.wait_for_sleep(Duration::from_secs(5));
    s.clock.advance(Duration::from_secs(5));
    s.draw();
    s.until("the toast", |s| !lsp_toasts(s).is_empty());
    assert_eq!(
        lsp_toasts(&s),
        ["“sub” is being worked on. Paste it once that’s done."]
    );
    assert_eq!(fs::read_to_string(s.path("src/util.rs")).unwrap(), UTIL);
    assert!(!s.path("src/sub/util.rs").exists());
    s.draw();
    assert_eq!(text(&s, &editor), MAIN);
    drop(running);
}
