//! After a jump's first char, the match each `{c2}` would go straight to is
//! marked `Direct`, with every other match of it in view a `Preview`.

use fe_vim::{HeadlessVim, jump::JumpLabelState};

const TEXT: &str = "x wh wa wo www x\nwh w";

fn vim(cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = HeadlessVim::new(TEXT, cursor);
    vim.set_window_rows(10);
    vim.feed(keys).unwrap();
    vim
}

/// The overlay as (offset, the char after it as `{c2}`, text, state).
fn shown(vim: &HeadlessVim) -> Vec<Entry> {
    let overlay = vim.jump_overlay().unwrap();
    overlay
        .labels
        .into_iter()
        .map(|label| {
            let next = TEXT[label.offset + 1..].chars().next();
            let c2 = next.filter(|&c| c != '\n').unwrap_or(' ');
            (label.offset, c2, label.text, label.state)
        })
        .collect()
}

type Entry = (usize, char, String, JumpLabelState);

/// `direct` are marked, with no text, and the rest previews with some.
fn check(shown: &[Entry], direct: &[usize]) {
    for (offset, _, text, state) in shown {
        match direct.contains(offset) {
            true => assert_eq!((text.as_str(), *state), ("", JumpLabelState::Direct)),
            false => {
                assert!(!text.is_empty(), "{offset}");
                assert_eq!(*state, JumpLabelState::Preview, "{offset}");
            }
        }
    }
}

#[test]
fn every_match_shows_after_first_char() {
    // Row 0's offsets: `wh` 2, `wa` 5, `wo` 8, `www` 11-13; row 1 starts
    // at 17: `wh` 17, `w` 20 at the row end.
    let forward = vim((0, 0), "gsw");
    let offsets: Vec<usize> = shown(&forward).iter().map(|label| label.0).collect();
    // Not the `www` run's interior, 12.
    assert_eq!(offsets, [2, 5, 8, 11, 13, 17, 20]);
    // Each group's nearest is direct, the rest previews.
    check(&shown(&forward), &[2, 5, 8, 11, 13]);

    // Backward from the last `w`: `h`'s nearest is row 1's `wh`, and the
    // blank's is `www`'s last.
    let backward = vim((1, 3), "gSw");
    let offsets: Vec<usize> = shown(&backward).iter().map(|label| label.0).collect();
    assert_eq!(offsets, [2, 5, 8, 11, 13, 17]);
    check(&shown(&backward), &[5, 8, 11, 13, 17]);
}

#[test]
fn direct_is_where_c2_lands() {
    let mut vim = vim((0, 0), "gsw");
    let direct = shown(&vim)
        .into_iter()
        .find(|label| label.1 == 'h' && label.3 == JumpLabelState::Direct)
        .unwrap()
        .0;
    vim.feed("h").unwrap();
    let at = vim.model().text().offset_to_point(direct);
    assert_eq!(vim.cursor(), (at.row, at.column as usize));
}
