//! Container ignore files get the ignore-file help, checked with Docker's rules.

use std::{cell::RefCell, fs, future::Future, pin::Pin, rc::Rc};

use fe_editor::completion::{CompletionSession, CompletionTrigger};
use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext};

use crate::tests::lsp::Lsp;

fn write(s: &Lsp, name: &str, text: &str) {
    let path = s.path(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// `name`, written with `text`, open and focused, the caret at its end.
fn open_at_end(s: &mut Lsp, name: &str, text: &str) -> Entity<EditorView> {
    write(s, name, text);
    s.open(name);
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("the file is shown");
    s.cx.update(|window, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(text.len());
            cx.notify();
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    editor
}

fn answer<T: 'static>(s: &mut Lsp, future: Pin<Box<dyn Future<Output = T>>>) -> T {
    let slot = Rc::new(RefCell::new(None));
    let filled = slot.clone();
    s.cx.update(|_, cx| {
        cx.spawn(async move |_| {
            *filled.borrow_mut() = Some(future.await);
        })
        .detach();
    });
    s.until("the answer", |_| slot.borrow().is_some());
    slot.take().unwrap()
}

/// The labels and edits offered at the caret.
fn offered(s: &mut Lsp, editor: &Entity<EditorView>) -> Vec<(String, String)> {
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
    let mut items: Vec<_> = (session.matches().iter())
        .map(|&ix| {
            let item = &session.items()[ix];
            (item.label.clone(), item.edit.1.clone())
        })
        .collect();
    items.sort();
    items
}

fn problem_count(s: &Lsp, editor: &Entity<EditorView>) -> usize {
    editor.read_with(&s.cx, |view, _| view.model().problems().len())
}

#[gpui::test]
fn diagnostics_and_completion_cover_dockerignore(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    write(&s, "assets/logo.png", "");
    write(&s, "assets/icons/a.svg", "");

    // Diagnostics: a pattern matching nothing, and Docker's own error.
    let editor = open_at_end(&mut s, ".dockerignore", "missing.log\n[abc\n");
    s.draw();
    s.cx.executor()
        .advance_clock(crate::ignore_diagnostics::DEBOUNCE);
    s.until("the diagnostics", |s| problem_count(s, &editor) == 2);

    // Completion in each container file: the context's children, with no
    // leading slash inserted, and boundary whitespace in a class.
    write(&s, "padded ", "");
    write(&s, " lead", "");
    for name in [
        ".dockerignore",
        "Dockerfile.dockerignore",
        ".containerignore",
    ] {
        let editor = open_at_end(&mut s, name, "");
        let items = offered(&mut s, &editor);
        for want in [
            ("assets/", "assets/"),
            ("padded ", "padded[ ]"),
            (" lead", "[ ]lead"),
        ] {
            let want = (want.0.to_owned(), want.1.to_owned());
            assert!(items.contains(&want), "{name}: {want:?} in {items:?}");
        }
    }
    // After trimmed whitespace and a `!`, by what the whole line then means.
    write(&s, "#hash", "");
    for (name, text, want) in [
        ("a.dockerignore", " ", ("#hash", "\\#hash")),
        ("b.containerignore", "! ", ("#hash", "#hash")),
        ("app.dockerignore", "! ", (" lead", "[ ]lead")),
        ("web.dockerignore", " ", (" lead", "[ ]lead")),
        ("x.containerignore", "/", (" lead", " lead")),
    ] {
        let editor = open_at_end(&mut s, name, text);
        let want = (want.0.to_owned(), want.1.to_owned());
        let items = offered(&mut s, &editor);
        assert!(
            items.contains(&want),
            "{name} {text:?}: {want:?} in {items:?}"
        );
    }
    let editor = open_at_end(&mut s, ".dockerignore", "");
    s.cx.simulate_input(" /assets/");
    s.draw();
    assert_eq!(
        offered(&mut s, &editor),
        [
            ("icons/".to_owned(), "icons/".to_owned()),
            ("logo.png".to_owned(), "logo.png".to_owned()),
        ]
    );

    // A Rust buffer gets no diagnostics.
    let editor = open_at_end(&mut s, "main.rs", "missing.log\n[abc\n");
    s.draw();
    assert_eq!(problem_count(&s, &editor), 0);
}
