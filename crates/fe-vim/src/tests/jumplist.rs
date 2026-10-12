//! Tests of the jumplist: `<C-o>`, `<C-i>` and `<Tab>` (see `src/jumplist.rs`).

use fe_command::{ContextPredicate, Effect};
use serde_json::json;

use super::{commit, invocation, run};
use crate::{HeadlessVim, Pending, Step, jumplist::JumpArgs, parse_keys, registry, step};

const ROWS: &str = "one\ntwo\n  three\nfour (x\ny) five\nsix\nseven\neight\nnine";

fn at(cursor: (u32, usize)) -> HeadlessVim {
    HeadlessVim::new(ROWS, cursor)
}

/// The cursor after each of `keys` (split on spaces), fed in turn.
fn cursors(vim: &mut HeadlessVim, keys: &str) -> Vec<(u32, usize)> {
    keys.split(' ')
        .map(|keys| {
            vim.feed(keys).unwrap();
            vim.cursor()
        })
        .collect()
}

/// `n` rows, `row N` each.
fn rows(n: u32) -> String {
    (0..n)
        .map(|row| format!("row {row}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn keys_commit_the_jump_commands() {
    for (keys, command, args) in [
        ("<C-o>", "vim.jump_older", json!({})),
        ("3<C-o>", "vim.jump_older", json!({"count": 3})),
        ("<C-i>", "vim.jump_newer", json!({})),
        ("<Tab>", "vim.jump_newer", json!({})),
        ("12<Tab>", "vim.jump_newer", json!({"count": 12})),
    ] {
        let committed = commit(keys);
        assert_eq!(committed.command.as_str(), command, "{keys}");
        assert_eq!(committed.args, args, "{keys}");
    }
    assert_eq!(
        serde_json::from_value::<JumpArgs>(json!({"count": 2})).unwrap(),
        JumpArgs { count: Some(2) }
    );
}

#[test]
fn keys_arent_taken_in_visual_or_operator_pending_modes() {
    // After an operator or a register, they cancel what's pending.
    for keys in ["d<C-o>", "y2<Tab>", "c<C-i>", "\"a<C-o>"] {
        assert_eq!(run(keys), Step::Cancel, "{keys}");
    }
    // In a visual mode they aren't vim's.
    for key in ["<C-o>", "<C-i>", "<Tab>"] {
        let key = parse_keys(key).unwrap()[0];
        assert_eq!(step(Pending::visual(), key), Step::Forward, "{key:?}");
    }
    let mut vim = at((0, 0));
    vim.feed("Gggvj").unwrap();
    assert!(vim.feed("<C-o>").is_err());
    assert_eq!((vim.mode(), vim.cursor()), ("visual", (1, 0)));
    vim.feed("<Esc>d").unwrap();
    vim.feed("<C-o>").unwrap();
    assert_eq!((vim.mode(), vim.cursor()), ("normal", (1, 0)));
    assert_eq!(vim.text(), ROWS);
}

#[test]
fn commands_are_registered_for_normal_mode() {
    let registry = registry();
    for id in ["vim.jump_older", "vim.jump_newer"] {
        let meta = registry.get(&id.parse().unwrap()).expect(id);
        assert_eq!(
            meta.context,
            Some(ContextPredicate::parse("editor && vim.mode == normal").unwrap()),
            "{id}"
        );
        assert!(!meta.mutates_buffer, "{id}");
        assert!(meta.args_schema.is_some(), "{id}");
    }

    // Dispatched as the host would.
    let mut vim = at((0, 0));
    vim.feed("G").unwrap();
    let older = invocation("vim.jump_older", JumpArgs { count: None });
    let dispatched = vim.dispatch(&older).unwrap();
    assert_eq!(dispatched.effect, Effect::Changed);
    assert!(dispatched.transactions.is_empty());
    assert_eq!(vim.cursor(), (0, 0));
    let newer = invocation("vim.jump_newer", JumpArgs { count: Some(1) });
    vim.dispatch(&newer).unwrap();
    assert_eq!(vim.cursor(), (8, 0));
    assert!(!vim.model().is_dirty());
}

#[test]
fn back_and_forward_to_exact_positions() {
    let mut vim = at((1, 2));
    // `G` and `gg` leave from (1, 2) and (8, 2); the first `<C-o>` adds
    // (0, 2), where it goes from, so `<Tab>` comes back to it.
    vim.feed("Ggg").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <Tab> <C-i>"),
        [(8, 2), (1, 2), (8, 2), (0, 2)]
    );
    // Counts: from the oldest entry, two forward; then two back.
    let mut vim = at((1, 2));
    vim.feed("Ggg").unwrap();
    assert_eq!(
        cursors(&mut vim, "2<C-o> 2<Tab> 2<C-o>"),
        [(1, 2), (0, 2), (1, 2)]
    );
    // `j` after one keeps the entry's column as its goal.
    let mut vim = HeadlessVim::new("ab\nabcdef\nabc", (1, 5));
    vim.feed("G<C-o>").unwrap();
    assert_eq!(vim.cursor(), (1, 5));
    vim.feed("j").unwrap();
    assert_eq!(vim.cursor(), (2, 2));
}

#[test]
fn only_the_first_ctrl_o_sets_the_jump_mark() {
    let mut vim = at((1, 2));
    vim.feed("Ggg").unwrap();
    assert_eq!(vim.mark('\''), Some((8, 2)));
    vim.feed("<C-o>").unwrap();
    assert_eq!(vim.mark('\''), Some((0, 2)));
    vim.feed("<C-o><Tab><Tab>").unwrap();
    assert_eq!(vim.cursor(), (0, 2));
    assert_eq!(vim.mark('\''), Some((0, 2)));
    // `''` is a jump: it adds where it left from.
    vim.feed("G''").unwrap();
    assert_eq!(vim.cursor(), (0, 0));
    vim.feed("<C-o>").unwrap();
    assert_eq!(vim.cursor(), (8, 2));
    // Landing where it left from, the jump mark goes back, as a jump's does.
    let mut vim = HeadlessVim::new("a\nb\nc\nd\ne\nf\ng\nh", (2, 0));
    vim.feed("5G6Ggg4Gdd").unwrap();
    assert_eq!(vim.mark('\''), Some((0, 0)));
    vim.feed("3<C-o>").unwrap();
    assert_eq!(vim.cursor(), (3, 0));
    assert_eq!(vim.mark('\''), Some((0, 0)));
}

#[test]
fn every_jump_adds_where_it_left_from() {
    let text = "alpha (one\ntwo) beta\ngamma alpha\ndelta\nalpha end";
    for (cursor, keys, from) in [
        ((3, 1), "gg", (3, 1)),
        ((0, 1), "G", (0, 1)),
        ((0, 6), "%", (0, 6)),
        ((0, 0), "/delta<CR>", (0, 0)),
        ((4, 0), "?gamma<CR>", (4, 0)),
        ((1, 0), "/alpha<CR>n", (2, 6)),
        ((1, 0), "/alpha<CR>N", (2, 6)),
        ((0, 0), "*", (0, 0)),
        ((4, 0), "#", (4, 0)),
        ((3, 2), "maggj'a", (1, 2)),
        ((3, 2), "maggj`a", (1, 2)),
        ((3, 2), "ggj''", (1, 2)),
    ] {
        let mut vim = HeadlessVim::new(text, cursor);
        vim.feed(keys).unwrap();
        vim.feed("<C-o>").unwrap();
        assert_eq!(vim.cursor(), from, "{keys}");
    }
    // `m'` adds one too; `ma` and `:{N}` don't.
    let mut vim = at((1, 2));
    vim.feed("Gkm'gg").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <C-o>"),
        [(7, 2), (1, 2), (1, 2)]
    );
    let mut vim = at((1, 2));
    vim.feed("Gkma:3<CR>gg").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <C-o>"),
        [(2, 2), (1, 2), (1, 2)]
    );
    // Nor does a search that finds nothing or `%` with no bracket.
    let mut vim = at((1, 2));
    vim.feed("G").unwrap();
    assert!(vim.feed("/nothing<CR>").is_ok());
    vim.feed("%").unwrap();
    assert_eq!(cursors(&mut vim, "<C-o> <C-o>"), [(1, 2), (1, 2)]);
}

#[test]
fn a_global_adds_one_jump_not_its_commands() {
    let text = "ab\nabcdef\nabc\nabcdefgh\nx\ny\nz\nw";
    let mut vim = HeadlessVim::new(text, (1, 5));
    vim.feed("Gkkkk:g/x/s/x/q/<CR>jgg").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <C-o> <C-o>"),
        [(5, 0), (3, 5), (1, 5), (1, 5)]
    );
}

#[test]
fn an_earlier_entry_on_a_later_ones_row_goes() {
    let text = "abcdef\nabcdef\nabcdef\nabcdef\nabcdef";
    // From (0, 0) and (0, 5): `<C-o>` goes back to row 0 once.
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.feed("Ggg$G").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <Tab>"),
        [(0, 5), (0, 5), (4, 5)]
    );
    // Where the newest entry is on the cursor's row, the first `<C-o>`
    // puts the cursor's position in its place.
    let mut vim = HeadlessVim::new(text, (0, 0));
    vim.feed("3lG0G$").unwrap();
    assert_eq!(
        cursors(&mut vim, "2<C-o> <C-o> <Tab>"),
        [(4, 5), (0, 3), (4, 5)]
    );
}

#[test]
fn at_most_a_hundred_entries() {
    let jumps: String = (2..=110).map(|row| format!("{row}G")).collect();
    let mut vim = HeadlessVim::new(&rows(120), (0, 0));
    vim.feed(&jumps).unwrap();
    // Rows 0 to 8 went; the first `<C-o>` adds row 109, and row 9 goes.
    vim.feed("100<C-o>").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.cursor(), (109, 0));
    vim.feed("99<C-o>").unwrap();
    assert_eq!(vim.cursor(), (10, 0));
    vim.feed("99<Tab>").unwrap();
    assert_eq!(vim.cursor(), (109, 0));
}

#[test]
fn entries_follow_edits() {
    // Rows put in above move them down.
    let mut vim = at((2, 0));
    vim.feed("5GggOx<Esc>").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <Tab>"),
        [(5, 0), (3, 0), (5, 0)]
    );
    // Another origin's edit too.
    let mut vim = at((2, 3));
    vim.feed("Ggg").unwrap();
    vim.model_mut()
        .buffer_mut()
        .edit(
            [(0..0, "new\nrows\n")],
            fe_buffer::Origin::Ai {
                agent: "agent".into(),
                session: "session".into(),
            },
        )
        .unwrap();
    assert_eq!(cursors(&mut vim, "<C-o> <C-o>"), [(10, 3), (4, 3)]);
}

#[test]
fn a_deleted_rows_entry_goes_to_the_row_that_took_its_place() {
    // The entry left from (1, 0), whose row `dd` deleted, is on the row
    // after it, as nvim's is: not skipped.
    let mut vim = HeadlessVim::new("a\nb\nc\nd\ne\nf\ng\nh", (1, 0));
    vim.feed("5GggjddjG").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <C-o> <C-o>"),
        [(2, 0), (3, 0), (1, 0), (1, 0)]
    );
    assert_eq!(vim.text(), "a\nc\nd\ne\nf\ng\nh");
    // At the same column, clamped as a caret is.
    let text = "ab\nabcdef\nabc\nabcdefgh\nx\ny\nz\nw";
    let mut vim = HeadlessVim::new(text, (1, 5));
    // `:2d` adds a jump from (3, 5), which the one `gg` adds from its row
    // replaces.
    vim.feed("Gkkkk:2d<CR>jgg").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <C-o> <C-o>"),
        [(2, 0), (1, 2), (1, 2), (1, 2)]
    );
    // `cc` over rows keeps the first: the others' entries go to the row
    // after them.
    let text = "abcdef\nabcdef\nabcdef\nabcdef\nabcdef\nabcdef";
    let mut vim = HeadlessVim::new(text, (2, 4));
    vim.feed("Gggj3cc<Esc>G3<C-o>").unwrap();
    assert_eq!(vim.cursor(), (2, 4));
}

#[test]
fn an_entry_past_the_last_row_is_e19() {
    let mut vim = HeadlessVim::new("a\nb\nc\nd\ne\nf\ng\nh", (0, 0));
    vim.feed("5GGggGddgg<C-o>").unwrap();
    assert_eq!(vim.cursor(), (6, 0));
    let mark = vim.mark('\'');
    vim.feed("<C-o>").unwrap();
    assert!(vim.state().failed());
    assert_eq!(
        vim.state().message(),
        Some("E19: Mark has invalid line number")
    );
    assert_eq!((vim.cursor(), vim.mark('\'')), ((6, 0), mark));
    // It's passed over: the next `<C-o>` goes on.
    vim.feed("<C-o>").unwrap();
    assert_eq!(vim.cursor(), (4, 0));
    // `dG` from the second row: its entry is past the last row.
    let mut vim = HeadlessVim::new("ab\nabcdef\nabc", (1, 5));
    vim.feed("dG<C-o>").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("E19: Mark has invalid line number")
    );
    assert_eq!((vim.text().as_str(), vim.cursor()), ("ab", (0, 1)));
}

#[test]
fn past_either_end_fails_with_nothing_changed() {
    // No jumps yet.
    let mut vim = at((2, 3));
    for keys in ["<C-o>", "<Tab>", "<C-i>"] {
        vim.feed(keys).unwrap();
        assert!(vim.state().failed(), "{keys}");
        assert_eq!((vim.cursor(), vim.mark('\'')), ((2, 3), None), "{keys}");
    }
    let mut vim = at((2, 3));
    vim.feed("Ggg").unwrap();
    // Not gone back yet.
    vim.feed("<Tab>").unwrap();
    assert!(vim.state().failed());
    assert_eq!((vim.cursor(), vim.mark('\'')), ((0, 2), Some((8, 3))));
    // Too far back: nothing added, so a count that fits still works.
    vim.feed("3<C-o>").unwrap();
    assert!(vim.state().failed());
    assert_eq!((vim.cursor(), vim.mark('\'')), ((0, 2), Some((8, 3))));
    vim.feed("2<C-o>").unwrap();
    assert!(!vim.state().failed());
    assert_eq!(vim.cursor(), (2, 3));
    // Too far forward.
    vim.feed("3<Tab>").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.cursor(), (2, 3));
    vim.feed("2<Tab>").unwrap();
    assert_eq!(vim.cursor(), (0, 2));
    vim.feed("<Tab>").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.cursor(), (0, 2));
    assert_eq!(vim.text(), ROWS);
}

#[test]
fn a_failing_jump_stops_a_macro() {
    let mut vim = at((2, 3));
    vim.feed("qaGq").unwrap();
    vim.feed("gg").unwrap();
    // `<C-o>` past the oldest entry ends the replay: `x` doesn't run.
    vim.feed("qb3<C-o>xq").unwrap();
    assert_eq!(vim.text(), ROWS.replacen("one", "on", 1));
    let text = vim.text();
    vim.feed("@b").unwrap();
    assert_eq!(vim.text(), text);
}

#[test]
fn a_failed_jump_still_drops_the_newest_entry_on_the_cursors_row() {
    // `G` from the last row adds (7, 0): `<Tab>` (or a `<C-o>` too far)
    // fails, but drops it first, so after `k` `<C-o>` goes to (2, 0).
    for keys in ["<Tab>", "<C-i>", "5<C-o>"] {
        let mut vim = HeadlessVim::new("a\nb\nc\nd\ne\nf\ng\nh", (2, 0));
        vim.feed("GG").unwrap();
        vim.feed(keys).unwrap();
        assert!(vim.state().failed(), "{keys}");
        assert_eq!(vim.cursor(), (7, 0), "{keys}");
        assert_eq!(
            cursors(&mut vim, "k <C-o> <C-o>"),
            [(6, 0), (2, 0), (2, 0)],
            "{keys}"
        );
    }
    // Once gone back, the newest entry stays.
    let mut vim = HeadlessVim::new("a\nb\nc\nd\ne\nf\ng\nh", (0, 0));
    vim.feed("3G5G7G<C-o>2j").unwrap();
    assert_eq!(cursors(&mut vim, "<Tab> <C-o>"), [(6, 0), (4, 0)]);
}

#[test]
fn a_global_adds_one_entry_from_where_it_began() {
    // One entry each: 51 of them don't push out the first two.
    let globals = ":g/x/<CR>".repeat(51);
    let mut vim = HeadlessVim::new("a\nx\nc\nd\ne\nf\ng\nh", (2, 0));
    vim.feed(&format!("G{globals}gg2<C-o>")).unwrap();
    assert_eq!(vim.cursor(), (7, 0));
    // Made before its commands run, it follows them: its row deleted, it's
    // past the last row.
    let mut vim = HeadlessVim::new("abcdef\nabcdef\nabcdef\nzzzzzz", (3, 2));
    vim.feed(":g/z/d<CR><C-o>").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("E19: Mark has invalid line number")
    );
    assert_eq!(vim.cursor(), (2, 0));
    // And it keeps its column through its row's getting shorter.
    let text = "abcdef\nabcdef\nabcdef\nabcdef";
    let mut vim = HeadlessVim::new(text, (1, 5));
    vim.feed(":g/abcdef/s/abcdef/x/<CR>kk0i0123456789<Esc>j<C-o>")
        .unwrap();
    assert_eq!(vim.cursor(), (1, 5));
}

#[test]
fn entries_past_the_last_row_are_all_on_one_row() {
    // `3dd` takes the last three rows, and the entries from (3, 4) and
    // (5, 4) with them: past the last row, they're one entry, as nvim's are.
    let text = "abcdef\nabcdef\nabcdef\nabcdef\nabcdef\nabcdef";
    let mut vim = HeadlessVim::new(text, (1, 4));
    vim.feed("4G6Gggjjj3ddgg<C-o>").unwrap();
    assert_eq!(vim.cursor(), (2, 4));
    vim.feed("3<C-o>").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.cursor(), (2, 4));
    vim.feed("<C-o>").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("E19: Mark has invalid line number")
    );
    assert_eq!(vim.cursor(), (2, 4));
    vim.feed("<C-o>").unwrap();
    assert_eq!(vim.cursor(), (1, 4));
    // Counted from the newest end, the second back is the one entry past
    // the last row.
    let mut vim = HeadlessVim::new(text, (1, 4));
    vim.feed("4G6Gggjjj3ddgg2<C-o>").unwrap();
    assert_eq!(
        vim.state().message(),
        Some("E19: Mark has invalid line number")
    );
    assert_eq!(vim.cursor(), (0, 4));
}

#[test]
fn ctrl_o_in_a_global_adds_nothing() {
    // Before it, the list is (2, 4), (5, 4), and the `:g`'s own (0, 4). The
    // `<C-o>` its `:normal` runs adds nothing but counts as if it had, so it
    // goes to (5, 4), and the list goes on from there.
    let text = "abcdef\nx\nabcdef\nabcdef\nabcdef\nabcdef";
    let global = |keys: &str| {
        let mut vim = HeadlessVim::new(text, (2, 4));
        vim.feed(&format!("Ggg:g/x/normal! {keys}<CR>")).unwrap();
        vim
    };
    let mut vim = global("<lt>C-o>");
    assert_eq!(vim.cursor(), (5, 4));
    assert_eq!(vim.mark('\''), Some((0, 4)));
    assert_eq!(cursors(&mut vim, "<C-o> <C-o>"), [(2, 4), (2, 4)]);
    let mut vim = global("<lt>C-o>");
    assert_eq!(cursors(&mut vim, "<Tab> <Tab>"), [(0, 4), (0, 4)]);
    assert_eq!(global("2<lt>C-o>").cursor(), (2, 4));
    // Too far: it fails where the `:g` left the cursor.
    assert_eq!(global("3<lt>C-o>").cursor(), (1, 0));
    // A jump in it adds nothing either.
    let mut vim = global("G<lt>C-o>");
    assert_eq!(cursors(&mut vim, "<C-o> <C-o>"), [(2, 4), (2, 4)]);
    // Outside a `:g`, `:normal`'s `<C-o>` adds where it goes from.
    let mut vim = HeadlessVim::new(text, (2, 4));
    vim.feed("Ggg:normal! <lt>C-o><CR>").unwrap();
    assert_eq!(
        cursors(&mut vim, "<C-o> <Tab> <Tab>"),
        [(2, 4), (5, 4), (0, 4)]
    );
}

#[test]
fn a_count_past_the_oldest_after_the_first_step_keeps_the_step() {
    // In a `:g`, `3<C-o>` steps onto the `:g`'s own entry, adding nothing,
    // then fails: the step stays, so `<C-o>` then goes on to (5, 4) and
    // `<Tab>` finds nothing newer. `9<C-o>` fails before it.
    let text = "abcdef\nx\nabcdef\nabcdef\nabcdef\nabcdef";
    let global = |keys: &str| {
        let mut vim = HeadlessVim::new(text, (2, 4));
        vim.feed(&format!("Ggg:g/x/normal! {keys}<CR>")).unwrap();
        assert_eq!(vim.cursor(), (1, 0), "{keys}");
        vim
    };
    let mut vim = global("3<lt>C-o>");
    assert_eq!(
        cursors(&mut vim, "<C-o> <C-o> <C-o>"),
        [(5, 4), (2, 4), (2, 4)]
    );
    let mut vim = global("3<lt>C-o>");
    vim.feed("<Tab>").unwrap();
    assert!(vim.state().failed());
    assert_eq!(vim.cursor(), (1, 0));
    let mut vim = global("9<lt>C-o>");
    vim.feed("<C-o>").unwrap();
    assert_eq!(vim.cursor(), (0, 4));
    assert_eq!(vim.mark('\''), Some((1, 0)));
    // With a hundred entries, `100<C-o>` adds where it goes from (the
    // oldest going) and steps onto it before it fails, leaving the jump
    // mark: after `k`, `<C-o>` goes on from there.
    let jumps: String = (2..=110).map(|row| format!("{row}G")).collect();
    for (count, back_to) in [(100, 108), (101, 107)] {
        let mut vim = HeadlessVim::new(&rows(120), (0, 0));
        vim.feed(&format!("{jumps}{count}<C-o>")).unwrap();
        assert!(vim.state().failed(), "{count}");
        assert_eq!(
            (vim.cursor(), vim.mark('\'')),
            ((109, 0), Some((108, 0))),
            "{count}"
        );
        vim.feed("k<C-o>").unwrap();
        assert_eq!(vim.cursor(), (back_to, 0), "{count}");
    }
}
