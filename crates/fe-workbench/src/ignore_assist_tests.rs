//! Paths and match counts in ignore files, through each editor's providers in a
//! real workbench over a temp dir, beside a fake language server.

use std::{
    cell::RefCell,
    fs,
    future::Future,
    path::Path,
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

use fe_editor::completion::{CompletionItem, CompletionSession, CompletionTrigger};
use fe_gpui::{EditorView, HoverContent};
use fe_lsp::fake::{Reply, Scripted};
use fe_project::{FsChange, check_ignore_file};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{
    LSPAny, LSPObject,
    request::{Completion, HoverRequest, Request as _},
};

use super::{MAX_ENTRIES, MAX_HOVER_PATHS, TypedPath, describe, escaped, typed_path};
use crate::tests::lsp::Lsp;

/// Writes `name` below the root, with its directories.
fn write(s: &Lsp, name: &str, text: &str) {
    let path = s.path(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// `name`, written with `text`, open and focused, the caret at its end.
fn open_ignore(s: &mut Lsp, name: &str, text: &str) -> Entity<EditorView> {
    write(s, name, text);
    s.open(name);
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("the file is shown");
    place_caret(s, &editor, text.len());
    editor
}

/// Replaces what `editor`'s buffer holds, the caret at its end.
fn set_text(s: &mut Lsp, editor: &Entity<EditorView>, text: &str) {
    place_caret(s, editor, 0);
    s.cx.simulate_keystrokes("cmd-a");
    editor.update(&mut s.cx, |view, cx| {
        view.model_mut().insert(text);
        cx.notify();
    });
    s.draw();
    place_caret(s, editor, text.len());
}

fn place_caret(s: &mut Lsp, editor: &Entity<EditorView>, caret: usize) {
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(caret);
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
}

/// What `future` comes to, with the background work it waits on let run.
fn answer<T: 'static>(s: &mut Lsp, future: Pin<Box<dyn Future<Output = T>>>) -> T {
    let slot = Rc::new(RefCell::new(None));
    let filled = slot.clone();
    s.cx.update(|_, cx| {
        cx.spawn(async move |_| {
            let answer = future.await;
            *filled.borrow_mut() = Some(answer);
        })
        .detach();
    });
    s.until("the answer", |_| slot.borrow().is_some());
    slot.take().unwrap()
}

/// What `editor`'s provider offers at its caret, as a menu there would rank it.
fn offered(s: &mut Lsp, editor: &Entity<EditorView>) -> Vec<CompletionItem> {
    let (snapshot, offset, reply) = s.cx.update(|_, cx| {
        let view = editor.read(cx);
        let provider = view.completion_provider().expect("a provider").clone();
        let snapshot = view.model().snapshot().clone();
        let offset = view.model().primary_selection().head();
        let reply = provider.completions(
            snapshot.id(),
            snapshot.version(),
            offset,
            CompletionTrigger::Invoked,
        );
        (snapshot, offset, reply)
    });
    let list = answer(s, reply).unwrap_or_default();
    let session = CompletionSession::new(snapshot, offset, list);
    (session.matches().iter())
        .map(|&ix| session.items()[ix].clone())
        .collect()
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    let mut labels: Vec<_> = items.iter().map(|item| item.label.as_str()).collect();
    labels.sort_unstable();
    labels
}

/// What `editor`'s provider says about byte `offset`.
fn hover_at(s: &mut Lsp, editor: &Entity<EditorView>, offset: usize) -> Option<HoverContent> {
    let reply = s.cx.update(|_, cx| {
        let view = editor.read(cx);
        let provider = view.hover_provider().expect("a provider").clone();
        let snapshot = view.model().snapshot();
        provider.hover(snapshot.id(), snapshot.version(), offset)
    });
    answer(s, reply)
}

fn triggers(s: &mut Lsp, editor: &Entity<EditorView>) -> Vec<String> {
    s.cx.update(|_, cx| {
        let view = editor.read(cx);
        let provider = view.completion_provider().expect("a provider").clone();
        provider.trigger_characters(view.model().buffer().id())
    })
}

fn text_of(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

/// Has the workbench's check record whether each run was on the main thread.
fn record_checks(s: &mut Lsp) -> Arc<Mutex<Vec<bool>>> {
    let checks = Arc::new(Mutex::new(Vec::new()));
    let recorded = checks.clone();
    let executor = s.cx.executor();
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |workbench, _| {
        workbench.set_ignore_check(Arc::new(move |path, text, cancel| {
            recorded.lock().unwrap().push(executor.is_main_thread());
            check_ignore_file(path, text, cancel)
        }));
    });
    checks
}

#[gpui::test]
fn path_completion_lists_directory_children(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write(&s, "assets/logo.png", "");
    write(&s, "assets/.hidden", "");
    write(&s, "assets/icons/a.svg", "");
    let editor = open_ignore(&mut s, ".gitignore", "");

    // Typing the `/` asks, and the menu shows.
    assert!(triggers(&mut s, &editor).contains(&"/".to_owned()));
    s.cx.simulate_input("assets/");
    s.until("the menu", |s| {
        editor.read_with(&s.cx, |view, _| view.completion_shown())
    });
    let items = offered(&mut s, &editor);
    assert_eq!(labels(&items), [".hidden", "icons/", "logo.png"]);
    let icons = (items.iter()).find(|item| item.label == "icons/").unwrap();
    assert_eq!(icons.kind.as_deref(), Some("folder"));
    assert_eq!(
        icons.edit,
        ("assets/".len().."assets/".len(), "icons/".to_owned())
    );

    // Narrowed by what's typed, and accepted in place of it.
    s.cx.simulate_input("lo");
    s.draw();
    assert!(editor.read_with(&s.cx, |view, _| view.completion_shown()));
    s.cx.simulate_keystrokes("enter");
    s.draw();
    assert_eq!(text_of(&s, &editor), "assets/logo.png");
}

#[gpui::test]
fn nested_ignore_file_completes_from_its_own_directory(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write(&s, "beta-at-root.txt", "");
    write(&s, "sub/bar.txt", "");
    write(&s, "sub/baz/inner.txt", "");
    let editor = open_ignore(&mut s, "sub/.gitignore", "b");
    assert_eq!(labels(&offered(&mut s, &editor)), ["bar.txt", "baz/"]);

    // Anchored, or negated, it's the same directory.
    set_text(&mut s, &editor, "!/b");
    assert_eq!(labels(&offered(&mut s, &editor)), ["bar.txt", "baz/"]);
    let item = offered(&mut s, &editor).remove(0);
    assert_eq!(item.edit.0, "!/".len().."!/b".len());
}

#[gpui::test]
fn hover_lists_matched_files(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    for ix in 0..MAX_HOVER_PATHS + 2 {
        write(&s, &format!("logs/{ix:02}.log"), "");
    }
    write(&s, "one.only", "");
    write(&s, "built/out.bin", "");
    let text = "*.log\n*.nothing\none.only\nbuilt/\n";
    let editor = open_ignore(&mut s, ".gitignore", text);

    // Through the editor's own hover, on the pattern under the caret.
    place_caret(&mut s, &editor, "*.l".len());
    s.cx.simulate_keystrokes("cmd-k cmd-i");
    s.until("the hover", |s| {
        editor
            .read_with(&s.cx, |view, _| view.popover_text())
            .is_some()
    });
    let shown = crate::tests::slashed(
        editor
            .read_with(&s.cx, |view, _| view.popover_text())
            .unwrap(),
    );
    assert!(shown.contains("matches 12 files"), "{shown}");
    assert!(shown.contains("logs/00.log"), "{shown}");
    assert!(shown.contains("logs/09.log"), "{shown}");
    assert!(!shown.contains("logs/10.log"), "{shown}");
    assert!(shown.contains("and 2 more"), "{shown}");

    let hover = hover_at(&mut s, &editor, 2).expect("an answer");
    assert_eq!(hover.range, Some(0.."*.log".len()));

    let nothing = "*.log\n".len();
    let hover = hover_at(&mut s, &editor, nothing + 1).expect("an answer");
    assert_eq!(hover.text, "This pattern matches no files");
    assert_eq!(hover.range, Some(nothing..nothing + "*.nothing".len()));

    let one = text.find("one.only").unwrap();
    let hover = hover_at(&mut s, &editor, one).expect("an answer");
    assert!(hover.text.contains("matches 1 file:"), "{}", hover.text);
    assert!(hover.text.contains("one.only"), "{}", hover.text);

    // A directory shows as one, without its contents.
    let built = text.find("built").unwrap();
    let hover = hover_at(&mut s, &editor, built).expect("an answer");
    assert!(hover.text.contains("built/\n"), "{}", hover.text);
    assert!(!hover.text.contains("out.bin"), "{}", hover.text);
}

#[gpui::test]
fn comments_dockerignore_and_other_buffers_are_untouched(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write(&s, "assets/logo.png", "");
    write(&s, "a.log", "");

    // A comment, with the caret in it or before it, and a blank line.
    let text = "# assets/\n\n*.log\n";
    let editor = open_ignore(&mut s, ".gitignore", text);
    for caret in [0, "# assets/".len()] {
        place_caret(&mut s, &editor, caret);
        assert_eq!(labels(&offered(&mut s, &editor)), [] as [&str; 0]);
    }
    assert!(hover_at(&mut s, &editor, 3).is_none());
    assert!(hover_at(&mut s, &editor, "# assets/\n".len()).is_none());
    assert!(hover_at(&mut s, &editor, "# assets/\n\n".len()).is_some());

    // Docker's file, whose patterns are read by other rules.
    let editor = open_ignore(&mut s, ".dockerignore", "assets/");
    assert!(!triggers(&mut s, &editor).contains(&"/".to_owned()));
    assert_eq!(labels(&offered(&mut s, &editor)), [] as [&str; 0]);
    assert!(hover_at(&mut s, &editor, 0).is_none());

    // A Rust buffer: what the server says, as it says it.
    let markup = LSPObject::from_iter([
        ("kind".to_owned(), LSPAny::from("markdown")),
        ("value".to_owned(), LSPAny::from("The server's own")),
    ]);
    let hover = LSPObject::from_iter([("contents".to_owned(), LSPAny::Object(markup))]);
    s.fake.set_reply(
        HoverRequest::METHOD,
        Scripted::now(Reply::Result(LSPAny::Object(hover))),
    );
    let list: LSPAny = r#"{ "isIncomplete": false, "items": [{ "label": "from_server" }] }"#
        .parse()
        .unwrap();
    s.fake
        .set_reply(Completion::METHOD, Scripted::now(Reply::Result(list)));
    let editor = open_ignore(&mut s, "src/assist.rs", "assets/");
    let uri = s.uri("src/assist.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    assert!(!triggers(&mut s, &editor).contains(&"/".to_owned()));
    assert_eq!(labels(&offered(&mut s, &editor)), ["from_server"]);
    let hover = hover_at(&mut s, &editor, 0).expect("the server's");
    assert_eq!(hover.text, "The server's own");
}

/// A split picked into another language keeps its answers, so the buffer isn't
/// helped until it's back.
#[gpui::test]
fn a_split_in_another_language_turns_help_off(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write(&s, "assets/logo.png", "");
    let first = open_ignore(&mut s, ".gitignore", "assets/");
    assert_eq!(labels(&offered(&mut s, &first)), ["logo.png"]);
    s.run("pane.split_right");
    s.draw();
    let second = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("the split shows it");
    assert_ne!(first, second);
    second.update(&mut s.cx, |view, cx| view.set_language(None, cx));
    s.draw();
    assert_eq!(labels(&offered(&mut s, &first)), [] as [&str; 0]);
    assert!(hover_at(&mut s, &first, 0).is_none());
    assert!(!triggers(&mut s, &first).contains(&"/".to_owned()));

    second.update(&mut s.cx, |view, cx| {
        view.set_language(Some("gitignore"), cx)
    });
    s.draw();
    assert_eq!(labels(&offered(&mut s, &first)), ["logo.png"]);
}

/// Named as an ignore file, but picked into another language by hand.
#[gpui::test]
fn a_hand_picked_rust_file_named_as_an_ignore_file_is_left_to_its_server(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write(&s, "assets/logo.png", "");
    let editor = open_ignore(&mut s, ".gitignore", "assets/");
    editor.update(&mut s.cx, |view, cx| view.set_language(Some("rust"), cx));
    s.draw();
    assert!(!triggers(&mut s, &editor).contains(&"/".to_owned()));
    let items = offered(&mut s, &editor);
    assert!(
        !items.iter().any(|item| item.label == "logo.png"),
        "{items:?}"
    );
}

/// One walk, off the main thread, serves hovers until something it depends on
/// changes.
#[gpui::test]
fn hover_walks_off_the_main_thread_once_until_something_changes(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let checks = record_checks(&mut s);
    write(&s, "a.log", "");
    let editor = open_ignore(&mut s, ".gitignore", "*.log\n*.txt\n");
    let before = checks.lock().unwrap().len();

    assert!(hover_at(&mut s, &editor, 0).is_some());
    assert!(hover_at(&mut s, &editor, 0).is_some());
    assert!(hover_at(&mut s, &editor, "*.log\n".len()).is_some());
    let walks = || checks.lock().unwrap().len();
    assert_eq!(walks() - before, 1, "one walk for one version");
    assert!(!checks.lock().unwrap().iter().any(|main| *main));

    // The file tree changes: a new match shows.
    write(&s, "b.log", "");
    let panel = s.workbench.read_with(&s.cx, |w, _| w.panel().clone());
    let changes = [FsChange::Path(s.path("b.log"))];
    panel.update(&mut s.cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
    s.draw();
    let hover = hover_at(&mut s, &editor, 0).expect("an answer");
    assert!(hover.text.contains("matches 2 files"), "{}", hover.text);

    // An edit: the unsaved text is what's checked.
    let count = walks();
    place_caret(&mut s, &editor, 0);
    s.cx.simulate_input("z");
    s.draw();
    let hover = hover_at(&mut s, &editor, 0).expect("an answer");
    assert!(hover.text.contains("matches no files"), "{}", hover.text);
    assert!(walks() > count);
}

#[gpui::test]
fn the_list_is_capped_and_asks_again_for_what_fits(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    for ix in 0..MAX_ENTRIES + 5 {
        write(&s, &format!("many/f{ix:03}.txt"), "");
    }
    write(&s, "many/zebra.txt", "");
    let editor = open_ignore(&mut s, ".gitignore", "many/");
    let reply = s.cx.update(|_, cx| {
        let view = editor.read(cx);
        let provider = view.completion_provider().unwrap().clone();
        let snapshot = view.model().snapshot();
        let offset = view.model().primary_selection().head();
        provider.completions(
            snapshot.id(),
            snapshot.version(),
            offset,
            CompletionTrigger::Invoked,
        )
    });
    let list = answer(&mut s, reply).expect("a list");
    assert_eq!(list.items.len(), MAX_ENTRIES);
    assert!(list.is_incomplete);
    assert!(!list.items.iter().any(|item| item.label == "zebra.txt"));

    // What's typed reaches past the cap.
    set_text(&mut s, &editor, "many/zeb");
    assert_eq!(labels(&offered(&mut s, &editor)), ["zebra.txt"]);
}

#[gpui::test]
fn git_dir_is_not_offered_and_exclude_bases_at_the_worktree(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write(&s, ".git/info/exclude", "");
    write(&s, "zebra.txt", "");
    let exclude = open_ignore(&mut s, ".git/info/exclude", "zeb");
    assert_eq!(labels(&offered(&mut s, &exclude)), ["zebra.txt"]);

    let editor = open_ignore(&mut s, ".gitignore", "");
    let items = offered(&mut s, &editor);
    assert!(
        items.iter().any(|item| item.label == "zebra.txt"),
        "{items:?}"
    );
    assert!(
        !items.iter().any(|item| item.label.starts_with(".git/")),
        "{items:?}"
    );
    set_text(&mut s, &editor, ".git/");
    assert_eq!(labels(&offered(&mut s, &editor)), [] as [&str; 0]);
}

#[cfg(unix)]
#[gpui::test]
fn symlinks_are_not_followed_or_shown_as_directories(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write(&s, "real/inner.txt", "");
    std::os::unix::fs::symlink(s.path("real"), s.path("link")).unwrap();
    let editor = open_ignore(&mut s, ".gitignore", "li");
    assert_eq!(labels(&offered(&mut s, &editor)), ["link"]);
    set_text(&mut s, &editor, "link/");
    assert_eq!(labels(&offered(&mut s, &editor)), [] as [&str; 0]);
}

#[test]
fn typed_paths_name_a_directory_or_nothing() {
    let typed = |line: &str| typed_path(line, line.len());
    let path = |dirs: &[&str], entry: &str, range| TypedPath {
        dirs: dirs.iter().map(|&dir| dir.to_owned()).collect(),
        entry: entry.to_owned(),
        range,
    };
    assert_eq!(typed(""), Some(path(&[], "", 0..0)));
    assert_eq!(typed("src/"), Some(path(&["src"], "", 4..4)));
    assert_eq!(typed("src/ma"), Some(path(&["src"], "ma", 4..6)));
    assert_eq!(typed("!/src/a"), Some(path(&["src"], "a", 6..7)));
    assert_eq!(typed("my\\ dir/x"), Some(path(&["my dir"], "x", 8..9)));
    assert_eq!(typed("\\#a"), Some(path(&[], "#a", 0..3)));
    // Git reads none of these as naming a directory.
    for line in [
        "# src/", "*.log", "src/*", "**/a", "a[b]/", "a?/", "../a", "./a", "a/../b", "a//b", "//a",
        ".git/", "a/.git/b", "a\\",
    ] {
        assert_eq!(typed(line), None, "{line}");
    }
    // The caret mid-line: what's before it, on a line that isn't a comment.
    assert_eq!(typed_path("src/main.rs", 4), Some(path(&["src"], "", 4..4)));
    assert_eq!(typed_path("# src/", 0), None);
}

/// What's inserted matches the entry, by the check's own matcher, as git's would.
#[test]
fn escaped_names_match_literally() {
    let dir = tempfile::tempdir().unwrap();
    // What `st*r` would also match, read as a glob.
    fs::write(dir.path().join("start"), "").unwrap();
    for name in [
        "#hash",
        "!bang",
        "st*r",
        "wh?t",
        "br[ack",
        "back\\slash",
        "end ",
        "a b",
    ] {
        fs::write(dir.path().join(name), "").unwrap();
        let pattern = escaped(name, true);
        let report = check(dir.path(), &format!("{pattern}\n"));
        assert_eq!(report.matches[0].total, 1, "{name:?} as {pattern:?}");
        assert_eq!(report.matches[0].paths, [Path::new(name)], "{pattern:?}");
    }
    // Away from the line's start, `#` and `!` need nothing.
    assert_eq!(escaped("#hash", false), "#hash");
    assert_eq!(escaped("!bang", false), "!bang");
}

/// A walk cut short can't say how many there are, or that there are none.
#[test]
fn a_truncated_walk_claims_no_exact_count() {
    let none = describe(0, &[], true);
    assert_eq!(
        none,
        "This pattern matches no files in the part of the project read"
    );
    let some = describe(3, &["a".to_owned()], true);
    assert!(some.contains("at least 3 files"), "{some}");
    assert!(some.ends_with("and 2 more"), "{some}");
    let one = describe(1, &["a".to_owned()], true);
    assert!(one.contains("at least 1 file:"), "{one}");
}

/// A path can't close the fence it's listed in.
#[test]
fn the_fence_outlasts_backticks_in_a_path() {
    let text = describe(1, &["a````b".to_owned()], false);
    assert!(text.contains("\n`````\na````b\n`````"), "{text}");
}

fn check(dir: &Path, text: &str) -> fe_project::IgnoreReport {
    check_ignore_file(&dir.join(".gitignore"), text, &AtomicBool::new(false))
}
