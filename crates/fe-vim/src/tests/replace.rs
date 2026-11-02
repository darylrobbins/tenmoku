use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use fe_buffer::{Origin, ResolvedSelection};
use fe_command::{ContextPredicate, DispatchError, Invocation};
use fe_editor::{EditorModel, commands::HeadlessHost};

use super::{Measuring, commit};
use crate::{
    CountArgs, HeadlessVim, Mode, Registers, Splits, VimState, VimTarget, context, registry,
    replace::{COPIES_EDITED, again, copies},
};

/// The user's `u`, with the transactions it undid.
fn undo(vim: &mut HeadlessVim) -> usize {
    let invocation = Invocation::of("vim.undo")
        .unwrap()
        .context(context(vim.state()));
    vim.dispatch(&invocation).unwrap().transactions.len()
}

#[test]
fn session_overwrite_and_backspace() {
    // `R` commits `vim.replace_mode`, with its count.
    let invocation = commit("3R");
    assert_eq!(invocation.command.as_str(), "vim.replace_mode");
    assert_eq!(
        serde_json::from_value::<CountArgs>(invocation.args).unwrap(),
        CountArgs { count: Some(3) }
    );
    let registry = registry();
    let meta = registry.get(&"vim.replace_mode".parse().unwrap()).unwrap();
    assert_eq!(
        meta.context,
        Some(ContextPredicate::parse("editor && vim.mode == normal").unwrap())
    );
    assert!(meta.mutates_buffer);
    let meta = registry
        .get(&"vim.replace_normal_mode".parse().unwrap())
        .unwrap();
    assert_eq!(
        meta.context,
        Some(ContextPredicate::parse("editor && vim.mode == replace").unwrap())
    );

    // In replace mode `vim.mode` is `replace`, and typing overwrites as
    // one undo step, `Esc` stepping back onto the last typed.
    let mut vim = HeadlessVim::new("abcd", (0, 0));
    vim.feed("R").unwrap();
    assert_eq!(vim.state().mode(), Mode::Replace);
    assert_eq!(vim.mode(), "replace");
    let stamped = context(vim.state());
    let editor = stamped
        .frames()
        .iter()
        .find(|frame| frame.name == "editor")
        .unwrap();
    assert_eq!(editor.get("vim.mode"), Some("replace"));
    // `vim.normal_mode` is insert mode's: `Esc` here is its own command.
    let normal_mode = Invocation::of("vim.normal_mode")
        .unwrap()
        .context(context(vim.state()));
    assert!(matches!(
        vim.dispatch(&normal_mode),
        Err(DispatchError::NotInContext { .. })
    ));
    vim.feed("xy<Esc>").unwrap();
    assert_eq!(vim.text(), "xycd");
    assert_eq!(vim.mode(), "normal");
    assert_eq!(vim.cursor(), (0, 1));
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "abcd");
    assert_eq!(vim.cursor(), (0, 0));

    // Past the row's end it appends.
    let mut vim = HeadlessVim::new("ab\ncd", (0, 1));
    vim.feed("Rxyz<Esc>").unwrap();
    assert_eq!(vim.text(), "axyz\ncd");
    assert_eq!(vim.cursor(), (0, 3));

    // `<BS>` puts back what was overwritten, takes out what was appended,
    // and stops at the session's start, there or mid-row.
    let mut vim = HeadlessVim::new("abcd", (0, 0));
    vim.feed("Rxy<BS>").unwrap();
    assert_eq!(vim.text(), "xbcd");
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "abcd");
    assert_eq!(vim.model().primary_selection().head(), 0);
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "abcd");
    assert_eq!(vim.model().primary_selection().head(), 0);
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.cursor(), (0, 0));
    let mut vim = HeadlessVim::new("ab", (0, 1));
    vim.feed("Rxyz<BS><BS>").unwrap();
    assert_eq!(vim.text(), "ax");
    vim.feed("<BS><BS>").unwrap();
    assert_eq!(vim.text(), "ab");
    assert_eq!(vim.model().primary_selection().head(), 1);
    vim.feed("<BS>z<Esc>").unwrap();
    assert_eq!(vim.text(), "az");
    let mut vim = HeadlessVim::new("abcd", (0, 2));
    vim.feed("Rxy<BS><BS><BS>").unwrap();
    assert_eq!(vim.text(), "abcd");
    assert_eq!(vim.model().primary_selection().head(), 2);
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.cursor(), (0, 1));

    // A line break splits the row, overwriting nothing; `<BS>` joins it
    // again.
    let mut vim = HeadlessVim::new("abcd", (0, 1));
    vim.feed("Rx<CR>y").unwrap();
    assert_eq!(vim.text(), "ax\nyd");
    vim.feed("<BS><BS>").unwrap();
    assert_eq!(vim.text(), "axcd");
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.cursor(), (0, 1));

    // A count overwrites with the typed text again, as one undo step.
    let mut vim = HeadlessVim::new("123456789", (0, 0));
    vim.feed("3Rab<Esc>").unwrap();
    assert_eq!(vim.text(), "ababab789");
    assert_eq!(vim.cursor(), (0, 5));
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "123456789");

    // `.` repeats it, with its count or a new one.
    let mut vim = HeadlessVim::new("abcdefgh", (0, 0));
    vim.feed("2Rxy<Esc>0.").unwrap();
    assert_eq!(vim.text(), "xyxyefgh");
    vim.feed("03.").unwrap();
    assert_eq!(vim.text(), "xyxyxygh");
    assert_eq!(vim.cursor(), (0, 5));
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "xyxyefgh");

    // The largest count's copies are one edit at the caret, as a small
    // count's are, and stop at the bytes a put may insert. (Four-byte
    // characters, so stepping back over the row they make, which measures
    // every grapheme on it, takes a quarter of the time.)
    let copies = |count: u32| {
        let mut vim = HeadlessVim::new("abc", (0, 0));
        vim.feed(&format!("{count}R𝒳𝒴<Esc>")).unwrap();
        (vim.text(), COPIES_EDITED.get())
    };
    assert_eq!(copies(3), ("𝒳𝒴".repeat(3), 1));
    let started = Instant::now();
    let (text, edited) = copies(u32::MAX);
    assert!(started.elapsed() < Duration::from_secs(60));
    assert_eq!(edited, 1);
    // The typed text, then the copies, over the rest of the row.
    assert_eq!(text.len(), "𝒳𝒴".len() + crate::operator::MAX_PUT);
    assert!(text.starts_with("𝒳𝒴𝒳𝒴"));

    // `replace_text` is what `HeadlessVim` types through in replace mode.
    let mut model = EditorModel::new(fe_buffer::Buffer::new("abcd"), None);
    let mut host = HeadlessHost::new();
    let mut state = VimState::new();
    let mut registers = Registers::new();
    let mut target = VimTarget {
        model: &mut model,
        host: &mut host,
        state: &mut state,
        registers: &mut registers,
    };
    let invocation = Invocation::of("vim.replace_mode")
        .unwrap()
        .context(context(target.state));
    registry.dispatch(&mut target, &invocation).unwrap();
    state.replace_text(&mut model, "xy");
    assert_eq!(model.text().to_string(), "xycd");
    state.replace_backspace(&mut model);
    assert_eq!(model.text().to_string(), "xbcd");
    // Outside replace mode they type as the editor does.
    let mut model = EditorModel::new(fe_buffer::Buffer::new("ab"), None);
    VimState::new().replace_text(&mut model, "x");
    assert_eq!(model.text().to_string(), "xab");
}

#[test]
fn splits_and_carets() {
    // A cursor key splits the session as it does an insert session's: the
    // mode stays replace, what came before is its own undo step and no
    // longer put back by `<BS>`.
    let mut vim = HeadlessVim::new("abcdef", (0, 0));
    vim.feed("Rx").unwrap();
    vim.split_session(Splits::Moves, |model| model.set_cursor(3));
    assert_eq!(vim.mode(), "replace");
    vim.feed("y<BS><BS>z<Esc>").unwrap();
    assert_eq!(vim.text(), "xbczef");
    assert_eq!(vim.mode(), "normal");
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "xbcdef");
    assert_eq!(undo(&mut vim), 1);
    assert_eq!(vim.text(), "abcdef");

    // Every caret overwrites, puts back and takes the count's copies.
    let mut vim = HeadlessVim::new("abcd\nefgh", (0, 0));
    vim.feed("R").unwrap();
    vim.model_mut()
        .set_selections([1, 6].map(|offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
    vim.feed("xyz<BS>").unwrap();
    assert_eq!(vim.text(), "axyd\nexyh");
    vim.feed("<Esc>").unwrap();
    assert_eq!(vim.text(), "axyd\nexyh");
}

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

fn carets(vim: &mut HeadlessVim, offsets: &[usize]) {
    vim.model_mut()
        .set_selections(offsets.iter().map(|&offset| ResolvedSelection {
            range: offset..offset,
            reversed: false,
        }));
}

#[test]
fn backspace_takes_back_only_the_sessions_own() {
    // Another origin's edit over what was typed: `<BS>` leaves it, and
    // everything typed before it.
    let mut vim = HeadlessVim::new("abcd", (0, 0));
    vim.feed("Rxy").unwrap();
    vim.model_mut().buffer_mut().edit([(1..2, "!")], ai());
    vim.feed("<BS><BS>").unwrap();
    assert_eq!(vim.text(), "x!cd");
    // Over what was appended.
    let mut vim = HeadlessVim::new("ab", (0, 1));
    vim.feed("Rxyz").unwrap();
    vim.model_mut().buffer_mut().edit([(3..4, "Z")], ai());
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "axyZ");
    // Over the indentation a line break copied.
    let mut vim = HeadlessVim::new("  abcd", (0, 3));
    vim.feed("Rx<CR>").unwrap();
    assert_eq!(vim.text(), "  ax\n  cd");
    vim.model_mut().buffer_mut().edit([(5..6, "-")], ai());
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "  ax\n- cd");

    // A typed grapheme of several characters goes whole, putting back what
    // its last overwrote, as nvim's `<BS>` does.
    let mut vim = HeadlessVim::new("abcdef", (0, 0));
    vim.feed("Re\u{301}x<BS><BS>").unwrap();
    assert_eq!(vim.text(), "bcdef");
    vim.feed("<BS><Esc>").unwrap();
    assert_eq!(vim.text(), "bcdef");

    // The indentation a line break copied goes whole, whatever its white
    // space, then the line break.
    let mut vim = HeadlessVim::new("\u{3000}\u{3000}abcd", (0, 7));
    vim.feed("Rx<CR>").unwrap();
    assert_eq!(vim.text(), "\u{3000}\u{3000}ax\n\u{3000}\u{3000}cd");
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "\u{3000}\u{3000}ax\ncd");
    vim.feed("<BS><BS><Esc>").unwrap();
    assert_eq!(vim.text(), "\u{3000}\u{3000}abcd");
}

#[test]
fn overlapping_carets_and_line_endings() {
    // A caret whose overwrite the one before took joins it: no record of
    // its own, and `<BS>` takes back the other's.
    let mut vim = HeadlessVim::new("abcd", (0, 0));
    vim.feed("R").unwrap();
    carets(&mut vim, &[0, 1]);
    // A clone shares the session and its record.
    let state = vim.state().clone();
    state.replace_text(vim.model_mut(), "xy");
    assert_eq!(vim.text(), "xycd");
    assert_eq!(vim.model().selections().len(), 1);
    assert_eq!(vim.model().primary_selection().head(), 2);
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "xbcd");
    vim.feed("<BS><Esc>").unwrap();
    assert_eq!(vim.text(), "abcd");
    // A shorter grapheme over a longer one, at two carets, with a count.
    let mut vim = HeadlessVim::new("日本\n日本", (0, 0));
    vim.feed("2R").unwrap();
    carets(&mut vim, &[0, 7]);
    vim.feed("x<BS>y<Esc>").unwrap();
    assert_eq!(vim.text(), "yy\nyy");

    // Any line ending is a line break, overwriting nothing.
    for ending in ["\r\n", "\r", "\n"] {
        let mut vim = HeadlessVim::new("abcd", (0, 1));
        vim.feed("R").unwrap();
        let state = vim.state().clone();
        state.replace_text(vim.model_mut(), &format!("x{ending}y"));
        assert_eq!(vim.text(), "ax\nyd", "{ending:?}");
        vim.feed("<BS><BS><BS><Esc>").unwrap();
        assert_eq!(vim.text(), "abcd", "{ending:?}");
    }
}

#[test]
fn copies_with_line_breaks_are_bounded() {
    // Once a copy types what the one before did, the rest go in at once,
    // up to the budget, however large the count.
    let started = Instant::now();
    let (copied, over) = copies("  ", &["a", "b"], "x\n", u32::MAX as usize, 1 << 20);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(copied.len() <= 1 << 20);
    assert!(copied.starts_with("x\n  x\n  x\n  "));
    assert_eq!(over, 2);
    // Blanks typed after a line break widen the next row's indentation, so
    // no two copies are alike: they stop at the budget.
    let (copied, _) = copies("", &[], "\n ", u32::MAX as usize, 1 << 16);
    assert!(copied.len() <= 1 << 16);
    assert!(copied.starts_with("\n \n  \n   "));
}

#[test]
fn backspace_owns_whole_graphemes_and_follows_other_edits() {
    // A grapheme the session typed only part of: `<BS>` takes out only its
    // own character, so it never reaches another origin's text or what was
    // there before the session began.
    let mut vim = HeadlessVim::new("abcdef", (0, 0));
    vim.feed("Re\u{301}").unwrap();
    vim.model_mut().buffer_mut().edit([(0..1, "q")], ai());
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "qbcdef");
    let mut vim = HeadlessVim::new("abcd", (0, 1));
    vim.feed("R\u{301}").unwrap();
    assert_eq!(vim.text(), "a\u{301}cd");
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "abcd");
    assert_eq!(vim.model().primary_selection().head(), 1);

    // Text another origin changed, even back to what it was, is no longer
    // the session's to take back.
    let mut vim = HeadlessVim::new("abcd", (0, 0));
    vim.feed("Rxy").unwrap();
    vim.model_mut().buffer_mut().edit([(1..2, "!")], ai());
    vim.model_mut().buffer_mut().edit([(1..2, "y")], ai());
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "xycd");

    // Indentation another origin edits as the model catches up with the
    // line break, through another model of the buffer.
    let mut vim = HeadlessVim::new("  abcd", (0, 3));
    vim.feed("Rx").unwrap();
    let mut split = vim.model().split();
    let edited = Rc::new(Cell::new(false));
    let measuring = {
        let edited = Rc::clone(&edited);
        Measuring::new(move |after_edit| {
            if after_edit && !edited.replace(true) {
                split.buffer_mut().edit([(5..6, "-")], ai());
            }
        })
    };
    vim.feed("<CR>").unwrap();
    drop(measuring);
    assert!(edited.get());
    assert_eq!(vim.text(), "  ax\n- cd");
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "  ax\n- cd");
}

#[test]
fn largest_counts_taking_back_line_breaks_return() {
    // Enter, then `<BS>` taking out its indentation: past the first copy,
    // whose `<BS>` takes out the line break itself, every copy leaves the
    // text as it is, so the largest count returns as a small one does
    // (`count_indent_bs_no_growth` and the others in the corpus), as a
    // session's count and as `.`'s.
    let largest = |buffer: &str, cursor: (u32, usize), keys: &str| {
        let started = Instant::now();
        let mut vim = HeadlessVim::new(buffer, cursor);
        vim.feed(keys).unwrap();
        assert!(started.elapsed() < Duration::from_secs(30), "{keys}");
        (vim.text(), vim.cursor())
    };
    let max = u32::MAX;
    assert_eq!(
        largest("  abc", (0, 2), &format!("{max}R<CR><BS><Esc>")),
        ("  \nabc".to_string(), (1, 0))
    );
    assert_eq!(
        largest("  abc", (0, 2), &format!("{max}R<CR><BS><BS><Esc>")),
        ("  abc".to_string(), (0, 1))
    );
    assert_eq!(
        largest("  abc\n  def", (0, 2), &format!("R<CR><BS><Esc>j^{max}.")),
        ("  \nabc\n  \ndef".to_string(), (3, 0))
    );
    // A combining mark joining the indentation's last blank, taken back
    // with it, then the rest of the indentation.
    assert_eq!(
        largest(
            "  abcdefgh",
            (0, 2),
            &format!("{max}R<CR>\u{301}<BS><BS><Esc>")
        ),
        ("  \nabcdefgh".to_string(), (1, 0))
    );
    assert_eq!(
        largest(
            "  abc\n  defgh",
            (0, 2),
            &format!("R<CR>\u{301}<BS><BS><Esc>j^{max}.")
        ),
        ("  \nabc\n  \ndefgh".to_string(), (3, 0))
    );
    // Without the rest of the indentation, each copy adds a line break, up
    // to the budget.
    let started = Instant::now();
    let (copied, over) = copies(" ", &["a", "b"], "\n\u{301}\u{8}", max as usize, 1 << 16);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!((copied, over), ("\n".repeat(1 << 16), 0));

    // Copies that shrink the text: each cuts more than it adds, until
    // there's no more to cut, at once.
    for (out, piece, cut) in [
        ("abcdefxy", "xy", 3),
        ("abcdefxy", "xy", 5),
        ("xy", "xy", 3),
    ] {
        for left in [0, 1, 2, 100] {
            let mut slow = out.to_string();
            let mut done = 0;
            while done < left && slow.len() >= cut {
                slow.truncate(slow.len() - cut);
                slow.push_str(piece);
                done += 1;
            }
            let mut fast = out.to_string();
            assert_eq!(
                (again(&mut fast, piece, cut, 1 << 10, left), fast),
                (done, slow),
                "{out} {piece} {cut} {left}"
            );
        }
    }
}

#[test]
fn combining_marks_take_back_the_indentations_last_blank() {
    // A combining mark joins the last blank of the indentation a line break
    // copied: `<BS>` takes them both, the line break staying, puts back
    // what the mark overwrote, then takes out the rest of the indentation,
    // then the line break.
    let mut vim = HeadlessVim::new("    abc", (0, 4));
    vim.feed("R<CR>\u{301}").unwrap();
    assert_eq!(vim.text(), "    \n    \u{301}bc");
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "    \n   abc");
    assert_eq!(vim.model().primary_selection().head(), 8);
    vim.feed("\u{301}\u{302}<BS>").unwrap();
    assert_eq!(vim.text(), "    \n  bc");
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "    \nbc");
    vim.feed("<BS>").unwrap();
    assert_eq!(vim.text(), "    bc");
    vim.feed("<BS><Esc>").unwrap();
    assert_eq!(vim.text(), "    bc");
    assert_eq!(vim.cursor(), (0, 3));

    // Typed again, the keys take a blank wherever the mark joins one: `.`
    // on a deeper row, and none on a row without indentation.
    let mut vim = HeadlessVim::new("  abc\n    def\nghi", (0, 2));
    vim.feed("R<CR>\u{301}<BS><Esc>j^.").unwrap();
    assert_eq!(vim.text(), "  \n abc\n    \n   def\nghi");
    vim.feed("j0.").unwrap();
    assert_eq!(vim.text(), "  \n abc\n    \n   def\n\nghi");
    // With a count, the copies take one blank each until none are left.
    let mut vim = HeadlessVim::new("    abcdefgh", (0, 4));
    vim.feed("4R<CR>\u{301}<BS>x<Esc>").unwrap();
    assert_eq!(vim.text(), "    \n   x\n  x\n x\nxefgh");
    // Taking all of a row's indentation, then the line break, leaves the
    // keys: typed on a deeper row they leave a line break there.
    let mut vim = HeadlessVim::new(" abc\n  def", (0, 1));
    vim.feed("R<CR>\u{301}<BS><BS><Esc>").unwrap();
    assert_eq!(vim.text(), " abc\n  def");
    vim.feed("j^.").unwrap();
    assert_eq!(vim.text(), " abc\n  \ndef");
}

#[test]
fn copies_take_back_whole_graphemes() {
    // A copy's `<BS>` takes the whole grapheme it typed before the caret,
    // as the session's does, putting back only what its last character
    // overwrote: after a line break joined again, and for a mark taken
    // back with it (nvim 0.12.4, `count_mark_cr_bs_*` in the corpus).
    let typed = |buffer: &str, keys: &str| {
        let mut vim = HeadlessVim::new(buffer, (0, 0));
        vim.feed(keys).unwrap();
        (vim.text(), vim.cursor())
    };
    let cases = [
        (
            "abcdefgh",
            "2Re\u{301}<CR><BS>\u{302}<BS><Esc>",
            "efgh",
            (0, 0),
        ),
        ("abcdefgh", "2Re\u{301}<CR><BS><BS><Esc>", "cdefgh", (0, 0)),
        (
            "abcdefghijk",
            "3Re\u{301}<CR><BS><BS><Esc>",
            "defghijk",
            (0, 0),
        ),
        (
            "abcdefgh\nabcdefghijkl",
            "Re\u{301}<CR><BS>\u{302}<BS><Esc>j03.",
            "cdefgh\nghijkl",
            (1, 0),
        ),
        (
            "abcdefgh\nabcdefghijkl",
            "Re\u{301}<CR><BS><BS><Esc>j03.",
            "bcdefgh\ndefghijkl",
            (1, 0),
        ),
    ];
    // Graphemes found in all the row the copy typed, not from the end:
    // a ZWJ sequence goes whole, regional indicators go in their pairs.
    let more = [
        (
            "abcdefghijk",
            "2R👩\u{200d}🚀<CR><BS><BS><Esc>",
            "efghijk",
            (0, 0),
        ),
        (
            "abcdefghijk",
            "2R🇦🇧🇨<CR><BS><BS><Esc>",
            "🇦🇧🇦🇧efghijk",
            (0, 8),
        ),
        (
            "abcdefghijk\nabcdefghijklmnop",
            "R👩\u{200d}🚀<CR><BS><BS><Esc>j03.",
            "cdefghijk\nghijklmnop",
            (1, 0),
        ),
    ];
    for (buffer, keys, text, cursor) in cases.into_iter().chain(more) {
        assert_eq!(typed(buffer, keys), (text.to_string(), cursor), "{keys}");
    }
    // The simulation alone: each copy types `e` and a mark over two
    // graphemes, and takes both back, putting back the second.
    let (copied, over) = copies(
        "",
        &["a", "b", "c", "d"],
        "e\u{301}\n\u{8}\u{8}",
        2,
        1 << 10,
    );
    assert_eq!((copied, over), (String::new(), 2));
    // Both ways a copy takes back: a `<BS>` after a line break joined
    // again, and characters typed and taken back by one `<BS>`, whose
    // grapheme the regional indicator before them may pair into.
    let rest = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let (copied, _) = copies("", &rest, "🇦🇧🇨\n\u{8}\u{8}", 2, 1 << 10);
    assert_eq!(copied, "🇦🇧🇦🇧");
    let (copied, over) = copies("", &rest, "🇦\n\u{8}🇧\u{8}", 2, 1 << 10);
    assert_eq!((copied, over), (String::new(), 2));
    let (copied, over) = copies("", &rest, "🇦🇧\n\u{8}🇨\u{8}", 1, 1 << 10);
    assert_eq!((copied, over), ("🇦🇧".to_string(), 2));
}

#[test]
fn virtual_breaks_mixing_replaced_and_added_rows() {
    // A caret whose break replaces an existing row keeps its indentation at
    // `Esc`, though another caret's, at the last row, adds one whose
    // indentation goes.
    let mut vim = HeadlessVim::new("  abc\nxx\n  def", (0, 2));
    vim.feed("gR").unwrap();
    carets(&mut vim, &[2, 11]);
    vim.feed("<CR><Esc>").unwrap();
    assert_eq!(vim.text(), "  \n  \n  \n");
}
