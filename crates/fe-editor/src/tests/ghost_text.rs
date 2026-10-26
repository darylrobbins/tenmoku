//! [`GhostText`]: a suggestion at the caret that typing its prefix
//! consumes, that edits elsewhere leave alone, and that accepting, whole,
//! by word or by line, inserts as the user's own undoable edit.

use super::*;
use crate::ghost_text::GhostText;

fn ai() -> Origin {
    Origin::Ai {
        agent: "a".into(),
        session: "s".into(),
    }
}

/// `text` suggested by an AI at `model`'s caret now.
fn ghost(model: &EditorModel, text: &str) -> GhostText {
    GhostText::new(
        model.snapshot(),
        model.primary_selection().head(),
        text,
        ai(),
    )
}

/// Accepting through every entry point changes nothing.
fn accepts_nothing(ghost: &GhostText, model: &mut EditorModel) {
    let (text, version) = (model.text().to_string(), model.snapshot().version());
    assert_eq!(ghost.accept(model), None);
    assert_eq!(ghost.accept_word(model), None);
    assert_eq!(ghost.accept_line(model), None);
    assert_eq!(model.text().to_string(), text);
    assert_eq!(model.snapshot().version(), version);
}

#[test]
fn typing_consumes_the_prefix() {
    let mut model = parse("let x = |;\n");
    let g = ghost(&model, "foo()");
    assert_eq!(g.text(), "foo()");
    assert_eq!(g.origin(), &ai());
    assert_eq!(g.remaining(&model), Some("foo()"));
    model.insert("f");
    assert_eq!(g.remaining(&model), Some("oo()"));
    model.insert("o");
    assert_eq!(g.remaining(&model), Some("o()"));
    // Deleting back toward the anchor shows more again.
    model.backspace();
    assert_eq!(g.remaining(&model), Some("oo()"));
    assert!(g.is_live(&model));
    // A char that doesn't match ends it, and deleting it doesn't matter.
    model.insert("x");
    assert_eq!(g.remaining(&model), None);
    assert!(!g.is_live(&model));
    accepts_nothing(&g, &mut model);

    // Typing all of it ends it too: nothing is left to show.
    let mut model = parse("|\n");
    let g = ghost(&model, "ab");
    model.insert("ab");
    assert_eq!(g.remaining(&model), None);

    // Multi-byte chars are consumed whole.
    let mut model = parse("|\n");
    let g = ghost(&model, "é😀z");
    model.insert("é");
    assert_eq!(g.remaining(&model), Some("😀z"));
    model.insert("😀");
    assert_eq!(g.remaining(&model), Some("z"));
}

#[test]
fn survives_other_edits() {
    let mut model = parse("fn a() {}\nlet x = |;\nfn b() {}\n");
    let g = ghost(&model, "foo()");
    model.insert("f");
    // An AI edits before the anchor and after the caret.
    model.buffer_mut().edit([(0..0, "// generated\n")], ai());
    assert_eq!(g.remaining(&model), Some("oo()"));
    let end = model.text().len();
    model.buffer_mut().edit([(end..end, "fn c() {}\n")], ai());
    let caret = model.primary_selection().head();
    model
        .buffer_mut()
        .edit([(caret + 1..caret + 1, " // x")], ai());
    assert_eq!(g.remaining(&model), Some("oo()"));
    assert_eq!(
        render(&model),
        "// generated\nfn a() {}\nlet x = f|; // x\nfn b() {}\nfn c() {}\n"
    );
    // Refreshed, it stays where it was.
    let mut g = g;
    g.refresh(model.snapshot());
    assert_eq!(g.version(), model.snapshot().version());
    assert_eq!(g.remaining(&model), Some("oo()"));
    // Accepting inserts at the shifted caret.
    assert!(g.accept(&mut model).is_some());
    assert_eq!(
        render(&model),
        "// generated\nfn a() {}\nlet x = foo()|; // x\nfn b() {}\nfn c() {}\n"
    );
    assert_eq!(g.remaining(&model), None);
}

#[test]
fn accept_is_one_user_undo_step() {
    let mut model = parse("fn main() {\n    |\n}\n");
    let g = ghost(&model, "let x = 1;\n    let y = 2;\n    x + y");
    model.insert("l");
    model.insert("e");
    let typed = render(&model);
    assert_eq!(typed, "fn main() {\n    le|\n}\n");
    let id = g.accept(&mut model);
    assert!(id.is_some());
    assert_eq!(
        render(&model),
        "fn main() {\n    let x = 1;\n    let y = 2;\n    x + y|\n}\n"
    );
    assert_eq!(g.remaining(&model), None);
    // The user's undo takes back exactly the accepted text, caret and all.
    assert_eq!(model.undo().unwrap(), id);
    assert_eq!(render(&model), typed);
    assert_eq!(
        g.remaining(&model),
        Some("t x = 1;\n    let y = 2;\n    x + y")
    );
    // Then the typing, as one step.
    model.undo().unwrap();
    assert_eq!(render(&model), "fn main() {\n    |\n}\n");
    // Redo puts it back, the caret after it.
    model.redo().unwrap();
    model.redo().unwrap();
    assert_eq!(
        render(&model),
        "fn main() {\n    let x = 1;\n    let y = 2;\n    x + y|\n}\n"
    );

    // Accepted inside a command run as an AI, it's still the user's.
    let mut model = parse("|\n");
    let g = ghost(&model, "abc");
    let (id, produced) = model.run_as(ai(), |model| g.accept(model));
    assert!(id.is_some());
    assert_eq!(produced, [id.unwrap()]);
    assert_eq!(model.undo().unwrap(), id);
    assert_eq!(render(&model), "|\n");

    // Inside an open user transaction, as vim's insert session holds, it
    // joins that one.
    let mut model = parse("|\n");
    let g = ghost(&model, "hello");
    let open = model.buffer_mut().start_transaction(Origin::User);
    model.insert("h");
    assert_eq!(g.accept(&mut model), Some(open));
    assert_eq!(render(&model), "hello|\n");
    assert_eq!(model.buffer_mut().end_transaction(), Some(open));
    assert_eq!(model.undo().unwrap(), Some(open));
    assert_eq!(render(&model), "|\n");
}

#[test]
fn accept_word_and_line() {
    let mut model = parse("|\n");
    let g = ghost(&model, "foo(bar, baz)\n    qux");
    let mut accepted = String::new();
    // A word, then each with the punctuation or spaces before it.
    for (word, rest) in [
        ("foo", "(bar, baz)\n    qux"),
        ("(bar", ", baz)\n    qux"),
        (", baz", ")\n    qux"),
        // Stopping before a line break...
        (")", "\n    qux"),
    ] {
        accepted.push_str(word);
        assert!(g.accept_word(&mut model).is_some());
        assert_eq!(render(&model), format!("{accepted}|\n"));
        assert_eq!(g.remaining(&model), Some(rest));
    }
    // ...unless what remains starts with one.
    assert!(g.accept_word(&mut model).is_some());
    assert_eq!(render(&model), "foo(bar, baz)\n    qux|\n");
    assert_eq!(g.remaining(&model), None);
    // Each word was its own undo step.
    model.undo().unwrap();
    assert_eq!(render(&model), "foo(bar, baz)|\n");
    assert_eq!(g.remaining(&model), Some("\n    qux"));

    let mut model = parse("x|\n");
    let g = ghost(&model, "a b\nc d\ne");
    assert!(g.accept_line(&mut model).is_some());
    assert_eq!(render(&model), "xa b\n|\n");
    assert_eq!(g.remaining(&model), Some("c d\ne"));
    // What's typed meanwhile still consumes it.
    model.insert("c");
    assert!(g.accept_line(&mut model).is_some());
    assert_eq!(render(&model), "xa b\nc d\n|\n");
    assert_eq!(g.remaining(&model), Some("e"));
    // The last line has no break: all of it.
    assert!(g.accept_line(&mut model).is_some());
    assert_eq!(render(&model), "xa b\nc d\ne|\n");
    assert_eq!(g.remaining(&model), None);
    model.undo().unwrap();
    assert_eq!(render(&model), "xa b\nc d\n|\n");
}

#[test]
fn ends_without_editing() {
    // A caret left of the anchor.
    let mut model = parse("ab|c\n");
    let g = ghost(&model, "xyz");
    model.move_cursors(Movement::Left, false);
    assert_eq!(g.remaining(&model), None);
    accepts_nothing(&g, &mut model);
    // Back at it, it shows again.
    model.move_cursors(Movement::Right, false);
    assert_eq!(g.remaining(&model), Some("xyz"));

    // A second caret.
    model.set_selections([caret(0), caret(2)]);
    assert_eq!(g.remaining(&model), None);
    accepts_nothing(&g, &mut model);

    // A selection, even one ending at the anchor.
    model.set_selections([selection_from(1, 2)]);
    assert_eq!(g.remaining(&model), None);
    accepts_nothing(&g, &mut model);

    // Another buffer.
    let mut other = parse("ab|c\n");
    assert_eq!(g.remaining(&other), None);
    accepts_nothing(&g, &mut other);

    // An anchor below the log's floor.
    let mut model = parse("|\n");
    let g = ghost(&model, "foo");
    let mut refreshed = g.clone();
    model.insert("f");
    assert_eq!(g.remaining(&model), Some("oo"));
    refreshed.refresh(model.snapshot());
    let floor = model.snapshot().version();
    model.buffer_mut().compact_log(floor);
    // The model sees the floor once it catches up with a later edit.
    model.insert("o");
    assert!(g.version() < model.snapshot().oldest_version());
    assert_eq!(g.remaining(&model), None);
    accepts_nothing(&g, &mut model);
    // Refreshing past it is a no-op; one refreshed first stays live.
    let mut stale = g.clone();
    stale.refresh(model.snapshot());
    assert_eq!(stale.remaining(&model), None);
    assert_eq!(refreshed.remaining(&model), Some("o"));
}

#[test]
fn crlf_and_lone_cr_follow_the_buffers_lf() {
    for offered in ["a\r\nb", "a\rb"] {
        let mut model = parse("|Z");
        let g = ghost(&model, offered);
        assert_eq!(g.text(), offered);
        assert_eq!(g.remaining(&model), Some("a\nb"));
        // Typing consumes it through the line break.
        model.insert("a\n");
        assert_eq!(g.remaining(&model), Some("b"));
        model.backspace();
        model.backspace();
        assert_eq!(g.remaining(&model), Some("a\nb"));

        // Partial acceptance keeps the rest live, caret before Z.
        assert!(g.accept_line(&mut model).is_some());
        assert_eq!(model.text().to_string(), "a\nZ");
        assert_eq!(model.primary_selection().head(), 2);
        assert_eq!(g.remaining(&model), Some("b"));
        assert!(g.accept(&mut model).is_some());
        assert_eq!(model.text().to_string(), "a\nbZ");
        assert_eq!(model.primary_selection().head(), 3);

        // Whole acceptance.
        let mut model = parse("|Z");
        let g = ghost(&model, offered);
        assert!(g.accept(&mut model).is_some());
        assert_eq!(model.text().to_string(), "a\nbZ");
        assert_eq!(model.primary_selection().head(), 3);
    }
}
