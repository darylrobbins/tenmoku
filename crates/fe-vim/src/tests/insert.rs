use fe_buffer::{Buffer, Origin, ResolvedSelection};
use fe_command::{ContextPredicate, DispatchError, Effect, Invocation};
use fe_editor::{EditorModel, commands::HeadlessHost};

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use super::{Measuring, commit, invocation};
use crate::{
    CountArgs, HeadlessVim, InsertArgs, InsertAt, Mode, Register, Registers, VimState, VimTarget,
    Write, context, insert::COPIES_NOTED, registry,
};

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

/// The user's `u`, with the transactions it undid.
fn undo(vim: &mut HeadlessVim) -> usize {
    let invocation = Invocation::of("vim.undo")
        .unwrap()
        .context(context(vim.state()));
    vim.dispatch(&invocation).unwrap().transactions.len()
}

#[test]
fn session_is_one_transaction() {
    // The change and the typed text are one undo step.
    let mut vim = HeadlessVim::new("one two three", (0, 5));
    vim.feed("ciwXY<Esc>").unwrap();
    assert_eq!(vim.text(), "one XY three");
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "one two three");

    // The count's copies too.
    let mut vim = HeadlessVim::new("ab", (0, 1));
    vim.feed("3ix<Esc>").unwrap();
    assert_eq!(vim.text(), "axxxb");
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "ab");

    // `o` and two typed rows.
    let mut vim = HeadlessVim::new("  ab", (0, 2));
    vim.feed("oA<CR>B<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n  A\n  B");
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "  ab");

    // Another origin's edit during the session gets its own transaction,
    // and the user's `u` leaves it.
    let mut vim = HeadlessVim::new("abc\nxyz", (0, 1));
    vim.feed("iQ").unwrap();
    let ai_edit = vim.model_mut().buffer_mut().edit([(6..6, "!")], ai());
    assert!(ai_edit.is_some());
    vim.feed("R<Esc>").unwrap();
    assert_eq!(vim.text(), "aQRbc\nx!yz");
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "abc\nx!yz");
    // Nothing of the user's is left to undo.
    assert_eq!(undo(&mut vim), 0);
    assert_eq!(vim.text(), "abc\nx!yz");
}

#[test]
fn insert_mode_routing_and_context() {
    let mut vim = HeadlessVim::new("abc", (0, 1));
    vim.feed("i").unwrap();
    assert_eq!(vim.state().mode(), Mode::Insert);
    assert_eq!(vim.mode(), "insert");
    let stamped = context(vim.state());
    let editor = stamped
        .frames()
        .iter()
        .find(|frame| frame.name == "editor")
        .unwrap();
    assert_eq!(editor.get("vim.mode"), Some("insert"));
    // Keys are text now: `3d` would be a count and an operator to `step`.
    vim.feed("3d").unwrap();
    assert_eq!(vim.text(), "a3dbc");
    assert_eq!(vim.state().pending().kind(), None);
    assert!(matches!(
        vim.feed("<Up>"),
        Err(crate::FeedError::Forwarded(crate::VimKey::Up))
    ));
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.mode(), "normal");
    // One grapheme back, onto the last typed.
    assert_eq!(vim.cursor(), (0, 2));

    // `vim.normal_mode` applies only in insert mode.
    let registry = registry();
    let meta = registry.get(&"vim.normal_mode".parse().unwrap()).unwrap();
    assert_eq!(
        meta.context,
        Some(ContextPredicate::parse("editor && vim.mode == insert").unwrap())
    );
    let normal_mode = Invocation::of("vim.normal_mode")
        .unwrap()
        .context(context(&VimState::new()));
    assert!(matches!(
        vim.dispatch(&normal_mode),
        Err(DispatchError::NotInContext { .. })
    ));

    // `<Esc>` steps back a whole grapheme, but not off a row's start.
    let mut vim = HeadlessVim::new("ae\u{301}x", (0, 4));
    vim.feed("i<Esc>").unwrap();
    assert_eq!(vim.cursor(), (0, 1));
    let mut vim = HeadlessVim::new("ab\ncd", (1, 1));
    vim.feed("0i<Esc>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    vim.feed("ix<BS><Esc>").unwrap();
    assert_eq!(vim.cursor(), (1, 0));

    // What `.` types again: the text from the session's start to the caret.
    let mut vim = HeadlessVim::new("one two", (0, 4));
    vim.feed("cwab<BS>c<Esc>").unwrap();
    assert_eq!(vim.text(), "one ac");
    let last = vim.state().last_change().unwrap();
    assert_eq!(last.inserted(), Some("ac"));
    assert_eq!(last.change().invocation().command.as_str(), "vim.operate");
    // Nothing if the caret ended before the start.
    let mut vim = HeadlessVim::new("one two", (0, 4));
    vim.feed("ixy").unwrap();
    vim.model_mut().set_cursor(1);
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "one xytwo");
    assert_eq!(vim.state().last_change().unwrap().inserted(), Some(""));
    // After it, the text up to it; the count copies only what was typed.
    let mut vim = HeadlessVim::new("one two\nthree", (0, 4));
    vim.feed("3ix").unwrap();
    vim.model_mut().set_cursor(12);
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "one xtwo\nthree");
    let last = vim.state().last_change().unwrap();
    assert_eq!(last.inserted(), Some("xtwo\nthr"));
    // Taken once indentation nothing was typed after is gone.
    let mut vim = HeadlessVim::new("  ab", (0, 2));
    vim.feed("A<CR><Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n");
    assert_eq!(vim.state().last_change().unwrap().inserted(), Some("\n"));

    // `cw` on a word keeps the space after it.
    let mut vim = HeadlessVim::new("one two three", (0, 4));
    vim.feed("cwX<Esc>").unwrap();
    assert_eq!(vim.text(), "one X three");
    assert_eq!(vim.cursor(), (0, 4));

    // Leaving insert mode with nothing open still lands in normal mode.
    let mut vim = HeadlessVim::new("abc", (0, 0));
    vim.feed("a").unwrap();
    let dispatched = vim
        .dispatch(&normal_mode.clone().context(context(vim.state())))
        .unwrap();
    assert_eq!(dispatched.effect, Effect::Changed);
    assert_eq!(vim.mode(), "normal");
}

/// Carets at these byte offsets.
fn carets(vim: &mut HeadlessVim, offsets: &[usize]) {
    vim.model_mut()
        .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
}

#[test]
fn indentation_dropped_is_the_sessions_own() {
    // Blanks already there stay, wherever the caret went.
    let mut vim = HeadlessVim::new("ab\n  \ncd", (0, 0));
    vim.feed("iX").unwrap();
    vim.model_mut().set_cursor(8);
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "Xab\n  \ncd");

    // Indentation `o` made goes with nothing typed, whatever another origin
    // edits meanwhile.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut().buffer_mut().edit([(10..10, "!")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n\ncd!");
}

#[test]
fn each_caret_finishes_its_own_session() {
    // A count copies each caret's own text, indentation included.
    let mut vim = HeadlessVim::new("  ab\n    cd", (0, 0));
    carets(&mut vim, &[3, 10]);
    vim.feed("2ix<CR>y<Esc>").unwrap();
    assert_eq!(vim.text(), "  ax\n  yx\n  yb\n    cx\n    yx\n    yd");

    // Every caret's new row loses the indentation nothing was typed after.
    let mut vim = HeadlessVim::new("  ab\n    cd", (0, 0));
    carets(&mut vim, &[3, 10]);
    vim.feed("A<CR><Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n\n    cd\n");
    let mut vim = HeadlessVim::new("  ab\n    cd", (0, 0));
    carets(&mut vim, &[3, 10]);
    vim.feed("o<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n\n    cd\n");
}

#[test]
fn another_origins_text_is_never_dropped() {
    // It replaced the indentation `o` made.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut().buffer_mut().edit([(5..7, "\t")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n\t\ncd");

    // It put a row of blanks among what was typed.
    let mut vim = HeadlessVim::new("ab", (0, 0));
    vim.feed("Ax").unwrap();
    vim.model_mut().buffer_mut().edit([(3..3, "\n  ")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "abx\n  ");

    // A line break was typed, then the caret went elsewhere: the row `o`
    // opened still loses its unused indentation.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o<CR>x").unwrap();
    assert_eq!(vim.text(), "  ab\n  \n  x\ncd");
    vim.model_mut().set_cursor(0);
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n\n  x\ncd");
}

#[test]
fn indentation_is_followed_through_every_edit() {
    // Another origin joins the row `o` opened to a row of blanks that was
    // already there: only the session's own blanks go.
    let mut vim = HeadlessVim::new("  ab\n  \ncd", (0, 0));
    vim.feed("o").unwrap();
    assert_eq!(vim.text(), "  ab\n  \n  \ncd");
    vim.model_mut().buffer_mut().edit([(7..8, "")], ai());
    assert_eq!(vim.text(), "  ab\n    \ncd");
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n  \ncd");
    assert_eq!(vim.cursor(), (1, 0));
    // Joined to the end of a row with text, it isn't alone on its row.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut().buffer_mut().edit([(4..5, "")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab  \ncd");

    // Indentation another origin changed stays as it is: it deleted some,
    let mut vim = HeadlessVim::new("    ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut().buffer_mut().edit([(7..8, "")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "    ab\n   \ncd");
    // or wrote inside it.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut().buffer_mut().edit([(6..6, " ")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n   \ncd");
    // Blanks it put next to it stay; the session's go.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut()
        .buffer_mut()
        .edit([(5..5, "\t"), (7..7, " ")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n\t \ncd");

    // A typed line break's indentation, after another origin split the row
    // before it.
    let mut vim = HeadlessVim::new("  ab", (0, 0));
    vim.feed("A<CR>").unwrap();
    vim.model_mut().buffer_mut().edit([(2..2, "\n")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  \nab\n");
}

#[test]
fn typing_counts_on_its_own_row() {
    // Typing at one caret leaves the other caret's unused indentation to go.
    let mut vim = HeadlessVim::new("  ab\n    cd", (0, 0));
    carets(&mut vim, &[3, 10]);
    vim.feed("o").unwrap();
    assert_eq!(vim.text(), "  ab\n  \n    cd\n    ");
    vim.model_mut().set_cursor(7);
    vim.feed("x<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n  x\n    cd\n");

    // So does typing outside what any caret typed, on another row.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut().set_cursor(8);
    vim.feed("x<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n\nxcd");
    // On the same row, it's typing there.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut().set_cursor(5);
    vim.feed(" <Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n   \ncd");

    // Another origin's typing isn't the session's.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    vim.model_mut().buffer_mut().edit([(7..7, "  ")], ai());
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  ab\n  \ncd");
}

#[test]
fn a_session_follows_edits_as_the_context_is_stamped() {
    // The host stamps the context for every key, so the edits made since
    // never pile up for `Esc`.
    let mut vim = HeadlessVim::new("  ab\ncd", (0, 0));
    vim.feed("o").unwrap();
    for _ in 0..3 {
        vim.model_mut().buffer_mut().edit([(0..0, "x")], ai());
    }
    let backlog = |vim: &HeadlessVim| vim.state().session.as_ref().unwrap().backlog();
    context(vim.state());
    assert_eq!(backlog(&vim), 0);
    vim.feed("y<BS>").unwrap();
    assert_eq!(backlog(&vim), 1);
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "xxx  ab\n  \ncd");
}

#[test]
fn clones_of_the_state_share_its_session() {
    let (mut host, mut registers, registry) = (HeadlessHost::new(), Registers::new(), registry());
    let mut dispatch = |model: &mut EditorModel, state: &mut VimState, command: Invocation| {
        let command = command.context(context(state));
        let mut target = VimTarget {
            model,
            host: &mut host,
            state,
            registers: &mut registers,
        };
        registry.dispatch(&mut target, &command).unwrap()
    };
    let text = |model: &EditorModel| model.text().slice_to_string(0..model.text().len());
    let normal_mode = Invocation::of("vim.normal_mode").unwrap();

    // The clone knows what ending the session through the first removed.
    let mut model = EditorModel::new(Buffer::new("  ab\ncd"), None);
    let mut state = VimState::new();
    let open = InsertArgs {
        at: InsertAt::LineBelow,
        count: None,
    };
    dispatch(&mut model, &mut state, invocation("vim.insert", open));
    let mut clone = state.clone();
    model.buffer_mut().edit([(0..2, "")], ai());
    dispatch(&mut model, &mut state, normal_mode.clone());
    assert_eq!(text(&model), "ab\n\ncd");
    dispatch(&mut model, &mut clone, normal_mode.clone());
    assert_eq!(text(&model), "ab\n\ncd");

    // A session ends once, whichever state ends it: the count's copies go
    // in once, in its one transaction, and the caret steps back once.
    let mut model = EditorModel::new(Buffer::new("ab"), None);
    let mut state = VimState::new();
    let open = InsertArgs {
        at: InsertAt::Before,
        count: Some(2),
    };
    dispatch(&mut model, &mut state, invocation("vim.insert", open));
    model.insert("x");
    let mut clone = state.clone();
    let ended = dispatch(&mut model, &mut state, normal_mode.clone());
    assert_eq!(ended.transactions.len(), 1);
    assert_eq!(text(&model), "xxab");
    assert_eq!(model.primary_selection().head(), 1);
    let again = dispatch(&mut model, &mut clone, normal_mode);
    assert!(again.transactions.is_empty());
    assert_eq!(text(&model), "xxab");
    assert_eq!(model.primary_selection().head(), 1);
    assert_eq!(clone.mode(), Mode::Normal);
    // Both states have the session as their last change and its carets.
    assert_eq!(clone.last_change(), state.last_change());
    assert_eq!(clone.last_change().unwrap().inserted(), Some("x"));
    let repeat = || invocation("vim.repeat", CountArgs { count: None });
    let undo = || invocation("vim.undo", CountArgs { count: None });
    dispatch(&mut model, &mut clone, repeat());
    assert_eq!(text(&model), "xxxxab");
    dispatch(&mut model, &mut clone, undo());
    assert_eq!(text(&model), "xxab");
    assert_eq!(model.primary_selection().head(), 1);
    // One undo step takes the session back, its caret where it began.
    model.set_cursor(3);
    dispatch(&mut model, &mut clone, undo());
    assert_eq!(text(&model), "ab");
    assert_eq!(model.primary_selection().head(), 0);
}

#[test]
fn only_a_typed_line_break_brings_indentation() {
    // Input holding a line break and white space is typed text, whatever
    // it holds: it stays, and `.` types it again.
    for blanks in [" ", "  ", "\t", "\u{3000}"] {
        let mut vim = HeadlessVim::new("abc\ndef", (0, 0));
        vim.feed("A").unwrap();
        vim.model_mut().insert(&format!("\n{blanks}"));
        vim.feed("<Esc>").unwrap();
        assert_eq!(vim.text(), format!("abc\n{blanks}\ndef"), "{blanks:?}");
        let inserted = vim.state().last_change().unwrap().inserted();
        assert_eq!(inserted, Some(format!("\n{blanks}").as_str()));
        vim.feed("j.").unwrap();
        assert_eq!(vim.text(), format!("abc\n{blanks}\ndef\n{blanks}"));
    }
    // Even where it's what a line break would have copied.
    let mut vim = HeadlessVim::new("  abc", (0, 0));
    vim.feed("A").unwrap();
    vim.model_mut().insert("\n  ");
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  abc\n  ");
    // A line break typed there is 'autoindent'.
    let mut vim = HeadlessVim::new("  abc", (0, 0));
    vim.feed("A<CR><Esc>").unwrap();
    assert_eq!(vim.text(), "  abc\n");
    // With no caret to type at, `Enter` types nothing, so input after it
    // from the same version is typed text still.
    let mut vim = HeadlessVim::new("  abc", (0, 0));
    vim.feed("A").unwrap();
    vim.model_mut().set_selections(Vec::new());
    vim.feed("<CR>").unwrap();
    assert_eq!(vim.text(), "  abc");
    vim.model_mut().set_cursor(5);
    vim.model_mut().insert("\n  ");
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  abc\n  ");
    let inserted = vim.state().last_change().unwrap().inserted();
    assert_eq!(inserted, Some("\n  "));
}

#[test]
fn counted_insert_bookkeeping_is_per_caret() {
    // A count's copies are noted as one entry per caret, however many there
    // are, and go in with the typed text as one undo step.
    let noted = |count: usize| {
        let mut vim = HeadlessVim::new("ab", (0, 1));
        vim.feed(&format!("{count}ix<Esc>")).unwrap();
        assert_eq!(vim.text(), format!("a{}b", "x".repeat(count)));
        let noted = COPIES_NOTED.get();
        assert_eq!(undo(&mut vim), 1);
        assert_eq!(vim.text(), "ab");
        noted
    };
    assert_eq!(noted(5), 1);
    assert_eq!(noted(1_000_000), 1);

    // The indentation in each caret's typed text is noted once, not once a
    // copy, and still goes from every copy.
    let noted = |count: usize| {
        let mut vim = HeadlessVim::new("  ab\n  cd", (0, 0));
        carets(&mut vim, &[2, 7]);
        vim.feed(&format!("{count}A<CR><Esc>")).unwrap();
        let rows = "\n".repeat(count);
        assert_eq!(vim.text(), format!("  ab{rows}\n  cd{rows}"));
        let noted = COPIES_NOTED.get();
        assert_eq!(undo(&mut vim), 1);
        assert_eq!(vim.text(), "  ab\n  cd");
        noted
    };
    assert_eq!(noted(5), 4);
    assert_eq!(noted(1_000), 4);
    // So with `o`, each copy in a row of its own.
    let noted = |count: usize| {
        let mut vim = HeadlessVim::new("  ab", (0, 0));
        vim.feed(&format!("{count}o<CR><Esc>")).unwrap();
        assert_eq!(vim.text(), format!("  ab{}", "\n".repeat(2 * count)));
        let noted = COPIES_NOTED.get();
        assert_eq!(undo(&mut vim), 1);
        assert_eq!(vim.text(), "  ab");
        noted
    };
    assert_eq!(noted(5), 1);
    assert_eq!(noted(1_000), 1);
}

#[test]
fn newline_provenance_before_sync() {
    // `A`, then `Enter` on a row indented with spaces and U+3000.
    let mut vim = HeadlessVim::new("  \u{3000}ab\ncd", (0, 0));
    vim.feed("A").unwrap();
    let (state, clone) = (vim.state().clone(), vim.state().clone());
    let session = clone.session.as_ref().unwrap();
    // While `newline()` catches the model up, measuring the new row may
    // stamp the context through a clone of the state, which follows the
    // line break's batch then. What measures (`fe_display::CharWidth`)
    // isn't a dependency of fe-vim, so the stamp comes right after the
    // edit, still inside the call `line_break` makes: to the session it's
    // the same.
    let mut followed = false;
    state.line_break_with(vim.model_mut(), |model| {
        // The line break is noted by the version its batch starts from
        // before it's typed, and nothing is left locked.
        assert_eq!(session.line_breaks(), Some(vec![model.buffer().version()]));
        model.newline();
        context(&clone);
        followed = session.backlog() == 0;
    });
    assert!(followed);
    assert_eq!(vim.text(), "  \u{3000}ab\n  \u{3000}\ncd");
    // The indentation it copied is the line break's, which `Esc` drops and
    // `.` doesn't type again.
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "  \u{3000}ab\n\ncd");
    assert_eq!(vim.state().last_change().unwrap().inserted(), Some("\n"));
    vim.feed(".").unwrap();
    assert_eq!(vim.text(), "  \u{3000}ab\n\n\ncd");
}

#[test]
fn closing_a_session_locks_nothing_while_measuring() {
    // Measuring text as the model catches up may stamp the context through
    // a clone of the state, which follows the session's edits: `Esc` never
    // has the session's record locked then, and comes out the same, the
    // count's copies (whose batch the stamp follows first) included.
    let esc = |stamping: bool| {
        let mut vim = HeadlessVim::new("  ab", (0, 0));
        vim.feed("3A<CR>").unwrap();
        let clone = vim.state().clone();
        let stamps = Rc::new(Cell::new(0));
        let measuring = stamping.then(|| {
            let stamps = Rc::clone(&stamps);
            Measuring::new(move |_| {
                let session = clone.session.as_ref().unwrap();
                assert!(!session.is_locked(), "measuring with the record locked");
                context(&clone);
                stamps.set(stamps.get() + 1);
            })
        });
        vim.feed("<Esc>").unwrap();
        drop(measuring);
        let inserted = vim
            .state()
            .last_change()
            .unwrap()
            .inserted()
            .map(str::to_string);
        let ended = (vim.text(), vim.cursor(), inserted);
        assert_eq!(undo(&mut vim), 1);
        assert_eq!(vim.text(), "  ab");
        (ended, stamps.get())
    };
    let (stamped, stamps) = esc(true);
    assert!(stamps >= 4, "{stamps} stamps");
    assert_eq!(stamped, esc(false).0);
    assert_eq!(stamped.0, "  ab\n\n\n");

    // A clone ending the session meanwhile (here once the count's copies
    // went in) finds it ending: it returns to normal mode and leaves the
    // rest to the first, so the copies go in once.
    let mut vim = HeadlessVim::new("ab", (0, 0));
    vim.feed("3ix").unwrap();
    let clone = Rc::new(RefCell::new(vim.state().clone()));
    let mut split = vim.model().split();
    split.set_cursor(vim.model().primary_selection().head());
    let ended = Rc::new(Cell::new(false));
    let measuring = {
        let ended = Rc::clone(&ended);
        let clone = Rc::clone(&clone);
        Measuring::new(move |edited| {
            let mut clone = clone.borrow_mut();
            let clone = &mut *clone;
            if !edited || ended.replace(true) {
                return;
            }
            assert!(!clone.session.as_ref().unwrap().is_locked());
            let normal_mode = Invocation::of("vim.normal_mode")
                .unwrap()
                .context(context(clone));
            let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
            let mut target = VimTarget {
                model: &mut split,
                host: &mut host,
                state: clone,
                registers: &mut registers,
            };
            registry().dispatch(&mut target, &normal_mode).unwrap();
            assert_eq!(clone.mode(), Mode::Normal);
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    assert!(ended.get());
    assert_eq!(vim.text(), "xxxab");
    assert_eq!(vim.state().last_change().unwrap().inserted(), Some("x"));
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "ab");

    // The clone, kept past the outer ending, adopts its outcome: `.` repeats
    // the insert, and undo and redo restore the carets.
    let mut vim = HeadlessVim::new("ab", (0, 1));
    vim.feed("3ix").unwrap();
    let state = vim.state().clone();
    let clone = Rc::new(RefCell::new(state));
    let mut split = vim.model().split();
    split.set_cursor(vim.model().primary_selection().head());
    let ended = Rc::new(Cell::new(false));
    let measuring = {
        let (ended, clone) = (Rc::clone(&ended), Rc::clone(&clone));
        Measuring::new(move |edited| {
            if !edited || ended.replace(true) {
                return;
            }
            let mut clone = clone.borrow_mut();
            let normal_mode = Invocation::of("vim.normal_mode")
                .unwrap()
                .context(context(&clone));
            let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
            let mut target = VimTarget {
                model: &mut split,
                host: &mut host,
                state: &mut clone,
                registers: &mut registers,
            };
            registry().dispatch(&mut target, &normal_mode).unwrap();
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    assert_eq!(vim.text(), "axxxb");
    // The clone drives the same model through the host's registry.
    let mut clone = Rc::try_unwrap(clone).ok().unwrap().into_inner();
    let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
    let mut run = |model: &mut EditorModel, state: &mut VimState, keys: &str| {
        let invocation = commit(keys).context(context(state));
        let mut target = VimTarget {
            model,
            host: &mut host,
            state,
            registers: &mut registers,
        };
        registry().dispatch(&mut target, &invocation).unwrap();
    };
    let model = vim.model_mut();
    run(model, &mut clone, ".");
    model.sync_display();
    assert_eq!(
        model.text().slice_to_string(0..model.text().len()),
        "axxxxxxb"
    );
    run(model, &mut clone, "u");
    run(model, &mut clone, "u");
    model.sync_display();
    assert_eq!(model.text().slice_to_string(0..model.text().len()), "ab");
    let head = model.primary_selection().head();
    assert_eq!(head, 1);
    run(model, &mut clone, "<C-r>");
    model.sync_display();
    assert_eq!(model.text().slice_to_string(0..model.text().len()), "axxxb");
}

#[test]
fn a_retained_clone_adopts_its_session_before_any_newer_change() {
    // The outer state ends the session while a clone is ending it, and the
    // clone is kept. Its next commands are a normal edit, then a second
    // insert session: each finds the first session's outcome adopted first,
    // so `.` repeats the newer change and undo and redo restore both
    // transactions' carets.
    let mut vim = HeadlessVim::new("ab\ncd", (0, 1));
    vim.feed("3ix").unwrap();
    let clone = Rc::new(RefCell::new(vim.state().clone()));
    let mut split = vim.model().split();
    split.set_cursor(vim.model().primary_selection().head());
    let ended = Rc::new(Cell::new(false));
    let measuring = {
        let (ended, clone) = (Rc::clone(&ended), Rc::clone(&clone));
        Measuring::new(move |edited| {
            if !edited || ended.replace(true) {
                return;
            }
            let mut clone = clone.borrow_mut();
            let normal_mode = Invocation::of("vim.normal_mode")
                .unwrap()
                .context(context(&clone));
            let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
            let mut target = VimTarget {
                model: &mut split,
                host: &mut host,
                state: &mut clone,
                registers: &mut registers,
            };
            registry().dispatch(&mut target, &normal_mode).unwrap();
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    assert_eq!(vim.text(), "axxxb\ncd");
    let mut clone = Rc::try_unwrap(clone).ok().unwrap().into_inner();
    let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
    let mut run = |model: &mut EditorModel, state: &mut VimState, invocation: Invocation| {
        let invocation = invocation.context(context(state));
        let mut target = VimTarget {
            model,
            host: &mut host,
            state,
            registers: &mut registers,
        };
        registry().dispatch(&mut target, &invocation).unwrap();
        target.model.sync_display();
    };
    let text = |model: &EditorModel| model.text().slice_to_string(0..model.text().len());
    let model = vim.model_mut();

    // A newer normal edit comes before the first session is adopted: `.`
    // repeats it, not the insert.
    model.set_cursor(4);
    run(model, &mut clone, commit("rZ"));
    assert_eq!(text(model), "axxxZ\ncd");
    model.set_cursor(0);
    run(model, &mut clone, commit("."));
    assert_eq!(text(model), "ZxxxZ\ncd");

    // A second insert session, opened before the first is adopted, keeps
    // both transactions' carets.
    let mut vim = HeadlessVim::new("ab\ncd", (0, 1));
    vim.feed("3ix").unwrap();
    let clone = Rc::new(RefCell::new(vim.state().clone()));
    let mut split = vim.model().split();
    split.set_cursor(vim.model().primary_selection().head());
    let ended = Rc::new(Cell::new(false));
    let measuring = {
        let (ended, clone) = (Rc::clone(&ended), Rc::clone(&clone));
        Measuring::new(move |edited| {
            if !edited || ended.replace(true) {
                return;
            }
            let mut clone = clone.borrow_mut();
            let normal_mode = Invocation::of("vim.normal_mode")
                .unwrap()
                .context(context(&clone));
            let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
            let mut target = VimTarget {
                model: &mut split,
                host: &mut host,
                state: &mut clone,
                registers: &mut registers,
            };
            registry().dispatch(&mut target, &normal_mode).unwrap();
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    let mut clone = Rc::try_unwrap(clone).ok().unwrap().into_inner();
    let model = vim.model_mut();
    model.set_cursor(4);
    run(model, &mut clone, commit("i"));
    model.insert("y");
    run(
        model,
        &mut clone,
        Invocation::of("vim.normal_mode").unwrap(),
    );
    assert_eq!(text(model), "axxxyb\ncd");
    for (keys, expected, head) in [
        ("u", "axxxb\ncd", 4),
        ("u", "ab\ncd", 1),
        ("<C-r>", "axxxb\ncd", 1),
        ("<C-r>", "axxxyb\ncd", 4),
    ] {
        model.set_cursor(0);
        run(model, &mut clone, commit(keys));
        assert_eq!(text(model), expected, "{keys}");
        assert_eq!(model.primary_selection().head(), head, "{keys}");
    }
}

/// Types `3ix` at (0, 1) of "ab\ncd" and ends it with `Esc`, while a clone of
/// the state ends the session first inside the measuring callback and then
/// runs `meanwhile` on the same model. Returns the model's editor and the
/// clone, to go on with.
fn closed_by_a_clone(meanwhile: impl Fn(&mut VimTarget<'_>) + 'static) -> (HeadlessVim, VimState) {
    let mut vim = HeadlessVim::new("ab\ncd", (0, 1));
    vim.feed("3ix").unwrap();
    let clone = Rc::new(RefCell::new(vim.state().clone()));
    let mut split = vim.model().split();
    split.set_cursor(vim.model().primary_selection().head());
    let ended = Rc::new(Cell::new(false));
    let measuring = {
        let (ended, clone) = (Rc::clone(&ended), Rc::clone(&clone));
        Measuring::new(move |edited| {
            if !edited || ended.replace(true) {
                return;
            }
            let mut clone = clone.borrow_mut();
            let normal_mode = Invocation::of("vim.normal_mode")
                .unwrap()
                .context(context(&clone));
            let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
            let mut target = VimTarget {
                model: &mut split,
                host: &mut host,
                state: &mut clone,
                registers: &mut registers,
            };
            registry().dispatch(&mut target, &normal_mode).unwrap();
            meanwhile(&mut target);
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    (vim, Rc::try_unwrap(clone).ok().unwrap().into_inner())
}

fn run_on(model: &mut EditorModel, state: &mut VimState, invocation: Invocation) {
    let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
    let invocation = invocation.context(context(state));
    let mut target = VimTarget {
        model,
        host: &mut host,
        state,
        registers: &mut registers,
    };
    registry().dispatch(&mut target, &invocation).unwrap();
    target.model.sync_display();
}

fn text_of(model: &EditorModel) -> String {
    model.text().slice_to_string(0..model.text().len())
}

#[test]
fn a_change_made_while_a_clone_closes_its_session_keeps_dot() {
    // An edit the clone makes inside the measuring callback, after it ended
    // the session, is newer than the insert the outer ending publishes later:
    // `.` repeats the edit.
    let (mut vim, mut clone) = closed_by_a_clone(|target| {
        run_on_target(target, commit("rZ"));
    });
    let model = vim.model_mut();
    let after = text_of(model);
    assert!(after.contains('Z'), "{after}");
    model.set_cursor(0);
    run_on(model, &mut clone, commit("."));
    assert_eq!(text_of(model).matches('Z').count(), 2, "{}", text_of(model));
    assert_eq!(text_of(model).matches('x').count(), 2, "{}", text_of(model));
}

fn run_on_target(target: &mut VimTarget<'_>, invocation: Invocation) {
    let invocation = invocation.context(context(target.state));
    registry().dispatch(target, &invocation).unwrap();
}

#[test]
fn undo_restores_the_first_commands_carets_through_a_closing_clone() {
    let history = |vim: &mut HeadlessVim, clone: &mut VimState| {
        let model = vim.model_mut();
        let mut seen = Vec::new();
        for keys in ["u", "<C-r>"] {
            model.set_cursor(0);
            run_on(model, clone, commit(keys));
            seen.push((text_of(model), model.primary_selection().head()));
        }
        seen
    };

    // A normal edit joining the still-open transaction.
    let (mut vim, mut clone) = closed_by_a_clone(|target| {
        run_on_target(target, commit("rZ"));
    });
    let seen = history(&mut vim, &mut clone);
    assert_eq!(seen[0].1, 1, "{seen:?}");
    assert_eq!(seen[1].1, 1, "{seen:?}");

    // A second insert started while the first is ending.
    let (mut vim, mut clone) = closed_by_a_clone(|target| {
        run_on_target(target, commit("A"));
        target.model.insert("y");
        run_on_target(target, Invocation::of("vim.normal_mode").unwrap());
    });
    let seen = history(&mut vim, &mut clone);
    assert_eq!(seen[0].1, 1, "{seen:?}");
    assert_eq!(seen[1].1, 1, "{seen:?}");
}

#[test]
fn undo_restores_the_first_carets_when_a_second_insert_outlives_publication() {
    // The second insert is still open when the first's outcome is published
    // (the transaction stays open, so it publishes none), and closes before
    // it's adopted; with or without a normal edit joining in between.
    for edit in [false, true] {
        let (mut vim, mut clone) = closed_by_a_clone(move |target| {
            if edit {
                run_on_target(target, commit("rZ"));
            }
            run_on_target(target, commit("A"));
            target.model.insert("y");
        });
        let model = vim.model_mut();
        run_on(
            model,
            &mut clone,
            Invocation::of("vim.normal_mode").unwrap(),
        );
        let after = text_of(model);
        assert!(after.contains('y'), "{after}");
        for (keys, has_y) in [("u", false), ("<C-r>", true)] {
            model.set_cursor(0);
            run_on(model, &mut clone, commit(keys));
            assert_eq!(text_of(model).contains('y'), has_y, "{keys} {edit}");
            assert_eq!(model.primary_selection().head(), 1, "{keys} {edit}");
        }
    }
}

#[test]
fn a_caret_inside_a_grapheme_is_on_it() {
    // Another origin puts a combining mark at the caret, leaving it inside
    // `é`: a command takes the caret to be on that grapheme, as nvim does
    // for `a`, `x`, `cw` and `p` from such a cursor (which only its API can
    // place; its `i` and `P` insert at the byte there).
    let inside = |keys: &str| {
        let mut vim = HeadlessVim::new("ae x", (0, 2));
        vim.model_mut().buffer_mut().edit([(2..2, "\u{301}")], ai());
        assert_eq!(vim.model().primary_selection().head(), 2);
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    assert_eq!(inside("aZ<Esc>"), ("ae\u{301}Z x".to_string(), (0, 4)));
    assert_eq!(inside("iZ<Esc>"), ("aZe\u{301} x".to_string(), (0, 1)));
    assert_eq!(inside("cwX<Esc>"), ("aX x".to_string(), (0, 1)));
    assert_eq!(inside("x"), ("a x".to_string(), (0, 1)));
    assert_eq!(inside("ylp"), ("ae\u{301}e\u{301} x".to_string(), (0, 4)));
    // So do `p` and `P` from there.
    let put = |keys: &str| {
        let mut vim = HeadlessVim::new("ae x", (0, 2));
        vim.model_mut().buffer_mut().edit([(2..2, "\u{301}")], ai());
        let host = &mut HeadlessHost::new();
        let value = Register::new("Z", false);
        vim.registers_mut()
            .write(None, Write::Yank, value, host)
            .unwrap();
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    assert_eq!(put("p"), ("ae\u{301}Z x".to_string(), (0, 4)));
    assert_eq!(put("P"), ("aZe\u{301} x".to_string(), (0, 1)));
}
