//! The jump's flash style (`vim.jump_style = "flash"`): `gs{c1}` labels
//! every match of the one char in the view, before and after the caret,
//! nearest first; `gS{c1}` those before it. A label is the counted motion
//! in its target's direction, so operators, a visual mode, `.` and macros
//! see `{count}gs{c1}` or `{count}gS{c1}`.

use fe_vim::{
    HeadlessVim,
    jump::{JumpLabelState, JumpStyle, label::FULL},
};

/// The caret on `C`, the `a`s around it on three rows.
const AROUND: &str = "a b a\nb a C a b\na b a";

fn flash(text: &str, cursor: (u32, usize)) -> HeadlessVim {
    let mut vim = HeadlessVim::new(text, cursor);
    vim.set_window_rows(10);
    vim.state_mut().set_jump_style(JumpStyle::Flash);
    vim
}

fn fed(text: &str, cursor: (u32, usize), keys: &str) -> HeadlessVim {
    let mut vim = flash(text, cursor);
    vim.feed(keys).unwrap();
    vim
}

/// The overlay's labels as `(row, column, label, live)`, in text order.
fn labels(vim: &HeadlessVim) -> Vec<(u32, usize, String, bool)> {
    let text = vim.model().text();
    let overlay = vim.jump_overlay().expect("a jump is pending");
    (overlay.labels.into_iter())
        .map(|label| {
            let point = text.offset_to_point(label.offset);
            let live = label.state == JumpLabelState::Live;
            (point.row, point.column as usize, label.text, live)
        })
        .collect()
}

fn live(labels: &[(u32, usize, &str)]) -> Vec<(u32, usize, String, bool)> {
    (labels.iter())
        .map(|&(row, column, label)| (row, column, label.to_string(), true))
        .collect()
}

#[test]
fn flash_one_char_labels_both_directions() {
    // One char labels them all at once, nearest first: the two on the
    // caret's row (the one after it first), then the next rows' by column.
    let want = live(&[
        (0, 0, "l"),
        (0, 4, "j"),
        (1, 2, "f"),
        (1, 6, "s"),
        (2, 0, "k"),
        (2, 4, "n"),
    ]);
    let vim = fed(AROUND, (1, 4), "gsa");
    assert_eq!(labels(&vim), want);
    assert_eq!(vim.state().jump_query(), Some("a"));
    assert_eq!(
        (vim.cursor(), vim.text(), vim.mode()),
        ((1, 4), AROUND.into(), "normal")
    );
    // As `s` does when it starts the jump, and `<CR>` repeats the last.
    let mut vim = flash(AROUND, (1, 4));
    vim.state_mut().set_jump_on_s(true);
    vim.feed("sa").unwrap();
    assert_eq!(labels(&vim), want);
    vim.feed("<Esc>gs<CR>").unwrap();
    assert_eq!(labels(&vim), want);
    // `gS` labels those before the caret only.
    let vim = fed(AROUND, (1, 4), "gSa");
    assert_eq!(labels(&vim), live(&[(0, 0, "n"), (0, 4, "f"), (1, 2, "s")]));

    // A label jumps, either way, setting the jump mark.
    for (label, to) in [("s", (1, 6)), ("f", (1, 2)), ("n", (2, 4)), ("l", (0, 0))] {
        let vim = fed(AROUND, (1, 4), &format!("gsa{label}"));
        assert_eq!((vim.cursor(), vim.mode()), (to, "normal"), "{label}");
        assert_eq!(vim.mark('\''), Some((1, 4)), "{label}");
        assert_eq!(vim.jump_overlay(), None, "{label}");
    }

    // A count before the trigger is the counted motion: no labels.
    let vim = fed(AROUND, (1, 4), "2gsa");
    assert_eq!((vim.cursor(), vim.jump_overlay()), ((2, 0), None));
    let vim = fed(AROUND, (1, 4), "3gSa");
    assert_eq!((vim.cursor(), vim.jump_overlay()), ((0, 0), None));

    // As an operator's target: after the caret inclusive, before it
    // exclusive, as `f` and `F` are.
    let vim = fed(AROUND, (1, 4), "dgsas");
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("a b a\nb a  b\na b a".into(), (1, 4))
    );
    let vim = fed(AROUND, (1, 4), "dgsaf");
    assert_eq!(
        (vim.text(), vim.cursor()),
        ("a b a\nb C a b\na b a".into(), (1, 2))
    );
    let vim = fed(AROUND, (1, 4), "ygsaj");
    assert_eq!(vim.registers().get('"').unwrap().text, "a\nb a ");

    // A visual mode extends to the target.
    let vim = fed(AROUND, (1, 4), "vgsan");
    assert_eq!(
        (vim.mode(), vim.visual_anchor(), vim.cursor()),
        ("visual", Some((1, 4)), (2, 4))
    );

    // `.` repeats the counted jump from the caret it repeats at, as the
    // counted motion's `.` does.
    for (keys, counted) in [("dgsak.", "d2gsa."), ("dgsaj.", "d2gSa.")] {
        let picked = fed(AROUND, (1, 4), keys);
        let typed = fed(AROUND, (1, 4), counted);
        assert_eq!(
            (picked.text(), picked.cursor()),
            (typed.text(), typed.cursor()),
            "{keys}"
        );
        assert_ne!(picked.text(), AROUND, "{keys}");
    }

    // A recording holds the jump resolved, and replays it from elsewhere
    // without labels.
    let mut vim = fed(AROUND, (1, 4), "qqgsajq");
    assert_eq!(vim.cursor(), (0, 4));
    assert_eq!(vim.registers().get('q').unwrap().text, "2gSa");
    vim.feed("``@q").unwrap();
    assert_eq!((vim.cursor(), vim.jump_overlay()), ((0, 4), None));
    let mut vim = fed(AROUND, (1, 4), "qqdgsasq");
    assert_eq!(vim.registers().get('q').unwrap().text, "d1gsa");
    vim.feed("u@q").unwrap();
    assert_eq!(vim.text(), "a b a\nb a  b\na b a");
}

#[test]
fn flash_single_match_and_no_match() {
    // One match, either way, jumps at once, no label waiting.
    for (cursor, keys, to) in [
        ((0, 0), "gsq", (1, 2)),
        ((1, 4), "gsq", (1, 2)),
        ((1, 4), "gSq", (1, 2)),
    ] {
        let vim = fed("x y\nz q w", cursor, keys);
        assert_eq!((vim.cursor(), vim.jump_overlay()), (to, None), "{keys}");
        assert_eq!(vim.mark('\''), Some(cursor), "{keys}");
    }
    // After an operator too.
    let vim = fed("x y\nz q w", (0, 0), "dgsq");
    assert_eq!((vim.text(), vim.cursor()), (" w".into(), (0, 0)));

    // None fails with nothing changed, not even the jump mark.
    for keys in ["gsv", "gSw", "dgsv", "vgsv", "3gsq"] {
        let vim = fed("x y\nz q w", (1, 2), keys);
        assert!(vim.state().failed(), "{keys}");
        assert_eq!(vim.text(), "x y\nz q w", "{keys}");
        assert_eq!(vim.cursor(), (1, 2), "{keys}");
        assert_eq!(vim.mark('\''), None, "{keys}");
        assert_eq!(vim.jump_overlay(), None, "{keys}");
        assert!(vim.registers().get('"').is_none(), "{keys}");
    }
    // A match outside the view gets no label: here the only one, so none.
    let mut text = "x\n".repeat(30);
    text.push('q');
    let vim = fed(&text, (0, 0), "gsq");
    assert_eq!((vim.cursor(), vim.jump_overlay()), ((0, 0), None));

    // More matches than labels page with `<Space>` and `<BS>`, round from
    // the last page to the first.
    let full = FULL.chars().count();
    let text = format!("x{}", " a".repeat(full + 6));
    let vim = fed(&text, (0, 0), "gsa");
    let shown = labels(&vim);
    assert_eq!(shown.iter().filter(|label| label.3).count(), full);
    assert_eq!(shown.iter().filter(|label| !label.3).count(), 6);
    assert_eq!(shown[0], (0, 2, "s".into(), true));
    let vim = fed(&text, (0, 0), "gsa<Space>");
    let shown = labels(&vim);
    assert_eq!(shown.iter().filter(|label| label.3).count(), 6);
    assert_eq!(shown[full], (0, 2 * full + 2, "s".into(), true));
    for (keys, to) in [
        ("gsa<Space>s", 2 * full + 2),
        ("gsa<Space><Space>s", 2),
        ("gsa<BS>s", 2 * full + 2),
        ("gsa<Space><BS>f", 4),
    ] {
        let vim = fed(&text, (0, 0), keys);
        assert_eq!(
            (vim.cursor(), vim.jump_overlay()),
            ((0, to), None),
            "{keys}"
        );
    }
}

#[test]
fn flash_leap_style_unchanged() {
    // The default is leap: `gs{c1}` waits for `{c2}`, showing nothing.
    for style in [None, Some(JumpStyle::Leap)] {
        let mut vim = HeadlessVim::new(AROUND, (1, 4));
        vim.set_window_rows(10);
        if let Some(style) = style {
            vim.state_mut().set_jump_style(JumpStyle::Flash);
            vim.state_mut().set_jump_style(style);
        }
        vim.feed("gsa").unwrap();
        assert_eq!(labels(&vim), [], "{style:?}");
        assert_eq!(vim.state().jump_query(), Some("a"), "{style:?}");
        assert_eq!(vim.cursor(), (1, 4), "{style:?}");
        // `{c2}` jumps forward only, to the nearest at once.
        vim.feed(" ").unwrap();
        assert_eq!(vim.cursor(), (1, 6), "{style:?}");
    }
    // A style switched while labels wait leaves them as they started.
    let mut vim = flash(AROUND, (1, 4));
    vim.feed("gsa").unwrap();
    vim.state_mut().set_jump_style(JumpStyle::Leap);
    vim.feed("l").unwrap();
    assert_eq!(vim.cursor(), (0, 0));
    vim.feed("gSa").unwrap();
    assert_eq!(labels(&vim), []);
}
