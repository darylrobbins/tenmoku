//! New File and Save As…: an untitled tab from cmd-n, and the save panel,
//! answered with `simulate_new_path_selection`, from cmd-shift-s or from
//! cmd-s on an untitled tab.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_gpui::EditorView;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, KeySequence, Keymap, KeymapLayer};
use fe_lsp::fake::Record;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::PaneItem as _;

use crate::{
    Workbench,
    tests::{lsp::Lsp, open_root},
};

/// A folder holding `a.rs`.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    dir
}

/// A workbench rooted at `root`, showing `a.rs` if `open`, focused.
fn setup<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    open: bool,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    if open {
        let pane = workbench.read_with(cx, |w, _| w.pane().clone());
        let path = root.join("a.rs");
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    }
    cx.run_until_parked();
    (workbench, cx)
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |w, cx| w.active_editor(cx).cloned().expect("a tab"))
}

/// A tab: its path, as the pane has it, its title and whether it's dirty.
type TabState = (Option<PathBuf>, String, bool);

/// Each tab of the active pane, and the active tab.
fn tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<TabState>, Option<usize>) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tabs = (pane.tabs().iter())
            .map(|tab| {
                let editor = tab.editor().unwrap().read(cx);
                let title = editor.tab_title(cx).to_string();
                (tab.path.clone(), title, editor.model().is_dirty())
            })
            .collect();
        (tabs, pane.active())
    })
}

fn language(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    editor.read_with(cx, |editor, _| {
        editor.syntax().map(|syntax| syntax.language().name())
    })
}

/// Answers the pending save panel with `answer`, and returns the folder it
/// was shown in.
fn answer(answer: Option<PathBuf>, cx: &mut VisualTestContext) -> PathBuf {
    assert!(cx.did_prompt_for_new_path(), "the save panel shows");
    let mut shown = None;
    cx.simulate_new_path_selection(|dir| {
        shown = Some(dir.to_owned());
        answer
    });
    cx.run_until_parked();
    shown.expect("answered")
}

#[gpui::test]
fn new_file(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let a = active(&workbench, cx);
    assert!(cx.update(|window, cx| a.focus_handle(cx).is_focused(window)));

    cx.simulate_keystrokes("cmd-n");
    cx.run_until_parked();
    let (tabs, shown) = tabs(&workbench, cx);
    assert_eq!(
        tabs,
        [
            (Some(dir.path().join("a.rs")), "a.rs".into(), false),
            (None, "untitled".into(), false)
        ]
    );
    assert_eq!(shown, Some(1));
    let untitled = active(&workbench, cx);
    assert!(cx.update(|window, cx| untitled.focus_handle(cx).is_focused(window)));
    assert_eq!(
        untitled.read_with(cx, |editor, _| editor.model().text().to_string()),
        ""
    );
    cx.simulate_input("hi");
    assert_eq!(
        untitled.read_with(cx, |editor, _| editor.model().text().to_string()),
        "hi"
    );
    assert!(!cx.did_prompt_for_new_path());

    // The File menu: New File first, Save As… after Save.
    let (bar, diagnostics) = fe_keymap::default_menus_macos(Some(&*crate::command_catalog()));
    assert_eq!(diagnostics, []);
    let bar = bar.unwrap();
    let (layer, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let keymap = Keymap::from_layers([layer.unwrap()]);
    let file = bar.menu("File").unwrap();
    let ids: Vec<_> = (file.items.iter())
        .map(|item| item.id().unwrap_or("-"))
        .collect();
    assert_eq!(ids[0], "workbench.new_file");
    let save = ids.iter().position(|&id| id == "editor.save").unwrap();
    assert_eq!(ids[save + 1], "workbench.save_as");
    for (id, title, keys) in [
        ("workbench.new_file", "New File", "cmd-n"),
        ("workbench.save_as", "Save As…", "cmd-shift-s"),
    ] {
        let item = file.item(id).and_then(|item| item.command()).unwrap();
        assert_eq!(item.title, title);
        assert_eq!(
            item.shortcut(&keymap),
            Some(KeySequence::parse(keys).unwrap())
        );
    }
}

#[gpui::test]
fn save_as_moves_the_tab(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let (a, b) = (dir.path().join("a.rs"), dir.path().join("b.py"));
    let editor = active(&workbench, cx);
    assert_eq!(language(&editor, cx), Some("rust"));
    // A split shows the buffer too, in another pane.
    cx.simulate_keystrokes("cmd-\\");
    cx.run_until_parked();
    let split = active(&workbench, cx);
    assert_ne!(split, editor);
    cx.simulate_input("x");
    cx.run_until_parked();

    cx.simulate_keystrokes("cmd-shift-s");
    assert_eq!(answer(Some(b.clone()), cx), dir.path());
    assert_eq!(fs::read_to_string(&b).unwrap(), "xfn a() {}\n");
    assert_eq!(
        fs::read_to_string(&a).unwrap(),
        "fn a() {}\n",
        "left as it was"
    );
    assert_eq!(
        tabs(&workbench, cx).0,
        [(Some(b.clone()), "b.py".into(), false)]
    );
    for view in [&editor, &split] {
        assert_eq!(
            view.read_with(cx, |view, _| view.model().path().map(Path::to_owned)),
            Some(b.clone())
        );
        assert_eq!(language(view, cx), Some("python"));
    }
    let panes = workbench.read_with(cx, |w, cx| w.pane_group(cx).panes().len());
    assert_eq!(panes, 2);
    let titles: Vec<_> = workbench.read_with(cx, |w, cx| {
        (w.pane_group(cx).panes().iter())
            .map(|pane| pane.read(cx).title(cx))
            .collect()
    });
    assert_eq!(titles, ["b.py — Tenmoku", "b.py — Tenmoku"]);
    // Saved there from now on.
    cx.simulate_input("y");
    cx.simulate_keystrokes("cmd-s");
    cx.run_until_parked();
    assert!(!cx.did_prompt_for_new_path());
    assert_eq!(fs::read_to_string(&b).unwrap(), "xyfn a() {}\n");
    assert_eq!(fs::read_to_string(&a).unwrap(), "fn a() {}\n");
}

#[gpui::test]
fn untitled_cmd_s(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = setup(cx, dir.path(), true);
    cx.simulate_keystrokes("cmd-n");
    cx.run_until_parked();
    cx.simulate_input("hello");

    // Cancelled: still untitled, and dirty.
    cx.simulate_keystrokes("cmd-s");
    cx.run_until_parked();
    assert_eq!(answer(None, cx), dir.path(), "in the root");
    let untitled = (None, "untitled".to_owned(), true);
    assert_eq!(tabs(&workbench, cx).0[1], untitled);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);

    // A file another tab shows is refused, by any path to it.
    let a = dir.path().join("a.rs");
    fs::create_dir(dir.path().join("sub")).unwrap();
    let mut aliases = vec![a.clone(), dir.path().join("sub/../a.rs")];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&a, dir.path().join("link.rs")).unwrap();
        aliases.push(dir.path().join("link.rs"));
    }
    for alias in aliases {
        cx.simulate_keystrokes("cmd-s");
        cx.run_until_parked();
        answer(Some(alias.clone()), cx);
        let prompt = cx.pending_prompt().map(|(question, _)| question);
        let name = alias.file_name().unwrap().to_string_lossy();
        let refusal = format!("“{name}” is open in another tab.");
        assert_eq!(prompt, Some(refusal), "{alias:?}");
        cx.simulate_prompt_answer("OK");
        cx.run_until_parked();
        assert_eq!(tabs(&workbench, cx).0[1], untitled);
        assert_eq!(fs::read_to_string(&a).unwrap(), "fn a() {}\n");
    }
    fs::remove_dir(dir.path().join("sub")).unwrap();
    #[cfg(unix)]
    fs::remove_file(dir.path().join("link.rs")).unwrap();

    // Shown in another window, or being read into one, it's refused too.
    let (b, c) = (dir.path().join("b.rs"), dir.path().join("c.rs"));
    fs::write(&b, "fn b() {}\n").unwrap();
    fs::write(&c, "fn c() {}\n").unwrap();
    let launch = |file: PathBuf, buffer: Option<fe_buffer::Buffer>| crate::Launch {
        root: Some(dir.path().to_owned()),
        file: Some((file, buffer.map(crate::loader::Opened::from))),
    };
    let shows = launch(b.clone(), Some(fe_buffer::Buffer::new("fn b() {}\n")));
    let (_shows, _shows_cx) = crate::tests::lsp::window(cx, shows);
    let (_reads, _reads_cx) = crate::tests::lsp::window(cx, launch(c.clone(), None));
    for other in [&b, &c] {
        cx.simulate_keystrokes("cmd-s");
        cx.run_until_parked();
        answer(Some(other.clone()), cx);
        let name = other.file_name().unwrap().to_string_lossy();
        let refused = format!("“{name}” is open in another tab.");
        assert_eq!(refusal(cx), Some(refused), "{other:?}");
        cx.run_until_parked();
        assert_eq!(tabs(&workbench, cx).0[1], untitled);
    }
    assert_eq!(fs::read_to_string(&b).unwrap(), "fn b() {}\n");
    assert_eq!(fs::read_to_string(&c).unwrap(), "fn c() {}\n");
    fs::remove_file(&b).unwrap();
    fs::remove_file(&c).unwrap();

    // Saved where it answers.
    let new = dir.path().join("new.txt");
    cx.simulate_keystrokes("cmd-s");
    cx.run_until_parked();
    answer(Some(new.clone()), cx);
    assert_eq!(fs::read_to_string(&new).unwrap(), "hello");
    assert_eq!(
        tabs(&workbench, cx),
        (
            vec![
                (Some(a), "a.rs".into(), false),
                (Some(new), "new.txt".into(), false)
            ],
            Some(1)
        )
    );
    assert!(cx.pending_prompt().is_none());
}

#[gpui::test]
fn lsp_follows(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/lib.rs");
    let (lib, c) = (s.uri("src/lib.rs"), s.uri("src/c.rs"));
    s.fake.wait_for_document(&lib, "didOpen", |r| r.is_some());

    s.cx.simulate_keystrokes("cmd-shift-s");
    s.cx.run_until_parked();
    answer(Some(s.path("src/c.rs")), &mut s.cx);
    s.draw();
    s.fake.wait_for_document(&c, "didOpen", |r| r.is_some());
    s.fake
        .wait_for_document(&lib, "didClose", |r| r.is_some_and(|r| !r.open));

    // A save of other.rs, then a Save As queued behind it, both landing
    // before any frame: other.rs hears of its own save only, and the
    // document under the new name, of none (it opens with the text saved).
    let other = s.uri("src/other.rs");
    s.open("src/other.rs");
    s.fake.wait_for_document(&other, "didOpen", |r| r.is_some());
    s.cx.simulate_input("x");
    s.draw();
    let editor = (s.workbench).read_with(&s.cx, |w, cx| w.active_editor(cx).cloned().unwrap());
    let (moved, moved_uri) = (s.path("src/moved.rs"), s.uri("src/moved.rs"));
    s.cx.update(|window, cx| {
        drop(editor.update(cx, |editor, cx| editor.save(window, cx)));
        (s.workbench.clone()).update(cx, |w, cx| w.save_editor_as(&editor, moved, window, cx))
    });
    s.cx.executor().run_until_parked();
    assert!(!editor.read_with(&s.cx, |view, _| view.model().is_dirty()));
    s.draw();
    s.fake
        .wait_for_document(&moved_uri, "didOpen", |r| r.is_some());
    s.fake
        .wait_for_document(&other, "didClose", |r| r.is_some_and(|r| !r.open));
    assert_eq!(s.sent(1, "textDocument/didSave", &other), 1);
    assert_eq!(s.sent(1, "textDocument/didSave", &moved_uri), 0);
    assert_eq!(
        fs::read_to_string(s.path("src/other.rs")).unwrap(),
        "xpub fn g() {}\n"
    );
    s.fake
        .wait_for_document(&lib, "didClose", |r| r.is_some_and(|r| !r.open));

    let records = s.fake.records_of(1);
    let at = |method: &str, uri: &lsp_types::Url| {
        records.iter().position(|record| match record {
            Record::Notification { method: m, params } => {
                m == method && params["textDocument"]["uri"] == uri.as_str()
            }
            _ => false,
        })
    };
    let closed = at("textDocument/didClose", &lib).expect("lib.rs closed");
    let opened = at("textDocument/didOpen", &c).expect("c.rs opened");
    assert!(closed < opened, "closed at {closed}, opened at {opened}");
    assert_eq!(s.sent(1, "textDocument/didOpen", &c), 1);
    assert_eq!(s.sent(1, "textDocument/didOpen", &lib), 1);
    // lib.rs wasn't saved: only c.rs was written.
    assert_eq!(s.sent(1, "textDocument/didSave", &lib), 0);
}

/// Saves `editor` as `path` through the workbench, as the save panel's
/// answer does, without running the save.
fn save_as(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    path: &Path,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        workbench.update(cx, |w, cx| {
            w.save_editor_as(editor, path.to_owned(), window, cx)
        })
    });
}

/// The pending prompt's question, answered OK.
fn refusal(cx: &mut VisualTestContext) -> Option<String> {
    let prompt = cx.pending_prompt().map(|(question, _)| question);
    if prompt.is_some() {
        cx.simulate_prompt_answer("OK");
    }
    prompt
}

/// While saves are held, each destination is taken, by any path to it: a
/// Save As of another buffer there is refused, and opening it waits for
/// the save, then shows the tab saved there. Saves queued for one buffer
/// land in the order asked.
#[gpui::test]
fn a_save_under_way(cx: &mut TestAppContext) {
    let dir = project();
    fs::write(dir.path().join("other.rs"), "fn other() {}\n").unwrap();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let (a_rs, b, c) = (
        dir.path().join("a.rs"),
        dir.path().join("b.rs"),
        dir.path().join("c.rs"),
    );
    let a = active(&workbench, cx);
    cx.simulate_input("x");
    // Not run: the writes wait on the background executor.
    save_as(&workbench, &a, &b, cx);
    save_as(&workbench, &a, &c, cx);
    assert!(!b.exists() && !c.exists());
    assert_eq!(
        tabs(&workbench, cx).0,
        [(Some(a_rs.clone()), "a.rs".into(), true)]
    );

    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    let other_rs = dir.path().join("other.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(other_rs, window, cx)));
    cx.run_until_parked();
    // Ran: hold the next ones.
    let other = active(&workbench, cx);
    assert_ne!(other, a);
    cx.simulate_input("y");
    let a_text = || fs::read_to_string(dir.path().join("a.rs")).unwrap();
    assert_eq!(a_text(), "fn a() {}\n");
    assert_eq!(fs::read_to_string(&c).unwrap(), "xfn a() {}\n");
    assert_eq!(
        active_path(&workbench, cx),
        Some(dir.path().join("other.rs"))
    );

    // Both landed, in order: the tab is c.rs's. Held again, it saves to b.rs.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    save_as(&workbench, &a, &b, cx);
    save_as(&workbench, &other, &b, cx);
    assert_eq!(
        refusal(cx).as_deref(),
        Some("“b.rs” is open in another tab.")
    );
    #[cfg(unix)]
    {
        // A link to it, dangling while it's being written.
        let link = dir.path().join("link.rs");
        std::os::unix::fs::symlink(&b, &link).unwrap();
        fs::remove_file(&b).unwrap();
        save_as(&workbench, &other, &link, cx);
        assert_eq!(
            refusal(cx),
            Some("“link.rs” is open in another tab.".into())
        );
    }
    // Opening it waits for the save, then shows its tab.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(b.clone(), window, cx)));
    let opening = pane.read_with(cx, |pane, _| pane.is_opening(&b));
    assert!(opening);
    cx.run_until_parked();
    assert_eq!(active(&workbench, cx), a);
    assert_eq!(active_path(&workbench, cx), Some(b.clone()));
    let (tabs, _) = tabs(&workbench, cx);
    assert_eq!(tabs.len(), 2, "{tabs:?}");
    assert_eq!(fs::read_to_string(&b).unwrap(), "xfn a() {}\n");
    assert_eq!(
        other.read_with(cx, |v, _| v.model().text().to_string()),
        "yfn other() {}\n"
    );
}

fn active_path(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    let editor = active(workbench, cx);
    editor.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned))
}

/// A save that fails changes nothing, and says why; meanwhile the buffer
/// keeps its file, which opens in its tab.
#[gpui::test]
fn a_failed_save_as_goes_back(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let a_rs = dir.path().join("a.rs");
    let a = active(&workbench, cx);
    cx.simulate_input("x");
    save_as(&workbench, &a, &dir.path().join("missing/b.py"), cx);
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(a_rs.clone(), window, cx)));
    assert_eq!(active(&workbench, cx), a);
    cx.run_until_parked();
    assert_eq!(refusal(cx).as_deref(), Some("“b.py” couldn’t be saved."));
    cx.run_until_parked();
    assert_eq!(
        tabs(&workbench, cx).0,
        [(Some(a_rs.clone()), "a.rs".into(), true)]
    );
    assert_eq!(language(&a, cx), Some("rust"));
    assert_eq!(fs::read_to_string(&a_rs).unwrap(), "fn a() {}\n");

    // A split's language, chosen by hand, stays: the buffer didn't move.
    cx.simulate_keystrokes("cmd-\\");
    cx.run_until_parked();
    let split = active(&workbench, cx);
    assert_ne!(split, a);
    split.update(cx, |split, cx| split.set_language(Some("python"), cx));
    cx.run_until_parked();
    assert_eq!(language(&split, cx), Some("python"));
    save_as(&workbench, &a, &dir.path().join("missing/c.rs"), cx);
    cx.run_until_parked();
    assert_eq!(refusal(cx).as_deref(), Some("“c.rs” couldn’t be saved."));
    cx.run_until_parked();
    assert_eq!(language(&split, cx), Some("python"));
    assert_eq!(language(&a, cx), Some("rust"));
}

/// The destination's line ending is the buffer's before it's written: the
/// file has it, and the buffer is clean.
#[gpui::test]
fn the_destinations_line_ending(cx: &mut TestAppContext) {
    let dir = project();
    fs::write(
        dir.path().join(".editorconfig"),
        "[*.txt]\nend_of_line = crlf\n",
    )
    .unwrap();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let a = active(&workbench, cx);
    let out = dir.path().join("out.txt");
    save_as(&workbench, &a, &out, cx);
    cx.run_until_parked();
    assert_eq!(fs::read(&out).unwrap(), b"fn a() {}\r\n");
    assert_eq!(
        tabs(&workbench, cx).0,
        [(Some(out), "out.txt".into(), false)]
    );
}

/// Each queued Save As is written with its own destination's line ending,
/// behind an ordinary save that's written with the buffer's.
#[gpui::test]
fn queued_save_as_line_endings(cx: &mut TestAppContext) {
    let dir = project();
    let config = "[*.txt]\nend_of_line = crlf\n\n[*.rs]\nend_of_line = lf\n";
    fs::write(dir.path().join(".editorconfig"), config).unwrap();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let (b, c) = (dir.path().join("b.txt"), dir.path().join("c.rs"));
    let a = active(&workbench, cx);
    cx.simulate_input("x");
    // Held, all three: none runs until the executor does.
    cx.update(|window, cx| {
        drop(a.update(cx, |editor, cx| editor.save(window, cx)));
        workbench.update(cx, |w, cx| {
            w.save_editor_as(&a, b.clone(), window, cx);
            w.save_editor_as(&a, c.clone(), window, cx);
        })
    });
    assert_eq!(
        a.read_with(cx, |editor, _| editor.model().buffer().line_ending()),
        fe_buffer::LineEnding::Lf
    );
    cx.run_until_parked();
    assert_eq!(fs::read(dir.path().join("a.rs")).unwrap(), b"xfn a() {}\n");
    assert_eq!(fs::read(&b).unwrap(), b"xfn a() {}\r\n");
    assert_eq!(fs::read(&c).unwrap(), b"xfn a() {}\n");
    assert_eq!(
        tabs(&workbench, cx).0,
        [(Some(c.clone()), "c.rs".into(), false)]
    );
    assert_eq!(
        a.read_with(cx, |editor, _| editor.model().buffer().line_ending()),
        fe_buffer::LineEnding::Lf
    );
}

/// A Save As that fails leaves the buffer's line ending, and whether it's
/// dirty, as they were, whatever the destination's setting.
#[gpui::test]
fn a_failed_save_as_keeps_line_endings(cx: &mut TestAppContext) {
    let dir = project();
    fs::write(
        dir.path().join(".editorconfig"),
        "[*.txt]\nend_of_line = crlf\n",
    )
    .unwrap();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let missing = dir.path().join("missing/x.txt");
    let config = fe_config::editorconfig_for(&missing);
    let crlf = crate::pane::config_line_ending(&config);
    assert_eq!(
        crlf,
        Some(fe_buffer::LineEnding::CrLf),
        "the setting applies"
    );
    let a = active(&workbench, cx);
    let state = |cx: &mut VisualTestContext| {
        a.read_with(cx, |editor, _| {
            let model = editor.model();
            (model.buffer().line_ending(), model.is_dirty())
        })
    };
    for typed in [false, true] {
        if typed {
            cx.simulate_input("x");
        }
        save_as(&workbench, &a, &missing, cx);
        cx.run_until_parked();
        assert_eq!(refusal(cx).as_deref(), Some("“x.txt” couldn’t be saved."));
        assert_eq!(state(cx), (fe_buffer::LineEnding::Lf, typed));
        assert_eq!(
            tabs(&workbench, cx).0,
            [(Some(dir.path().join("a.rs")), "a.rs".into(), typed)]
        );
    }
}

/// The `.editorconfig` properties follow the file to its new name and
/// folder, and an untitled buffer gets its file's once saved.
#[gpui::test]
fn editorconfig_follows(cx: &mut TestAppContext) {
    let dir = project();
    let config = "[*.rs]\ntab_width = 2\n\n[*.py]\ntab_width = 3\n";
    fs::write(dir.path().join(".editorconfig"), config).unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/.editorconfig"), "[*]\ntab_width = 5\n").unwrap();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let editor = active(&workbench, cx);
    let tab_size = |cx: &mut VisualTestContext| {
        let editor = workbench.read_with(cx, |w, cx| w.active_editor(cx).cloned().unwrap());
        editor.read_with(cx, |editor, _| editor.model().tab_size())
    };
    assert_eq!(tab_size(cx), 2);

    let save_as = |path: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes("cmd-shift-s");
        answer(Some(dir.path().join(path)), cx);
    };
    save_as("b.py", cx);
    assert_eq!(tab_size(cx), 3);
    save_as("sub/c.py", cx);
    assert_eq!(tab_size(cx), 5);
    save_as("plain.txt", cx);
    assert_eq!(tab_size(cx), 4, "the settings' again");
    assert_eq!(active(&workbench, cx), editor);

    cx.simulate_keystrokes("cmd-n");
    cx.run_until_parked();
    cx.simulate_input("fn new() {}\n");
    cx.simulate_keystrokes("cmd-s");
    answer(Some(dir.path().join("new.rs")), cx);
    assert_eq!(tab_size(cx), 2);
}

struct HoldSave;

impl fe_gpui::pane_item::SaveHook for HoldSave {
    fn before_save(&self, _: &Entity<EditorView>, cx: &mut gpui::App) -> gpui::Task<()> {
        gpui::AppContext::background_spawn(cx, std::future::pending())
    }
}

/// The save panel's Save As writes the destination's `.editorconfig` fixes,
/// not the source's (a.rs has none, b.md trims and ends with a newline), at
/// once and queued behind a save that writes the text as typed.
#[gpui::test]
fn save_as_applies_the_destinations_editorconfig(cx: &mut TestAppContext) {
    let dir = project();
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*.md]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n",
    )
    .unwrap();
    let (workbench, cx) = setup(cx, dir.path(), true);
    let a = active(&workbench, cx);
    let type_dirty = |cx: &mut VisualTestContext| {
        a.update(cx, |a, _| {
            a.model_mut().select_all();
            a.model_mut().insert("a  \nb\t");
        });
    };

    // At once.
    type_dirty(cx);
    cx.simulate_keystrokes("cmd-shift-s");
    answer(Some(dir.path().join("b.md")), cx);
    assert_eq!(
        fs::read_to_string(dir.path().join("b.md")).unwrap(),
        "a\nb\n"
    );

    // Queued: the save ahead of it is the buffer's own (b.md now, with the
    // fixes), so go back to a source without any.
    let c = dir.path().join("c.rs");
    save_as(&workbench, &a, &c, cx);
    cx.run_until_parked();
    type_dirty(cx);
    let saving = cx.update(|window, cx| {
        a.update(cx, |a, cx| {
            a.set_save_hook(Some(std::rc::Rc::new(HoldSave)));
            a.save(window, cx)
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-shift-s");
    answer(Some(dir.path().join("d.md")), cx);
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    cx.run_until_parked();
    drop(saving);
    assert_eq!(fs::read_to_string(&c).unwrap(), "a  \nb\t");
    assert_eq!(
        fs::read_to_string(dir.path().join("d.md")).unwrap(),
        "a\nb\n"
    );
}
