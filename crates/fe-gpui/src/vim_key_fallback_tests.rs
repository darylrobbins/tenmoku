//! Keys the default keymaps leave unbound under vim (shift-tab, alt-up,
//! alt-down and the duplicate-row shortcut) reach vim's own key handler
//! and do what vim does with them, on both platform keymaps.

use std::{ops::Range, path::Path};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_LINUX, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{EditorTheme, EditorView, init_with_keymap, set_vim_enabled};

fn keymap(source: &str) -> Keymap {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), source, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers(vec![layer.unwrap()])
}

/// A focused editor on `text`, vim on, with `source` as the keymap.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let map = keymap(source);
    cx.update(|cx| init_with_keymap(cx, map));
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

/// The primary selection's head as a row and a byte column.
fn head(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let point = model
            .text()
            .offset_to_point(model.primary_selection().head());
        (point.row, point.column)
    })
}

fn rows(count: u32) -> String {
    (0..count)
        .map(|row| format!("row {row}"))
        .collect::<Vec<_>>()
        .join("\n")
}

const PLATFORMS: [(&str, &str, &str); 2] = [
    ("macOS", DEFAULT_MACOS, "shift-alt-down"),
    ("Linux", DEFAULT_LINUX, "ctrl-shift-alt-down"),
];

#[gpui::test]
fn shift_tab_jumps_newer_in_normal_mode(cx: &mut TestAppContext) {
    for (os, source, _) in PLATFORMS {
        let start = rows(20);
        let (view, cx) = editor(cx, source, &start);
        cx.simulate_keystrokes("G");
        let newer = head(&view, cx);
        assert_ne!(newer, (0, 0), "{os}: G moved");
        cx.simulate_keystrokes("ctrl-o");
        assert_eq!(head(&view, cx), (0, 0), "{os}: ctrl-o goes back");
        cx.simulate_keystrokes("shift-tab");
        assert_eq!(head(&view, cx), newer, "{os}: shift-tab goes newer");
        assert_eq!(mode(&view, cx), Some("normal"), "{os}");
        assert_eq!(text(&view, cx), start, "{os}: nothing edited");
    }
}

/// Every selection as a byte range and whether it is reversed.
fn selections(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(Range<usize>, bool)> {
    view.read_with(cx, |view, _| {
        view.model()
            .selections()
            .into_iter()
            .map(|selection| (selection.range, selection.reversed))
            .collect()
    })
}

fn first_start(selections: &[(Range<usize>, bool)]) -> usize {
    selections
        .iter()
        .map(|(range, _)| range.start)
        .min()
        .unwrap()
}

#[gpui::test]
fn modified_arrows_move_visual_selections(cx: &mut TestAppContext) {
    for (os, source, duplicate) in PLATFORMS {
        for (enter, name) in [
            ("v", "visual"),
            ("shift-v", "visual_line"),
            ("ctrl-v", "visual_block"),
        ] {
            let start = rows(6);
            let (view, cx) = editor(cx, source, &start);
            cx.simulate_keystrokes("j");
            cx.simulate_keystrokes(enter);
            let label = format!("{os} {enter}");
            assert_eq!(mode(&view, cx), Some(name), "{label}");
            let before = selections(&view, cx);

            // What vim's own `j` extends the selection to.
            cx.simulate_keystrokes("j");
            let extended = selections(&view, cx);
            assert_ne!(extended, before, "{label}: j extends");
            cx.simulate_keystrokes("k");
            assert_eq!(selections(&view, cx), before, "{label}: k restores");

            for down in ["alt-down", duplicate] {
                cx.simulate_keystrokes(down);
                assert_eq!(selections(&view, cx), extended, "{label} {down}: extends");
                assert_eq!(
                    first_start(&extended),
                    first_start(&before),
                    "{label} {down}: the anchor stays"
                );
                assert_eq!(head(&view, cx).0, 2, "{label} {down}: a row down");
                assert_eq!(mode(&view, cx), Some(name), "{label} {down}: mode");
                assert_eq!(text(&view, cx), start, "{label} {down}: unchanged");
                cx.simulate_keystrokes("alt-up");
                assert_eq!(selections(&view, cx), before, "{label} {down}: alt-up");
                assert_eq!(mode(&view, cx), Some(name), "{label} {down}: mode back");
                assert_eq!(text(&view, cx), start, "{label} {down}: unchanged back");
            }
        }
    }
}
