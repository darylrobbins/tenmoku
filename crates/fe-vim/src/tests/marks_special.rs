//! Tests of the special marks (`src/marks.rs`'s `settable` and `jumpable`
//! name which a key reaches): `'[`, `']`, `'.` and `'^`, and the uppercase
//! marks. The positions are nvim 0.12.4's (`getpos()`, a linewise `']` on
//! its row's last byte).

use crate::HeadlessVim;

const TEXT: &str = "one two three\nfour five\nsix\nseven\neight";

fn ai() -> fe_buffer::Origin {
    fe_buffer::Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

/// `'[`, `']` and `'.` after `keys` from `cursor` in [`TEXT`].
fn changed(cursor: (u32, usize), keys: &str) -> [Option<(u32, usize)>; 3] {
    let mut vim = HeadlessVim::new(TEXT, cursor);
    vim.feed(keys).unwrap();
    ['[', ']', '.'].map(|name| vim.mark(name))
}

#[test]
fn a_yank_sets_start_and_end() {
    // The first and last char yanked; a linewise yank's first row's column
    // 0 and last row's last byte. Nothing changed, so no `'.`.
    assert_eq!(changed((1, 5), "yiw"), [Some((1, 5)), Some((1, 8)), None]);
    assert_eq!(changed((1, 5), "yy"), [Some((1, 0)), Some((1, 8)), None]);
    assert_eq!(changed((1, 5), "yj"), [Some((1, 0)), Some((2, 2)), None]);
    assert_eq!(changed((1, 5), "vey"), [Some((1, 5)), Some((1, 8)), None]);
}

#[test]
fn a_put_sets_them_around_what_it_put() {
    assert_eq!(
        changed((1, 0), "yiwp"),
        [Some((1, 1)), Some((1, 4)), Some((1, 1))]
    );
    assert_eq!(
        changed((1, 5), "yiwP"),
        [Some((1, 5)), Some((1, 8)), Some((1, 5))]
    );
    // Whole rows, below, above and after the last.
    assert_eq!(
        changed((1, 5), "yjp"),
        [Some((2, 0)), Some((3, 2)), Some((2, 0))]
    );
    assert_eq!(
        changed((1, 5), "yyP"),
        [Some((1, 0)), Some((1, 8)), Some((1, 0))]
    );
    assert_eq!(
        changed((1, 5), "yyGp"),
        [Some((5, 0)), Some((5, 8)), Some((5, 0))]
    );
}

#[test]
fn a_delete_sets_them_where_it_took_text() {
    assert_eq!(
        changed((1, 0), "dw"),
        [Some((1, 0)), Some((1, 0)), Some((1, 0))]
    );
    assert_eq!(
        changed((1, 2), "3x"),
        [Some((1, 2)), Some((1, 2)), Some((1, 2))]
    );
    // Whole rows: on the row after them at the caret's column, `'.` at its
    // start.
    assert_eq!(
        changed((1, 2), "dd"),
        [Some((1, 2)), Some((1, 2)), Some((1, 0))]
    );
    assert_eq!(
        changed((1, 2), "dk"),
        [Some((0, 2)), Some((0, 2)), Some((0, 0))]
    );
    // Over a line break, `'.` is on the row after.
    assert_eq!(
        changed((0, 4), "vjd"),
        [Some((0, 4)), Some((0, 4)), Some((1, 0))]
    );
}

#[test]
fn a_shift_spans_the_caret_to_the_last_rows_end() {
    assert_eq!(
        changed((1, 2), ">>"),
        [Some((1, 2)), Some((1, 12)), Some((1, 0))]
    );
    assert_eq!(
        changed((1, 2), ">j"),
        [Some((1, 2)), Some((2, 6)), Some((1, 0))]
    );
}

#[test]
fn leaving_insert_mode_sets_start_end_and_insert() {
    for (keys, cursor, start, end, change) in [
        ("cwxy<Esc>", (1, 0), (1, 0), (1, 2), (1, 0)),
        ("ixy<Esc>", (1, 2), (1, 2), (1, 4), (1, 2)),
        ("Axy<Esc>", (1, 0), (1, 9), (1, 11), (1, 9)),
        ("oab<Esc>", (1, 0), (2, 0), (2, 2), (2, 0)),
        ("ia<CR>b<Esc>", (1, 2), (1, 2), (2, 1), (2, 0)),
    ] {
        let mut vim = HeadlessVim::new(TEXT, cursor);
        vim.feed(keys).unwrap();
        let marks = ['[', ']', '.', '^'].map(|name| vim.mark(name));
        let want = [start, end, change, end].map(Some);
        assert_eq!(marks, want, "{keys}");
    }
    // Typing nothing sets `'[`, `']` and `'^`, not `'.`.
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    vim.feed("a<Esc>").unwrap();
    let marks = ['[', ']', '.', '^'].map(|name| vim.mark(name));
    assert_eq!(marks, [Some((1, 3)), Some((1, 3)), None, Some((1, 3))]);
    // `'^` stays where insert mode was left; it goes with its row.
    vim.feed("ixy<Esc>ggdd").unwrap();
    assert_eq!(vim.mark('^'), Some((0, 4)));
    vim.feed("`^").unwrap();
    assert_eq!(vim.cursor(), (0, 4));
}

#[test]
fn undo_sets_them_on_the_rows_it_changed() {
    assert_eq!(
        changed((1, 2), "jjxu"),
        [Some((3, 0)), Some((3, 0)), Some((3, 0))]
    );
    assert_eq!(
        changed((1, 2), "dju"),
        [Some((1, 0)), Some((2, 0)), Some((1, 0))]
    );
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    vim.feed("jjxugg`.").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
}

#[test]
fn set_start_and_end_with_m() {
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    vim.feed("m[jjm]gg").unwrap();
    assert_eq!((vim.mark('['), vim.mark(']')), (Some((1, 2)), Some((3, 2))));
    vim.feed("`]").unwrap();
    assert_eq!(vim.cursor(), (3, 2));
    vim.feed("'[").unwrap();
    assert_eq!(vim.cursor(), (1, 0));
    // The next change sets them again.
    vim.feed("x").unwrap();
    assert_eq!((vim.mark('['), vim.mark(']')), (Some((1, 0)), Some((1, 0))));
}

#[test]
fn jumps_to_them_go_where_they_are() {
    let mut vim = HeadlessVim::new(TEXT, (1, 5));
    vim.feed("yiwgg`]").unwrap();
    assert_eq!(vim.cursor(), (1, 8));
    vim.feed("gg`[").unwrap();
    assert_eq!(vim.cursor(), (1, 5));
    vim.feed("yyG`]").unwrap();
    assert_eq!(vim.cursor(), (1, 8));
    // Unset, a jump fails.
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    for name in ['[', ']', '.', '^', 'A'] {
        vim.feed(&format!("`{name}")).unwrap();
        assert_eq!(vim.cursor(), (1, 2), "`{name}");
        assert!(vim.state().failed, "`{name}");
    }
}

#[test]
fn other_origins_edits_move_them_without_setting_them() {
    let mut vim = HeadlessVim::new(TEXT, (1, 5));
    vim.feed("yiw").unwrap();
    // A row put in above moves them down; an edit in their row leaves their
    // columns.
    vim.model_mut()
        .buffer_mut()
        .edit([(0..0, "new\n"), (4..4, "zz")], ai())
        .unwrap();
    assert_eq!((vim.mark('['), vim.mark(']')), (Some((2, 5)), Some((2, 8))));
    assert_eq!(vim.mark('.'), None);
    vim.feed("gg`]").unwrap();
    assert_eq!(vim.cursor(), (2, 8));
}

#[test]
fn uppercase_marks_are_set_and_jumped_to_as_lowercase_ones() {
    let mut vim = HeadlessVim::new("one\n  two three\nfour\nfive", (1, 6));
    vim.feed("mAjmZgg'A").unwrap();
    assert_eq!(vim.cursor(), (1, 2));
    vim.feed("`A").unwrap();
    assert_eq!(vim.cursor(), (1, 6));
    assert_eq!((vim.mark('A'), vim.mark('Z')), (Some((1, 6)), Some((2, 3))));
    // Apart from `a`.
    assert_eq!(vim.mark('a'), None);
    // Rows put in above move them.
    vim.feed("ggyyP").unwrap();
    assert_eq!(vim.mark('A'), Some((2, 6)));
    vim.feed("gg").unwrap();
    vim.feed("d'A").unwrap();
    assert_eq!(vim.text(), "four\nfive");
}

#[test]
fn an_uppercase_mark_goes_to_the_row_that_takes_its_rows_place() {
    // As nvim's: deleting its row doesn't delete it.
    let mut vim = HeadlessVim::new(TEXT, (2, 1));
    vim.feed("mAdd").unwrap();
    assert_eq!(vim.mark('A'), Some((2, 1)));
    vim.feed("gg`A").unwrap();
    assert_eq!(vim.cursor(), (2, 1));
    // Joined to the row above, it goes with its text.
    let mut vim = HeadlessVim::new(TEXT, (2, 1));
    vim.feed("mAkJ").unwrap();
    assert_eq!(vim.mark('A'), Some((1, 11)));
    // Past the last row, it's nowhere.
    let mut vim = HeadlessVim::new(TEXT, (4, 1));
    vim.feed("mAdd").unwrap();
    assert_eq!(vim.mark('A'), None);
}

#[test]
fn any_actors_vim_commands_set_them_and_no_other_edit_does() {
    use fe_command::Actor;
    // The first command of all, whichever it is.
    for (keys, want) in [("rx", (1, 2)), ("~", (1, 2)), ("J", (1, 9))] {
        let mut vim = HeadlessVim::new(TEXT, (1, 2));
        vim.feed(keys).unwrap();
        assert_eq!(vim.mark('['), Some(want), "{keys}");
    }
    // An agent's vim command, then the user's.
    let mut vim = HeadlessVim::new(TEXT, (1, 0));
    let agent = Actor::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    let dw = super::commit("dw")
        .context(crate::context(vim.state()))
        .actor(agent);
    vim.dispatch(&dw).unwrap();
    assert_eq!(vim.text(), "one two three\nfive\nsix\nseven\neight");
    assert_eq!(vim.mark('.'), Some((1, 0)));
    vim.feed("jx").unwrap();
    assert_eq!(vim.mark('.'), Some((2, 0)));
    // An edit no vim command made sets nothing, whoever made it.
    for origin in [ai(), fe_buffer::Origin::User] {
        let edit = vim.model_mut().buffer_mut().edit([(0..3, "ONE")], origin);
        edit.unwrap();
        assert_eq!(vim.mark('.'), Some((2, 0)));
    }
}

#[test]
fn a_put_after_an_empty_last_row_marks_the_new_row() {
    let mut vim = HeadlessVim::new("one\n", (1, 0));
    vim.feed("yyp").unwrap();
    assert_eq!(vim.text(), "one\n\n");
    let marks = ['[', ']', '.'].map(|name| vim.mark(name));
    assert_eq!(marks, [Some((2, 0)); 3]);
}

#[test]
fn a_puts_or_shifts_end_keeps_its_column() {
    // Another origin's text at the row's end leaves `']` where it was.
    for keys in ["yyp", ">>"] {
        let mut vim = HeadlessVim::new("one\ntwo", (0, 1));
        vim.feed(keys).unwrap();
        let end = vim.mark(']').unwrap();
        let text = vim.model().text();
        let row_end = text.row_start(end.0) + text.row_len(end.0) as usize;
        vim.model_mut()
            .buffer_mut()
            .edit([(row_end..row_end, " more")], ai())
            .unwrap();
        assert_eq!(vim.mark(']'), Some(end), "{keys}");
    }
}

#[test]
fn a_shift_over_unchanged_rows_spans_them_all() {
    // `>j` down to an empty row, and `>>` on one, which changes nothing.
    let mut vim = HeadlessVim::new("one\n\ntwo", (0, 2));
    vim.feed(">j").unwrap();
    assert_eq!((vim.mark('['), vim.mark(']')), (Some((0, 2)), Some((1, 0))));
    vim.feed("j>>").unwrap();
    let marks = ['[', ']', '.'].map(|name| vim.mark(name));
    assert_eq!(marks, [Some((1, 0)); 3]);
    // Indentation of any width.
    let deep = format!("{}x\ny", " ".repeat(2000));
    let mut vim = HeadlessVim::new(&deep, (0, 2000));
    vim.feed("<<").unwrap();
    assert_eq!(
        (vim.mark('['), vim.mark(']')),
        (Some((0, 2000)), Some((0, 1996)))
    );
}

#[test]
fn clones_leaving_insert_mode_share_its_marks() {
    use fe_buffer::Buffer;
    use fe_command::Invocation;
    use fe_editor::{EditorModel, commands::HeadlessHost};

    use crate::{InsertArgs, InsertAt, Registers, VimState, VimTarget, context, registry};

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
    let normal_mode = Invocation::of("vim.normal_mode").unwrap();
    let mut model = EditorModel::new(Buffer::new("ab\ncd"), None);
    model.set_selections([crate::operator::caret(1)]);
    let mut state = VimState::new();
    let open = InsertArgs {
        at: InsertAt::Before,
        count: None,
    };
    dispatch(
        &mut model,
        &mut state,
        super::invocation("vim.insert", open),
    );
    model.insert("xy");
    let mut clone = state.clone();
    dispatch(&mut model, &mut state, normal_mode.clone());
    dispatch(&mut model, &mut clone, normal_mode);
    let snapshot = model.snapshot();
    for name in ['[', ']', '^'] {
        let marks = [&state, &clone].map(|state| state.mark(name).and_then(|m| m.get(snapshot)));
        let want = match name {
            '[' => (0, 1),
            _ => (0, 3),
        };
        assert_eq!(marks, [Some(want); 2], "{name}");
    }
}

#[test]
fn a_model_behind_the_buffer_keeps_them() {
    // Another view edits the shared buffer after a change here; following
    // with this view's older snapshot neither loses nor misplaces them.
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    vim.feed("x").unwrap();
    let mut other = vim.model().split();
    other.buffer_mut().edit([(0..0, "new\n")], ai()).unwrap();
    let snapshot = vim.model().snapshot().clone();
    crate::marks::catch_up(vim.state_mut());
    vim.state_mut().marks.follow(&snapshot);
    vim.model_mut().sync_display();
    let snapshot = vim.model().snapshot();
    let at = |name| vim.state().mark(name).and_then(|mark| mark.get(snapshot));
    assert_eq!((at('['), at('.')), (Some((2, 2)), Some((2, 2))));
}

#[test]
fn a_jump_past_the_last_row_fails_with_e19() {
    for keys in ["Gdd`[", "GmAddgg`A"] {
        let mut vim = HeadlessVim::new(TEXT, (1, 2));
        vim.feed(keys).unwrap();
        assert_eq!(
            vim.state().message(),
            Some("E19: Mark has invalid line number"),
            "{keys}"
        );
    }
    // All the text taken charwise leaves `'.` on the row after it.
    let mut vim = HeadlessVim::new("abc\ndef", (0, 0));
    vim.feed("vG$d`.").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("E19: Mark has invalid line number")
    );
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    vim.feed("`[").unwrap();
    assert_eq!(vim.state().message(), Some("E20: Mark not set"));
}

#[test]
fn leaving_insert_mode_marks_where_it_began_whatever_was_erased() {
    // `'[` stays where typing began, as nvim's, though `<BS>` erased text
    // before it.
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    vim.feed("i<BS><Esc>").unwrap();
    let marks = ['[', ']', '^'].map(|name| vim.mark(name));
    assert_eq!(marks, [Some((1, 2)), Some((1, 1)), Some((1, 1))]);
}

#[test]
fn a_clone_adopts_insert_marks_only_if_nothing_newer_set_them() {
    use std::{cell::Cell, cell::RefCell, rc::Rc};

    use fe_command::Invocation;
    use fe_editor::commands::HeadlessHost;

    use super::Measuring;
    use crate::{Registers, VimTarget, context, registry};

    // The outer state ends the session while a clone, ending it too, makes
    // a newer change before the outcome is published: the clone keeps that
    // change's marks.
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
            let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
            for keys in ["<Esc>", "x"] {
                let invocation = match keys {
                    "<Esc>" => Invocation::of("vim.normal_mode").unwrap(),
                    keys => super::commit(keys),
                };
                let invocation = invocation.context(context(&clone));
                let mut target = VimTarget {
                    model: &mut split,
                    host: &mut host,
                    state: &mut clone,
                    registers: &mut registers,
                };
                registry().dispatch(&mut target, &invocation).unwrap();
            }
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    assert!(ended.get());
    let mut clone = Rc::try_unwrap(clone).ok().unwrap().into_inner();
    let snapshot = vim.model().snapshot().clone();
    let at = |state: &crate::VimState, name| state.mark(name).and_then(|mark| mark.get(&snapshot));
    let newer = (at(&clone, '['), at(&clone, ']'));
    assert!(clone.closing.is_some());
    crate::insert::adopt_closing(&mut clone);
    assert_eq!((at(&clone, '['), at(&clone, ']')), newer);
    assert_ne!(newer, (at(vim.state(), '['), at(vim.state(), ']')));
}

/// Carried T159 F2: `:delmarks ^` and `:delmarks!` in a clone ending an
/// insert session, before its outcome is published, stay done once the
/// clone adopts it: `'^` isn't set again.
#[test]
fn deleting_the_insert_mark_while_a_clone_closes_keeps_it_deleted() {
    use std::{cell::Cell, cell::RefCell, rc::Rc};

    use super::Measuring;

    let deletes: [fn(&mut crate::VimState); 2] = [
        |state| crate::marks::delete(state, '^'),
        crate::marks::delete_lowercase,
    ];
    for delete in deletes {
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
                dispatch(&mut clone, &mut split, &["<Esc>"]);
                delete(&mut clone);
            })
        };
        vim.feed("<Esc>").unwrap();
        drop(measuring);
        assert!(ended.get());
        let mut clone = Rc::try_unwrap(clone).ok().unwrap().into_inner();
        assert!(vim.state().mark('^').is_some());
        assert!(clone.mark('^').is_none());
        assert!(clone.closing.is_some());
        crate::insert::adopt_closing(&mut clone);
        assert!(clone.mark('^').is_none());
    }
}

#[test]
fn a_blocks_end_follows_edits_made_while_measuring() {
    use super::Measuring;

    // Another view puts a row in above while `Esc` types on the block's
    // other rows: `']` stays at the end of what went in on its last row.
    let mut vim = HeadlessVim::new("abcd\nefgh\nijkl", (0, 1));
    vim.feed("<C-v>jIX").unwrap();
    let mut split = vim.model().split();
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let measuring = {
        let done = std::rc::Rc::clone(&done);
        Measuring::new(move |edited| {
            if edited && !done.replace(true) {
                split.buffer_mut().edit([(0..0, "new\n")], ai()).unwrap();
            }
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    assert!(done.get());
    let text = vim.text();
    let row = text.lines().position(|line| line == "eXfgh").unwrap() as u32;
    assert_eq!(vim.mark(']'), Some((row, 2)));
}

/// Runs `keys` on `state` in `model`, one committed invocation each (`Esc`
/// leaving insert mode), as another view sharing the buffer would.
fn dispatch(state: &mut crate::VimState, model: &mut fe_editor::EditorModel, keys: &[&str]) {
    use fe_command::Invocation;
    use fe_editor::commands::HeadlessHost;

    use crate::{Registers, VimTarget, context, registry};

    let (mut host, mut registers) = (HeadlessHost::new(), Registers::new());
    for keys in keys {
        let invocation = match *keys {
            "<Esc>" => Invocation::of("vim.normal_mode").unwrap(),
            keys => super::commit(keys),
        };
        let invocation = invocation.context(context(state));
        let mut target = VimTarget {
            model,
            host: &mut host,
            state,
            registers: &mut registers,
        };
        registry().dispatch(&mut target, &invocation).unwrap();
    }
}

/// What a clone of the state did: the state, `'[`, `']`, `'.` and `'^` as
/// it left them, in the text as it was then.
type Cloned = (crate::VimState, [Option<(u32, usize)>; 4]);

/// `keys` from `cursor` in `text`, then `Esc`: as its first edit catches
/// the model up (measuring text), a clone of the state ends the session
/// too, in another view of the buffer, and runs `after` there before the
/// outcome is published. Returns the editor, and the clone with its marks
/// as `after` left them, the outcome yet to be adopted.
fn closed_by_a_clone(
    text: &str,
    cursor: (u32, usize),
    keys: &str,
    after: &'static [&'static str],
) -> (HeadlessVim, Cloned) {
    use std::{cell::RefCell, rc::Rc};

    use super::Measuring;

    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    let cloned = Rc::new(RefCell::new(None));
    let measuring = {
        let mut clone = Some(vim.state().clone());
        let mut split = vim.model().split();
        split.set_cursor(vim.model().primary_selection().head());
        let cloned = Rc::clone(&cloned);
        Measuring::new(move |edited| {
            let Some(mut clone) = clone.take_if(|_| edited) else {
                return;
            };
            dispatch(&mut clone, &mut split, &["<Esc>"]);
            dispatch(&mut clone, &mut split, after);
            let snapshot = split.snapshot();
            let marks = ['[', ']', '.', '^']
                .map(|name| clone.mark(name).and_then(|mark| mark.get(snapshot)));
            *cloned.borrow_mut() = Some((clone, marks));
        })
    };
    vim.feed("<Esc>").unwrap();
    drop(measuring);
    let cloned = cloned
        .borrow_mut()
        .take()
        .expect("the clone ended the session");
    assert!(
        cloned.0.closing.is_some(),
        "the outcome was yet to be published"
    );
    (vim, cloned)
}

/// `'[`, `']`, `'.` and `'^` in `state`, in `vim`'s text.
fn special(vim: &HeadlessVim, state: &crate::VimState) -> [Option<(u32, usize)>; 4] {
    let snapshot = vim.model().snapshot();
    ['[', ']', '.', '^'].map(|name| state.mark(name).and_then(|mark| mark.get(snapshot)))
}

#[test]
fn a_clone_keeps_a_yank_made_while_its_session_closed() {
    // The clone yanks after leaving the session, before the outcome is
    // published: adopting it keeps the yank's `'[` and `']`, but takes the
    // session's `'.` and `'^`, which a yank doesn't set (nvim's `3ix<Esc>`
    // then `yiw`).
    let (vim, (mut clone, yanked)) = closed_by_a_clone("ab\ncd", (0, 1), "3ix", &["yiw"]);
    assert_eq!(yanked[..2], [Some((0, 0)), Some((0, 4))]);
    crate::insert::adopt_closing(&mut clone);
    let left = special(&vim, vim.state());
    assert_eq!(left[2], Some((0, 3)));
    assert_ne!(left[..2], yanked[..2]);
    assert_eq!(
        special(&vim, &clone),
        [yanked[0], yanked[1], left[2], left[3]]
    );
}

#[test]
fn a_clone_keeps_one_end_set_while_its_session_closed() {
    // `m[` sets `'[` alone: `']` is the session's.
    let (vim, (mut clone, set)) = closed_by_a_clone("ab\ncd", (0, 1), "3ix", &["0", "m["]);
    assert_eq!(set[0], Some((0, 0)));
    crate::insert::adopt_closing(&mut clone);
    let left = special(&vim, vim.state());
    assert_eq!(special(&vim, &clone), [set[0], left[1], left[2], left[3]]);
}

#[test]
fn the_rest_of_esc_doesnt_overwrite_a_clones_newer_marks() {
    // `Esc` takes the unused indentation out of the last copy's row after
    // the clone yanked: that batch, made after the yank but older than it,
    // leaves the yank's `'[` and `']`. `'.` is the session's: on that row,
    // as `Esc` took indentation out there (nvim's `2Ax<CR><Esc>` on an
    // indented row, then `yiw` on the first).
    let (vim, (mut clone, yanked)) =
        closed_by_a_clone("  one\nsix", (0, 4), "2Ax<CR>", &["gg", "yiw"]);
    assert_eq!(vim.text(), "  onex\n  x\n\nsix");
    assert_eq!(yanked[..2], [Some((0, 2)), Some((0, 5))]);
    crate::insert::adopt_closing(&mut clone);
    let left = special(&vim, vim.state());
    assert_eq!(left[2], Some((2, 0)));
    assert_eq!(
        special(&vim, &clone),
        [yanked[0], yanked[1], left[2], left[3]]
    );
}

#[test]
fn a_clone_keeps_a_change_made_while_its_session_closed() {
    // A change sets `'[`, `']` and `'.`: only `'^` is the session's.
    let (vim, (mut clone, changed)) = closed_by_a_clone("ab\ncd", (0, 1), "3ix", &["x"]);
    crate::insert::adopt_closing(&mut clone);
    let left = special(&vim, vim.state());
    assert_eq!(
        special(&vim, &clone),
        [changed[0], changed[1], changed[2], left[3]]
    );
}

/// `keys` from `cursor` in `text`, then `Esc`; then a clone of the state
/// made before it leaves the session too, in another view, and adopts the
/// outcome: the editor's `'[`, `']`, `'.` and `'^`, and the clone's.
fn adopted(text: &str, cursor: (u32, usize), keys: &str) -> [[Option<(u32, usize)>; 4]; 2] {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.feed(keys).unwrap();
    let mut clone = vim.state().clone();
    let mut split = vim.model().split();
    split.set_cursor(vim.model().primary_selection().head());
    vim.feed("<Esc>").unwrap();
    dispatch(&mut clone, &mut split, &["<Esc>"]);
    [special(&vim, vim.state()), special(&vim, &clone)]
}

#[test]
fn a_clone_adopts_the_change_mark_of_a_count_or_a_block() {
    // `'.` where the last copy or the block's rows put it, as nvim's (the
    // corpus' `count_insert_change` and `block_I_change`), in the
    // clone that adopts the session as in the editor that ended it.
    let [ended, clone] = adopted(TEXT, (1, 2), "3ixy");
    assert_eq!(ended[2], Some((1, 6)));
    assert_eq!(clone, ended);
    let [ended, clone] = adopted(TEXT, (1, 2), "3ia<CR>");
    assert_eq!(ended[2], Some((3, 1)));
    assert_eq!(clone, ended);
    let [ended, clone] = adopted("abcd\nefgh\nijkl", (0, 1), "<C-v>jIX");
    assert_eq!(ended[2], Some((1, 0)));
    assert_eq!(clone, ended);
}

#[test]
fn a_clone_reads_the_change_mark_with_the_batch() {
    // A clone that hasn't left the session reads `'.` from the batches,
    // the last one noted with where it goes.
    let mut vim = HeadlessVim::new(TEXT, (1, 2));
    vim.feed("3ia<CR>").unwrap();
    let clone = vim.state().clone();
    vim.feed("<Esc>").unwrap();
    assert_eq!(special(&vim, &clone)[2], vim.mark('.'));
    assert_eq!(vim.mark('.'), Some((3, 1)));
}

#[test]
fn a_block_yanks_corners_lie_past_a_short_rows_end() {
    // Where the block's column lies past a row's end, so do `'[` and `']`,
    // as nvim's `getpos()` has them: its visual cursor stops there.
    let marks = |text, cursor, keys| {
        let mut vim = HeadlessVim::new(text, cursor);
        vim.feed(keys).unwrap();
        [vim.mark('['), vim.mark(']')]
    };
    assert_eq!(
        marks("abc\nd", (0, 2), "<C-v>jy"),
        [Some((0, 1)), Some((1, 1))]
    );
    assert_eq!(
        marks("abc\nde", (0, 2), "<C-v>jy"),
        [Some((0, 2)), Some((1, 2))]
    );
    assert_eq!(
        marks("abc\n", (0, 2), "<C-v>jy"),
        [Some((0, 0)), Some((1, 0))]
    );
    assert_eq!(
        marks("a\nbcd", (1, 2), "<C-v>ky"),
        [Some((0, 1)), Some((1, 2))]
    );
    assert_eq!(
        marks("abc\ndef", (0, 1), "<C-v>jy"),
        [Some((0, 1)), Some((1, 1))]
    );
}
