//! How the which-key popup shows its list ([`super::which_key_layout`]):
//! keys as glyphs in a column of one width, so the arrows line up, and a
//! group's title in its own colour.

use std::time::Duration;

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_vim::{Remap, RemapMode, WhichKey};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::which_key_layout::{entry_colors, key_glyph, keys_glyphs};
use super::*;
use crate::{EditorTheme, EditorView, init};

const DELAY: Duration = Duration::from_millis(200);

/// A focused editor in a window of its own, vim on, the popup after
/// [`DELAY`], and `remaps` set with `,` as the leader.
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    remaps: &[(&str, &str)],
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new("one\ntwo\nthree"), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.update(|_, cx| {
        set_vim_enabled(cx, true);
        set_vim_which_key(cx, true, DELAY);
    });
    let remaps: Vec<Remap> = (remaps.iter())
        .map(|&(from, to)| Remap {
            modes: vec![RemapMode::Normal],
            from: from.to_string(),
            to: to.to_string(),
            recursive: false,
        })
        .collect();
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
    (view, cx)
}

/// Holds `,` past the delay and draws the popup it shows.
fn leader_popup(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> WhichKey {
    cx.simulate_keystrokes(",");
    cx.executor().advance_clock(DELAY);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| view.which_key_shown().cloned())
        .expect("the leader's popup")
}

#[gpui::test]
fn glyphs_for_special_keys(cx: &mut TestAppContext) {
    let table = [
        ("<Space>", "␣"),
        ("<Tab>", "⇥"),
        ("<CR>", "↵"),
        ("<Esc>", "⎋"),
        ("<BS>", "⌫"),
        ("<Bar>", "|"),
        ("<lt>", "<"),
        ("<Del>", "⌦"),
        ("<Up>", "↑"),
        ("<Down>", "↓"),
        ("<Left>", "←"),
        ("<Right>", "→"),
        // As written.
        ("<C-a>", "<C-a>"),
        ("<M-x>", "<M-x>"),
        ("<D-k>", "<D-k>"),
        ("a", "a"),
        ("W", "W"),
        ("]", "]"),
        ("<F5>", "<F5>"),
    ];
    for (key, glyph) in table {
        assert_eq!(key_glyph(key), glyph, "{key}");
    }
    // A prefix, key by key.
    assert_eq!(keys_glyphs("<Space>f"), "␣f");
    assert_eq!(keys_glyphs(",<C-w><Bar>"), ",<C-w>|");
    assert_eq!(keys_glyphs("<lt>"), "<");

    // Display only: the list keeps vim's notation.
    let (view, cx) = vim_editor(
        cx,
        &[
            ("<leader><Space>", "x"),
            ("<leader><Bar>", "x"),
            ("<leader>a", "x"),
        ],
    );
    let which = leader_popup(&view, cx);
    let keys: Vec<_> = (which.entries.iter())
        .map(|entry| entry.key.as_str())
        .collect();
    assert_eq!(keys, ["<Bar>", "<Space>", "a"]);
    // `|` and `␣` select the same keys: the popup's debug ids keep the notation.
    assert!(cx.debug_bounds("which-key-entry-<Space>").is_some());
    assert!(cx.debug_bounds("which-key-entry-<Bar>").is_some());
}

/// With keys of one, several and glyph widths, every arrow starts at one
/// x, past the widest key.
#[gpui::test]
fn arrows_line_up(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(
        cx,
        &[
            ("<leader>a", "x"),
            ("<leader><Space>", "x"),
            ("<leader><Bar>", "x"),
            ("<leader><C-a>", "x"),
        ],
    );
    // One column: each key on a row of its own.
    cx.simulate_resize(gpui::size(gpui::px(320.), gpui::px(480.)));
    cx.run_until_parked();
    let which = leader_popup(&view, cx);
    assert_eq!(which.entries.len(), 4);
    let bounds = |selector: String, cx: &mut VisualTestContext| {
        (cx.debug_bounds(Box::leak(selector.clone().into_boxed_str())))
            .unwrap_or_else(|| panic!("{selector} paints"))
    };
    let mut xs = Vec::new();
    let mut tops = Vec::new();
    for entry in &which.entries {
        let cell = bounds(format!("which-key-entry-{}", entry.key), cx);
        let arrow = bounds(format!("which-key-arrow-{}", entry.key), cx);
        assert!(
            arrow.left() > cell.left(),
            "{}: {arrow:?} in {cell:?}",
            entry.key
        );
        xs.push(arrow.left());
        tops.push(cell.top());
    }
    assert!(xs.iter().all(|x| *x == xs[0]), "{xs:?}");
    // One column, so the arrows share an x on different rows.
    tops.dedup();
    assert_eq!(tops.len(), 4, "{tops:?}");
    // Past `<C-a>`, the widest: five of the font's characters.
    let cell = bounds("which-key-entry-<C-a>".into(), cx);
    let ch = cx.update(|_, cx| {
        let theme = view.read(cx).theme.clone();
        let text = cx.text_system();
        let font = text.resolve_font(&theme.font());
        text.ch_advance(font, theme.font_size).unwrap()
    });
    assert!(xs[0] - cell.left() >= ch * 5., "{xs:?} {cell:?} {ch:?}");
    // The hint to close it paints below the keys.
    let footer = bounds("which-key-footer".into(), cx);
    let entries = bounds("which-key-entries".into(), cx);
    assert!(footer.top() >= entries.bottom(), "{footer:?} {entries:?}");
}

/// A group's title is the information colour, a map that's also a prefix
/// among them; an item's is the text's. Keys and arrows are alike for both.
#[gpui::test]
fn a_group_title_is_the_group_colour_and_items_are_not(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(
        cx,
        &[
            ("<leader>ff", "x"),
            ("<leader>w", "<Cmd>fe:editor.undo<CR>"),
            ("<leader>t", "x"),
            ("<leader>tt", "dd"),
        ],
    );
    let which = leader_popup(&view, cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    assert_ne!(theme.information, theme.foreground);
    let colors = |key: &str| {
        let entry = (which.entries.iter())
            .find(|entry| entry.key == key)
            .unwrap_or_else(|| panic!("{key} listed"));
        entry_colors(entry, &theme)
    };
    let (f, t, w) = (colors("f"), colors("t"), colors("w"));
    assert_eq!(f.title, theme.information);
    assert_eq!(t.title, theme.information);
    assert_eq!(w.title, theme.foreground);
    for colors in [f, t, w] {
        assert_eq!(colors.key, theme.warning);
        assert_eq!(colors.arrow, theme.gutter_foreground);
    }
    // Items first.
    let keys: Vec<_> = (which.entries.iter())
        .map(|entry| entry.key.as_str())
        .collect();
    assert_eq!(keys, ["w", "f", "t"]);
}
