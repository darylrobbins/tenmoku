use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use super::{parse, render};
use crate::*;

#[test]
fn split_shares_text_history_and_dirty_state() {
    let dir = std::env::temp_dir().join(format!("fastedit-split-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shared.txt");
    std::fs::write(&path, "one\ntwo").unwrap();

    let mut a = EditorModel::open(&path).unwrap();
    let mut b = a.split();
    let unrelated = parse("one\ntwo");
    assert!(a.shares_buffer_with(&b) && b.shares_buffer_with(&a));
    assert!(!a.shares_buffer_with(&unrelated) && !unrelated.shares_buffer_with(&a));
    assert_eq!(b.path(), Some(path.as_path()));

    a.set_cursor(7);
    a.insert("!");
    b.sync_display();
    assert_eq!(b.text().to_string(), "one\ntwo!");
    assert!(a.is_dirty() && b.is_dirty());

    // Undo acts on the shared history, whichever model made the edit.
    b.undo().unwrap();
    a.sync_display();
    assert_eq!(a.text().to_string(), "one\ntwo");
    assert_eq!(b.text().to_string(), "one\ntwo");
    a.insert("?");
    assert!(b.is_dirty());

    a.save().unwrap();
    assert!(!a.is_dirty() && !b.is_dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo?");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_other_models_carets_follow_edits() {
    let mut a = parse("foo| bar\nbaz|");
    let mut b = a.split();
    assert_eq!(render(&b), "foo| bar\nbaz|");
    a.set_cursor(0);
    a.insert("0123456789");
    // No sync: b catches up before computing its edit.
    b.insert("x");
    assert_eq!(render(&b), "0123456789foox| bar\nbazx|");
    a.sync_display();
    assert_eq!(render(&a), "0123456789|foox bar\nbazx");
}

#[test]
fn display_and_folds_stay_per_model() {
    let mut a = parse("top|\ndef a():\n    one\n    two\nend");
    let mut b = a.split();
    a.fold_rows(1..=3);
    assert_eq!(a.display().folds(), [fe_display::Fold { start: 1, end: 3 }]);
    assert_eq!(b.display().folds(), []);
    assert_eq!(b.display().row_count(), 5);

    a.set_cursor(0);
    a.insert("1\n2\n3\n4\n5\n");
    let fold = a.display().folds()[0];
    assert_eq!(fold, fe_display::Fold { start: 6, end: 8 });
    let text = a.text();
    let folded =
        text.row_start(fold.start)..text.row_start(fold.end) + text.row_len(fold.end) as usize;
    assert_eq!(text.slice_to_string(folded), "def a():\n    one\n    two");

    b.sync_display();
    assert_eq!(b.display().folds(), []);
    assert_eq!(b.text().row_count(), 10);
    assert_eq!(b.display().row_count(), b.text().row_count());
}

#[test]
fn a_quiet_model_holds_no_events() {
    let mut a = parse("|");
    let mut b = a.split();
    for _ in 0..1_000 {
        a.insert("x");
    }
    b.sync_display();
    assert!(b.events.drain().is_empty());
    assert_eq!(b.snapshot.version(), b.buffer().version());
    assert_eq!(b.text().len(), 1_000);
}

#[test]
fn rows_given_before_catching_up_mean_the_rows_shown() {
    let mut a = parse("|top\ndef a():\n    one\n    two\nend");
    let mut b = a.split();
    a.insert("1\n2\n3\n4\n5\n");
    // b still shows the old text, so a gutter click there names its rows.
    b.fold_rows(1..=3);
    b.sync_display();
    assert_eq!(b.display().folds(), [fe_display::Fold { start: 6, end: 8 }]);
}

#[test]
fn reads_agree_until_the_model_catches_up() {
    let mut a = parse("foo|");
    let mut b = a.split();
    a.set_cursor(0);
    a.insert("0123456789");
    // b's text, selections and copy all show the buffer before a's edit.
    assert_eq!(render(&b), "foo|");
    assert_eq!(b.primary_selection().head(), 3);
    assert_eq!(b.copy_text(), ("foo\n".to_string(), true));
    b.sync_display();
    assert_eq!(render(&b), "0123456789foo|");
    assert_eq!(b.copy_text(), ("0123456789foo\n".to_string(), true));
}

#[test]
fn commands_that_find_their_own_rows_catch_up() {
    let mut a = parse("|");
    let mut b = a.split();
    a.insert("x");
    b.select_all();
    b.insert("y");
    assert_eq!(b.text().to_string(), "y");

    a.sync_display();
    a.select_all();
    a.insert("def a():\n    one\nend");
    b.fold_all([]);
    assert_eq!(b.display().folds(), [fe_display::Fold { start: 0, end: 1 }]);
}

#[test]
fn fold_candidates_name_the_rows_shown() {
    let mut a = parse("|top\ndef a():\n    one\n    two\nend");
    let mut b = a.split();
    b.set_cursor(5);
    a.insert("1\n2\n3\n4\n5\n");
    // Backed by b's text before a's edit, as a syntax tree matched to
    // b.snapshot() would be.
    b.fold(&|row| (row == 1).then_some(1..=2));
    assert_eq!(b.display().folds(), [fe_display::Fold { start: 6, end: 7 }]);
    assert_eq!(
        render(&b),
        "1\n2\n3\n4\n5\ntop\nd|ef a():\n    one\n    two\nend"
    );
}

#[test]
fn fold_finds_indented_blocks_in_the_current_text() {
    let mut a = parse("|");
    let mut b = a.split();
    a.insert("def a():\n    one\nend");
    b.fold(&|_| None);
    assert_eq!(b.display().folds(), [fe_display::Fold { start: 0, end: 1 }]);
}

/// Measures like [`Monospace`], borrowing the shared buffer mutably on every
/// char, as a width that edited through another model would.
struct BorrowsTheBuffer {
    buffer: Rc<RefCell<Buffer>>,
    /// Chars measured, and how many of those found the buffer borrowed.
    counts: Rc<Cell<(usize, usize)>>,
}

impl CharWidth for BorrowsTheBuffer {
    fn char_width(&mut self, _: char) -> f32 {
        let (measured, refused) = self.counts.get();
        let refused = refused + usize::from(self.buffer.try_borrow_mut().is_err());
        self.counts.set((measured + 1, refused));
        1.
    }
}

#[test]
fn catching_up_holds_no_borrow_while_measuring() {
    let mut a = parse("|");
    let mut b = a.split();
    let counts = Rc::new(Cell::new((0, 0)));
    b.set_char_widths(Box::new(BorrowsTheBuffer {
        buffer: b.buffer.clone(),
        counts: counts.clone(),
    }));
    b.set_wrap_width(4.);
    b.set_soft_wrap(true);

    // ASCII widths are measured once; other chars every time.
    a.insert("ąąąą bbbb");
    b.sync_display();
    let (measured, refused) = counts.get();
    assert!(measured > 0);
    assert_eq!(refused, 0);

    b.buffer_mut().edit([(0..0, "ćććć ")], Origin::User);
    assert!(counts.get().0 > measured);
    assert_eq!(counts.get().1, 0);
    assert_eq!(b.display().row_count(), 3);
}

/// An edit for [`EditsOnce`] to make.
type Hook = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

/// Measures like [`Monospace`], but the first non-ASCII char measured while
/// an edit is hooked makes it, as a width that edited through another model
/// would.
struct EditsOnce(Hook);

impl CharWidth for EditsOnce {
    fn char_width(&mut self, ch: char) -> f32 {
        let edit = if ch.is_ascii() {
            None
        } else {
            self.0.borrow_mut().take()
        };
        if let Some(edit) = edit {
            edit();
        }
        1.
    }
}

/// `a`, shared so a width can edit through it, and a split of it whose
/// widths run the edits hooked on the returned hook.
fn split_with_hook(a: EditorModel) -> (Rc<RefCell<EditorModel>>, EditorModel, Hook) {
    let a = Rc::new(RefCell::new(a));
    let mut b = a.borrow().split();
    let hook = Hook::default();
    b.set_char_widths(Box::new(EditsOnce(hook.clone())));
    // Wrapping is what measures; nothing is long enough to wrap.
    b.set_soft_wrap(true);
    (a, b, hook)
}

/// Hooks `a` inserting `text` at its carets.
fn hook_insert(hook: &Hook, a: &Rc<RefCell<EditorModel>>, text: &'static str) {
    let a = a.clone();
    *hook.borrow_mut() = Some(Box::new(move || a.borrow_mut().insert(text)));
}

#[test]
fn edits_made_while_catching_up_are_caught_up_with() {
    let (a, mut b, hook) = split_with_hook(parse("|foo"));
    b.set_cursor(3);
    a.borrow_mut().insert("ą");
    // Measuring "ą" while b catches up puts "Z" after it.
    hook_insert(&hook, &a, "Z");
    b.insert("X");
    assert_eq!(render(&b), "ąZfooX|");

    // The same when an edit through the buffer ends.
    a.borrow_mut().insert("ć");
    hook_insert(&hook, &a, "W");
    let len = b.buffer().len();
    b.buffer_mut().edit([(len..len, "!")], Origin::User);
    assert_eq!(b.snapshot().version(), b.buffer().version());
    assert_eq!(b.text().to_string(), "ąZćWfooX!");
    assert_eq!(b.display().row(0).range, 0..b.text().len());
    assert!(b.events.drain().is_empty());
}

/// Hooks an edit through `a`'s buffer, not as the user, so it leaves the
/// user's undo alone.
fn hook_edit(hook: &Hook, a: &Rc<RefCell<EditorModel>>, offset: usize, text: &'static str) {
    let a = a.clone();
    *hook.borrow_mut() = Some(Box::new(move || {
        a.borrow_mut()
            .buffer_mut()
            .edit([(offset..offset, text)], Origin::Lsp);
    }));
}

#[test]
fn carets_follow_edits_made_while_catching_up() {
    let (a, mut b, hook) = split_with_hook(parse("ą|foo"));
    b.set_cursor(5);
    // Measuring "ąfooX" after b's edit puts "Z" before its caret.
    hook_edit(&hook, &a, 2, "Z");
    b.insert("X");
    assert_eq!(render(&b), "ąZfooX|");

    // Likewise measuring "ąZfoo" after undoing "X".
    hook_edit(&hook, &a, 2, "Y");
    b.undo().unwrap();
    assert_eq!(render(&b), "ąYZfoo|");
}

#[test]
fn counted_moves_start_from_the_caught_up_carets() {
    let mut a = parse("foo|");
    let mut b = a.split();
    a.set_cursor(0);
    a.insert("x");
    // b shows its caret at 3, and is at 4 once caught up: one move left
    // lands back on 3, which mustn't look like a move that went nowhere.
    let invocation = fe_command::Invocation::of("editor.move")
        .unwrap()
        .args(serde_json::json!({"motion": "left", "count": 2}))
        .context(commands::context());
    let mut host = commands::HeadlessHost::default();
    commands::registry()
        .dispatch(
            &mut commands::EditorTarget::new(&mut b, &mut host),
            &invocation,
        )
        .unwrap();
    assert_eq!(render(&b), "xf|oo");
}

#[test]
fn fold_finds_the_selected_rows_in_the_current_text() {
    let mut a = parse("[top\n    one\n]    two\nend");
    let mut b = a.split();
    a.set_cursor(4);
    a.insert("    extra\n");
    b.fold(&|_| None);
    assert_eq!(b.display().folds(), [fe_display::Fold { start: 0, end: 2 }]);

    // A selection's start stays after text inserted at it.
    let mut a = parse("top\n[def] a():\n    one\nend");
    let mut b = a.split();
    a.set_cursor(4);
    a.insert("x\n");
    b.fold(&|_| None);
    assert_eq!(b.display().folds(), [fe_display::Fold { start: 2, end: 3 }]);
}

#[test]
fn catching_up_on_edits_around_a_fold_keeps_it() {
    for through_buffer in [false, true] {
        let mut a = parse("|top\ndef a():\n    one\n    two\nend");
        let mut b = a.split();
        b.fold_rows(1..=3);
        a.insert("above\n");
        a.set_cursor(a.text().len());
        a.insert("!");
        if through_buffer {
            b.buffer_mut().edit([(0..0, "#")], Origin::User);
        } else {
            b.sync_display();
        }
        assert_eq!(
            b.display().folds(),
            [fe_display::Fold { start: 2, end: 4 }],
            "through_buffer: {through_buffer}"
        );

        // An edit inside it still unfolds it.
        a.sync_display();
        a.set_cursor(0);
        a.insert("x\n");
        a.set_cursor(a.text().row_start(5));
        a.insert("x");
        b.sync_display();
        assert_eq!(b.display().folds(), []);
    }
}

#[test]
fn batched_catch_up_folds_as_catching_up_after_each_edit_does() {
    type Edit = fn(&mut EditorModel);
    let cases: [(&str, Edit); 8] = [
        ("cut the row above", |a| {
            a.set_cursor(0);
            a.cut();
        }),
        ("replace the row above", |a| a.select_range(0..4, false)),
        ("join the row above", |a| a.select_range(3..4, false)),
        ("rename the header", |a| a.select_range(8..9, false)),
        ("split the header", |a| {
            a.set_cursor(4);
            a.insert("\n# c");
        }),
        ("replace across the header's start", |a| {
            a.select_range(0..6, false)
        }),
        ("prefix the header", |a| {
            a.set_cursor(4);
            a.insert("# ");
        }),
        ("edit a hidden row", |a| {
            a.set_cursor(15);
            a.insert("!");
        }),
    ];
    for (name, edit) in cases {
        let mut a = parse("top\ndef a():\n    one\n    two\nend");
        let (mut batched, mut each) = (a.split(), a.split());
        batched.fold_rows(1..=3);
        each.fold_rows(1..=3);
        edit(&mut a);
        if !a.selections()[0].range.is_empty() {
            a.insert("X");
        }
        each.sync_display();
        // And an edit below the fold, so there are two batches to take in.
        a.set_cursor(a.text().len());
        a.insert("!");
        each.sync_display();
        batched.sync_display();
        assert_eq!(batched.display().folds(), each.display().folds(), "{name}");
    }
}
