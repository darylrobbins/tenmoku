//! [`CompletionSession`]: filtering by the word typed, a selection that
//! stays put while its item matches, and accepting an item with its
//! additional edits as one user transaction rebased over typing since the
//! request.

use super::*;
use crate::completion::{CompletionItem, CompletionList, CompletionSession};

fn item(label: &str, edit: (Range<usize>, &str)) -> CompletionItem {
    CompletionItem {
        label: label.into(),
        edit: (edit.0, edit.1.into()),
        ..CompletionItem::default()
    }
}

/// A session for `items`, asked for at `model`'s caret now.
fn session(model: &EditorModel, items: Vec<CompletionItem>) -> CompletionSession {
    let list = CompletionList {
        items,
        is_incomplete: false,
    };
    CompletionSession::new(
        model.snapshot().clone(),
        model.primary_selection().head(),
        list,
    )
}

fn labels(session: &CompletionSession) -> Vec<&str> {
    (session.matches().iter())
        .map(|&ix| session.items()[ix].label.as_str())
        .collect()
}

fn selected(session: &CompletionSession) -> &str {
    &session.items()[session.selected().unwrap()].label
}

#[test]
fn accept_is_one_user_transaction() {
    // Filtering, and the selection.
    let mut model = parse("use std;\nfn main() { fo| }\n");
    let fo = model.text().to_string().find("fo ").unwrap();
    let word = fo..fo + 2;
    let mut s = session(
        &model,
        ["format", "info", "forget", "bar", "fooBar", "foo"]
            .map(|label| item(label, (word.clone(), label)))
            .to_vec(),
    );
    // By how well they match `fo`: as a run from the start first, by
    // label among those, then `info`'s scattered match.
    assert_eq!(labels(&s), ["foo", "fooBar", "forget", "format", "info"]);
    assert_eq!(selected(&s), "foo");
    s.select_next(false);
    s.select_next(false);
    assert_eq!(selected(&s), "forget");
    s.select_next(true);
    assert_eq!(selected(&s), "fooBar");
    s.select_next(true);
    s.select_next(true);
    assert_eq!(selected(&s), "info");
    s.select_next(true);
    assert_eq!(selected(&s), "format");
    // Typing narrows; the selected item stays while it matches.
    model.insert("r");
    assert!(s.update(&model));
    assert_eq!(s.word(), "for");
    assert_eq!(labels(&s), ["forget", "format", "fooBar"]);
    assert_eq!(selected(&s), "format");
    // Fuzzy: `fogt` in `forget`, but not in `format`; the selection moves
    // to the first match.
    model.backspace();
    model.insert("gt");
    assert!(s.update(&model));
    assert_eq!(labels(&s), ["forget"]);
    assert_eq!(selected(&s), "forget");
    // Out of the word: over.
    model.insert(".");
    assert!(!s.update(&model));

    // Chars whose lower case is several match it, in either spelling, and
    // the chars after them still line up.
    let model = parse("İs|\n");
    let s = session(
        &model,
        ["İstanbul", "i\u{307}stanbul", "istanbul", "Straße"]
            .map(|label| item(label, (0..2, label)))
            .to_vec(),
    );
    let mut matched = labels(&s);
    matched.sort();
    assert_eq!(matched, ["i\u{307}stanbul", "İstanbul"]);
    let mut model = parse("|\n");
    let mut s = session(&model, vec![item("Straße", (0..0, "Straße"))]);
    model.insert("straẞ");
    assert!(s.update(&model));
    assert_eq!(labels(&s), ["Straße"]);

    // Accepting an item with an import, two chars typed since the
    // request.
    let mut model = parse("use std;\nfn main() { fo| }\n");
    let before = model.text().to_string();
    let mut foo_bar = item("fooBar", (word.clone(), "fooBar"));
    foo_bar.additional_edits = vec![(0..0, "use crate::fooBar;\n".into())];
    let mut s = session(
        &model,
        vec![foo_bar, item("format", (word.clone(), "format"))],
    );
    model.insert("o");
    model.insert("b");
    assert!(s.update(&model));
    assert_eq!(labels(&s), ["fooBar"]);
    let typed = model.text().to_string();
    let accepted = s.accept(&mut model).unwrap();
    assert!(accepted.is_some());
    assert_eq!(
        render(&model),
        "use crate::fooBar;\nuse std;\nfn main() { fooBar| }\n"
    );
    // One undo takes the whole completion back, and only it, leaving the
    // caret where it was.
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), typed);
    assert_eq!(render(&model), "use std;\nfn main() { foob| }\n");
    // And redoing puts it after the completion again.
    model.redo().unwrap();
    assert_eq!(
        render(&model),
        "use crate::fooBar;\nuse std;\nfn main() { fooBar| }\n"
    );
    model.undo().unwrap();
    assert_eq!(render(&model), "use std;\nfn main() { foob| }\n");
    model.undo().unwrap();
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), before);
    model.redo().unwrap();
    model.redo().unwrap();
    model.redo().unwrap();
    assert_eq!(
        model.text().to_string(),
        "use crate::fooBar;\nuse std;\nfn main() { fooBar }\n"
    );

    // Nothing typed since: the caret goes back alone too.
    let mut model = parse("use std;\nfn main() { fo| }\n");
    let mut foo = item("foo", (word.clone(), "foo"));
    foo.additional_edits = vec![(0..0, "use crate::foo;\n".into())];
    session(&model, vec![foo]).accept(&mut model).unwrap();
    assert_eq!(
        render(&model),
        "use crate::foo;\nuse std;\nfn main() { foo| }\n"
    );
    model.undo().unwrap();
    assert_eq!(render(&model), "use std;\nfn main() { fo| }\n");

    // With typing in between, and two completions: each undo still puts
    // one caret back.
    let mut model = parse("fn main() { fo| }\n");
    let fo = 12..14;
    let mut foo = item("foo", (fo.clone(), "foo"));
    foo.additional_edits = vec![(0..0, "use a::foo;\n".into())];
    session(&model, vec![foo]).accept(&mut model).unwrap();
    model.insert(" + ba");
    let caret = model.primary_selection().head();
    let mut bar = item("bar", (caret - 2..caret, "bar"));
    bar.additional_edits = vec![(0..0, "use b::bar;\n".into())];
    session(&model, vec![bar]).accept(&mut model).unwrap();
    model.insert("!");
    assert_eq!(
        render(&model),
        "use b::bar;\nuse a::foo;\nfn main() { foo + bar!| }\n"
    );
    model.undo().unwrap();
    model.undo().unwrap();
    assert_eq!(render(&model), "use a::foo;\nfn main() { foo + ba| }\n");
    model.undo().unwrap();
    model.undo().unwrap();
    assert_eq!(render(&model), "fn main() { fo| }\n");
    model.redo().unwrap();
    assert_eq!(render(&model), "use a::foo;\nfn main() { foo| }\n");

    // The caret goes back where it was, though that's no edit's end: a
    // replacement past the caret, or a caret within what was typed since.
    let mut model = parse("x.fo|bar\n");
    session(&model, vec![item("foobar", (2..7, "foobar"))])
        .accept(&mut model)
        .unwrap();
    assert_eq!(render(&model), "x.foobar|\n");
    model.undo().unwrap();
    assert_eq!(render(&model), "x.fo|bar\n");
    model.redo().unwrap();
    assert_eq!(render(&model), "x.foobar|\n");
    let mut model = parse("x.fo|\n");
    let s = session(&model, vec![item("foobar", (2..4, "foobar"))]);
    model.insert("ob");
    model.move_cursors(Movement::Left, false);
    assert_eq!(s.accept(&mut model).map(|id| id.is_some()), Ok(true));
    assert_eq!(render(&model), "x.foobar|\n");
    model.undo().unwrap();
    assert_eq!(render(&model), "x.foo|b\n");

    // From another view of the buffer, and past many completions.
    let mut model = parse("fo|\n");
    let mut other = model.split();
    let mut accepted = Vec::new();
    for n in 0..70 {
        if n > 0 {
            model.insert(" fo");
        }
        let caret = model.primary_selection().head();
        let mut foo = item("foo", (caret - 2..caret, "foo"));
        foo.additional_edits = vec![(0..0, format!("use m{n};\n"))];
        session(&model, vec![foo]).accept(&mut model).unwrap();
        accepted.push(render(&model));
    }
    for n in (0..70).rev() {
        other.undo().unwrap();
        let end = other.text().len() - 1;
        assert_eq!(other.selections(), [crate::caret(end)], "{n}");
        if n > 0 {
            other.undo().unwrap();
        }
    }
    assert_eq!(render(&other), "fo|\n");
    other.redo().unwrap();
    assert_eq!(render(&other), accepted[0]);

    // After a trigger character: an insertion at the caret, with what was
    // typed since replaced.
    let mut model = parse("v.|\n");
    let s = session(&model, vec![item("len", (2..2, "len()"))]);
    model.insert("le");
    assert!(s.accept(&mut model).unwrap().is_some());
    assert_eq!(render(&model), "v.len()|\n");
    model.undo().unwrap();
    assert_eq!(render(&model), "v.le|\n");

    // An edit over the rest of the word, past the caret.
    let mut model = parse("x.fo|bar\n");
    let s = session(&model, vec![item("foobar", (2..7, "foobar"))]);
    model.insert("o");
    s.accept(&mut model).unwrap();
    assert_eq!(render(&model), "x.foobar|\n");

    // An import at the main edit's start goes before it.
    let mut model = parse("Has|\n");
    let mut map = item("HashMap", (0..3, "HashMap"));
    map.additional_edits = vec![(0..0, "use std::collections::HashMap;\n".into())];
    session(&model, vec![map]).accept(&mut model).unwrap();
    assert_eq!(render(&model), "use std::collections::HashMap;\nHashMap|\n");

    // The text an edit replaces changed since: nothing is applied.
    let mut model = parse("x.fo|\n");
    let mut foo = item("foo", (2..4, "foo"));
    foo.additional_edits = vec![(0..1, "use x;\nx".into())];
    let s = session(&model, vec![foo]);
    model.buffer_mut().edit([(0..1, "y")], Origin::User);
    let text = model.text().to_string();
    assert!(s.accept(&mut model).is_err());
    assert_eq!(model.text().to_string(), text);
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), "x.fo\n");

    // Once the session is over, accepting does nothing: the caret moved
    // out of the word, or the trigger character was deleted.
    let mut model = parse("x.fo| + y\n");
    let mut s = session(&model, vec![item("foo", (2..4, "foo"))]);
    model.set_cursor(model.text().len() - 1);
    assert!(!s.update(&model));
    assert!(!s.may_ask_again(&model));
    assert_eq!(s.accept(&mut model), Ok(None));
    assert_eq!(model.text().to_string(), "x.fo + y\n");
    let mut model = parse("v.|\n");
    let mut s = session(&model, vec![item("len", (2..2, "len()"))]);
    model.backspace();
    assert!(!s.update(&model));
    assert!(!s.may_ask_again(&model));
    assert_eq!(s.accept(&mut model), Ok(None));
    assert_eq!(render(&model), "v|\n");
    // Cutting back the word asked about ends the session too, but the
    // caret is still in the word: ask again there.
    let mut model = parse("x.fo|\n");
    let mut s = session(&model, vec![item("foo", (2..4, "foo"))]);
    model.backspace();
    assert!(!s.update(&model));
    assert!(s.may_ask_again(&model));
    model.backspace();
    assert!(!s.may_ask_again(&model));
}
