//! `:earlier`, `:later`, `g-` and `g+` over fe-buffer's linear undo
//! history: by count (as `u` and `<C-r>`), by the time each change was made
//! (through a fake clock on the buffer) and by file write.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use fe_buffer::{LineEnding, Origin};
use fe_vim::HeadlessVim;

const TEXT: &str = "abc\ndef\nghi";

/// Three changes: `x` on each row.
fn three_changes() -> HeadlessVim {
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    vim.feed("xjxjx").unwrap();
    vim
}

/// `vim` after `keys`, against a twin fed `twin` after the same setup: the
/// same text and caret, and neither failed.
fn same(vim: &mut HeadlessVim, keys: &str, twin: &mut HeadlessVim, twin_keys: &str) {
    vim.feed(keys).unwrap();
    twin.feed(twin_keys).unwrap();
    assert_eq!(vim.text(), twin.text(), "{keys} vs {twin_keys}");
    assert_eq!(vim.cursor(), twin.cursor(), "{keys} vs {twin_keys}");
    assert!(!vim.state().failed(), "{keys}");
}

/// Feeds `line` as an ex command, which mustn't fail or say anything.
fn ex(vim: &mut HeadlessVim, line: &str) {
    vim.feed(&format!(":{line}<CR>")).unwrap();
    assert!(!vim.state().failed(), ":{line}");
    assert_eq!(vim.state().message(), None, ":{line}");
}

#[test]
fn earlier_and_later_with_a_count_step_changes() {
    let (mut vim, mut twin) = (three_changes(), three_changes());
    same(&mut vim, ":earlier 2<CR>", &mut twin, "2u");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    same(&mut vim, ":later<CR>", &mut twin, "<C-r>");
    assert_eq!(vim.text(), "bc\nef\nghi");
    same(&mut vim, ":earlier<CR>", &mut twin, "u");
    same(&mut vim, ":lat 2<CR>", &mut twin, "2<C-r>");
    assert_eq!(vim.text(), "bc\nef\nhi");
    same(&mut vim, ":ea 3<CR>", &mut twin, "3u");
    assert_eq!(vim.text(), TEXT);
    // 0 changes stays, as does a count beyond the history.
    ex(&mut vim, "later 0");
    assert_eq!(vim.text(), TEXT);
    same(&mut vim, ":later 1<CR>", &mut twin, "<C-r>");
    // Each is the newest change undone, not a transaction of its own.
    same(&mut vim, ":earlier 1|later 1<CR>", &mut twin, "u<C-r>");
    same(&mut vim, "u", &mut twin, "u");
    assert_eq!(vim.text(), TEXT);
}

#[test]
fn earlier_by_seconds_uses_entry_times() {
    let now = Arc::new(AtomicU64::new(1_000_000));
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    let clock = now.clone();
    vim.model_mut()
        .buffer_mut()
        .set_clock(move || clock.load(Ordering::Relaxed));
    // Changes 0 s, 5 s, 20 s and 30 s in.
    for (at, keys) in [(0, "x"), (5, "jx"), (20, "jx"), (30, "ggx")] {
        now.store(1_000_000 + at * 1_000, Ordering::Relaxed);
        vim.feed(keys).unwrap();
    }
    assert_eq!(vim.text(), "c\nef\nhi");
    // The current state is 30 s in: back to 20 s in undoes only the newest.
    ex(&mut vim, "earlier 10s");
    assert_eq!(vim.text(), "bc\nef\nhi");
    // Now 20 s in: back to 10 s in.
    ex(&mut vim, "earlier 10s");
    assert_eq!(vim.text(), "bc\nef\nghi");
    // Now 5 s in: forward to 20 s in redoes the change made then, no more.
    ex(&mut vim, "later 15s");
    assert_eq!(vim.text(), "bc\nef\nhi");
    // Less than a change's distance goes nowhere.
    ex(&mut vim, "later 9s");
    assert_eq!(vim.text(), "bc\nef\nhi");
    ex(&mut vim, "later 10s");
    assert_eq!(vim.text(), "c\nef\nhi");
    // Minutes reach before the first change; hours back to the newest.
    ex(&mut vim, "earlier 1m");
    assert_eq!(vim.text(), TEXT);
    ex(&mut vim, "later 1h");
    assert_eq!(vim.text(), "c\nef\nhi");
    // The caret is where `u` would leave it.
    let mut twin = HeadlessVim::new(TEXT, (0, 0));
    twin.feed("xjxjxggxuu").unwrap();
    ex(&mut vim, "earlier 25s");
    assert_eq!(vim.text(), twin.text());
    assert_eq!(vim.cursor(), twin.cursor());
    // Days, and as many seconds as fit.
    ex(&mut vim, "later 1d");
    assert_eq!(vim.text(), "c\nef\nhi");
    ex(&mut vim, "earlier 99999999999999999999s");
    assert_eq!(vim.text(), TEXT);
}

#[test]
fn earlier_1f_goes_to_the_previous_write() {
    // Each step as nvim 0.12.4 takes it, the same keys sourced with
    // `--clean --headless`, each change its own undo block.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    let write = |vim: &mut HeadlessVim| vim.model_mut().buffer_mut().did_save();
    vim.feed("x").unwrap();
    write(&mut vim);
    vim.feed("jxjx").unwrap();
    write(&mut vim);
    vim.feed("ggx").unwrap();
    assert_eq!(vim.text(), "c\nef\nhi");
    // Changes since the last write: back to it.
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), "bc\nef\nhi");
    // At a write: back to the one before.
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    // At the first: before the first change.
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), TEXT);
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), TEXT);
    // Forward, write by write, then to the newest state.
    ex(&mut vim, "later 1f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    ex(&mut vim, "later 1f");
    assert_eq!(vim.text(), "bc\nef\nhi");
    ex(&mut vim, "later 1f");
    assert_eq!(vim.text(), "c\nef\nhi");
    // Counts skip writes; past the oldest is before the first change.
    ex(&mut vim, "earlier 2f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    ex(&mut vim, "later 5f");
    assert_eq!(vim.text(), "c\nef\nhi");
    ex(&mut vim, "earlier 3f");
    assert_eq!(vim.text(), TEXT);
    // `0f` is the newest write at or before the current state: none here.
    ex(&mut vim, "later 0f");
    assert_eq!(vim.text(), TEXT);
    ex(&mut vim, "later");
    ex(&mut vim, "earlier 0f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    ex(&mut vim, "later 1f|later 1f|earlier 0f");
    assert_eq!(vim.text(), "bc\nef\nhi");

    // A write the history can't reach any more, its state discarded as a
    // redo by a newer change, is skipped.
    ex(&mut vim, "later|earlier 2f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    vim.feed("Gx").unwrap();
    assert_eq!(vim.text(), "bc\ndef\nhi");
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    ex(&mut vim, "earlier 0f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), TEXT);
    ex(&mut vim, "later 1f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    // No write at all: nowhere to go for `0f`, before the first change for
    // `1f`.
    let mut unsaved = three_changes();
    ex(&mut unsaved, "earlier 0f");
    assert_eq!(unsaved.text(), "bc\nef\nhi");
    ex(&mut unsaved, "earlier 1f");
    assert_eq!(unsaved.text(), TEXT);
    ex(&mut unsaved, "later 1f");
    assert_eq!(unsaved.text(), "bc\nef\nhi");
}

#[test]
fn writes_count_in_the_order_they_were_made() {
    // Three changes each saved, two undone, and a save of the first again.
    let setup = || {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        for keys in ["x", "jx", "jx"] {
            vim.feed(keys).unwrap();
            vim.model_mut().buffer_mut().did_save();
        }
        vim.feed("2u").unwrap();
        vim.model_mut().buffer_mut().did_save();
        assert_eq!(vim.text(), "bc\ndef\nghi");
        vim
    };
    // Back from the last write goes to the third, later in the history
    // (as nvim 0.12.4 does), then the second and the first.
    let mut vim = setup();
    for text in ["bc\nef\nhi", "bc\nef\nghi", "bc\ndef\nghi", TEXT, TEXT] {
        ex(&mut vim, "earlier 1f");
        assert_eq!(vim.text(), text);
    }
    // And forward again, write by write, then to the newest state.
    for text in [
        "bc\ndef\nghi",
        "bc\nef\nghi",
        "bc\nef\nhi",
        "bc\ndef\nghi",
        "bc\nef\nhi",
    ] {
        ex(&mut vim, "later 1f");
        assert_eq!(vim.text(), text);
    }
    // From the third change, reached by redo, forward is the last write
    // (as nvim 0.12.4 does).
    let mut vim = setup();
    ex(&mut vim, "later 2");
    ex(&mut vim, "later 1f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    ex(&mut vim, "earlier 2f");
    assert_eq!(vim.text(), "bc\nef\nghi");
    ex(&mut vim, "later 0f");
    assert_eq!(vim.text(), "bc\nef\nghi");

    // Leaving the write last gone to by undo and coming back by redo counts
    // from the newest write of the state again: the fourth, not the first
    // gone to (as nvim 0.12.4 does).
    let mut vim = setup();
    for _ in 0..3 {
        ex(&mut vim, "earlier 1f");
    }
    assert_eq!(vim.text(), "bc\ndef\nghi");
    vim.feed("u<C-r>").unwrap();
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), "bc\nef\nhi");

    // So does an undo and redo of a step that changes only the line
    // ending: the first write is forgotten, and back from the third is the
    // second.
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    let save = |vim: &mut HeadlessVim| vim.model_mut().buffer_mut().did_save();
    vim.model_mut()
        .buffer_mut()
        .change_line_ending(LineEnding::CrLf, Origin::User)
        .unwrap();
    save(&mut vim);
    vim.feed("x").unwrap();
    save(&mut vim);
    vim.feed("u").unwrap();
    save(&mut vim);
    ex(&mut vim, "earlier 1f|earlier 1f");
    assert_eq!(vim.text(), TEXT);
    assert_eq!(vim.model().buffer().line_ending(), LineEnding::CrLf);
    vim.feed("u").unwrap();
    assert_eq!(vim.model().buffer().line_ending(), LineEnding::Lf);
    vim.feed("<C-r>").unwrap();
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), "bc\ndef\nghi");

    // A save after going to a write counts from that save: back is the
    // write before it, not the one before the write gone to (each step as
    // nvim 0.12.4 takes it).
    let mut vim = HeadlessVim::new(TEXT, (0, 0));
    for keys in ["x", "jx"] {
        vim.feed(keys).unwrap();
        vim.model_mut().buffer_mut().did_save();
    }
    ex(&mut vim, "earlier 1f");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    vim.model_mut().buffer_mut().did_save();
    for (line, text) in [
        ("earlier 1f", "bc\nef\nghi"),
        ("earlier 1f", "bc\ndef\nghi"),
        ("later 1f", "bc\nef\nghi"),
    ] {
        ex(&mut vim, line);
        assert_eq!(vim.text(), text, ":{line}");
    }
}

#[test]
fn a_conflict_part_way_fails() {
    let ai = Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    // Two changes, the older's text then overwritten by another origin.
    let setup = || {
        let mut vim = HeadlessVim::new(TEXT, (0, 0));
        vim.feed("iXY<Esc>jx").unwrap();
        // Part of the inserted text, and some after it.
        vim.model_mut()
            .buffer_mut()
            .edit([(1..3, "ZZ")], ai.clone())
            .unwrap();
        assert_eq!(vim.text(), "XZZbc\ndf\nghi");
        vim
    };
    for keys in [
        "2g-",
        ":earlier 2<CR>",
        ":earlier 1m<CR>",
        ":earlier 1f<CR>",
    ] {
        let mut vim = setup();
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), "XZZbc\ndef\nghi", "{keys}");
        assert!(vim.state().failed(), "{keys}");
    }
    // A piped command after the failure doesn't run.
    let mut vim = setup();
    vim.feed(":earlier 2|later 1<CR>").unwrap();
    assert_eq!(vim.text(), "XZZbc\ndef\nghi");
    assert!(vim.state().failed());
}

#[test]
fn later_past_the_newest_stops_quietly() {
    let mut vim = three_changes();
    for line in ["later", "later 5", "later 1s", "later 1f", "later 0f"] {
        ex(&mut vim, line);
        assert_eq!(vim.text(), "bc\nef\nhi", ":{line}");
    }
    ex(&mut vim, "earlier 7");
    assert_eq!(vim.text(), TEXT);
    for line in ["earlier", "earlier 5", "earlier 1d", "earlier 1f"] {
        ex(&mut vim, line);
        assert_eq!(vim.text(), TEXT, ":{line}");
    }
    ex(&mut vim, "later 99999999999999999999");
    assert_eq!(vim.text(), "bc\nef\nhi");
    // Where `u` and `<C-r>` fail for running out.
    vim.feed("<C-r>").unwrap();
    assert!(vim.state().failed());
    // A buffer with no history at all.
    let mut fresh = HeadlessVim::new(TEXT, (1, 1));
    for line in [
        "earlier",
        "later 3",
        "earlier 10s",
        "later 1h",
        "earlier 2f",
    ] {
        ex(&mut fresh, line);
        assert_eq!(fresh.text(), TEXT, ":{line}");
        assert_eq!(fresh.cursor(), (1, 1), ":{line}");
    }
}

#[test]
fn invalid_argument_is_e475() {
    for (line, arg) in [
        ("earlier x", "x"),
        ("later 1x", "1x"),
        ("earlier 1 s", "1 s"),
        ("earlier -1", "-1"),
        ("later +2", "+2"),
        ("earlier s", "s"),
        ("later 1sf", "1sf"),
        ("ea 1.5m", "1.5m"),
    ] {
        let mut vim = three_changes();
        let (text, cursor) = (vim.text(), vim.cursor());
        vim.feed(&format!(":{line}<CR>")).unwrap();
        assert_eq!(vim.text(), text, ":{line}");
        assert_eq!(vim.cursor(), cursor, ":{line}");
        let message = vim.state().message().unwrap_or_default();
        assert!(
            message.contains(&format!("E475: Invalid argument: {arg}")),
            ":{line}: {message}"
        );
    }
    // Neither takes a `!` or a range.
    for (line, error) in [
        ("earlier! 1", "E477"),
        ("later!", "E477"),
        ("2earlier", "E481"),
        ("%later 1", "E481"),
    ] {
        let mut vim = three_changes();
        let text = vim.text();
        vim.feed(&format!(":{line}<CR>")).unwrap();
        assert_eq!(vim.text(), text, ":{line}");
        let message = vim.state().message().unwrap_or_default();
        assert!(message.contains(error), ":{line}: {message}");
    }
}

#[test]
fn g_minus_and_g_plus_with_a_count() {
    let (mut vim, mut twin) = (three_changes(), three_changes());
    same(&mut vim, "2g-", &mut twin, "2u");
    assert_eq!(vim.text(), "bc\ndef\nghi");
    same(&mut vim, "g+", &mut twin, "<C-r>");
    same(&mut vim, "g-", &mut twin, "u");
    same(&mut vim, "2g+", &mut twin, "2<C-r>");
    assert_eq!(vim.text(), "bc\nef\nhi");
    // Past the oldest stops there without failing, where `u` fails.
    same(&mut vim, "5g-", &mut twin, "3u");
    assert_eq!(vim.text(), TEXT);
    same(&mut vim, "g-", &mut twin, "");
    same(&mut vim, "9g+", &mut twin, "3<C-r>");
    assert_eq!(vim.text(), "bc\nef\nhi");
    same(&mut vim, "g+", &mut twin, "");

    // `.` repeats the change before, not `g-`.
    let (mut vim, mut twin) = (three_changes(), three_changes());
    same(&mut vim, "g-.", &mut twin, "u.");
    // After an operator, or in a visual mode, they cancel.
    let mut vim = three_changes();
    for keys in ["dg-", "cg+", "vg-<Esc>", "Vg+<Esc>"] {
        vim.feed(keys).unwrap();
        assert_eq!(vim.text(), "bc\nef\nhi", "{keys}");
        assert_eq!(vim.mode(), "normal", "{keys}");
    }
}
