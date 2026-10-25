//! The which-key popup ([`super::which_key`]): it shows the keys that go on
//! from a prefix once the delay passes with no key, never for a sequence
//! typed faster, and closes on the next key, `<Esc>`, a blur or a mode
//! change.

use std::time::Duration;

use fe_buffer::Buffer;
use fe_command::Invocation;
use fe_editor::EditorModel;
use fe_vim::{Remap, RemapMode, WhichKey, WhichKeyLabel};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, commands::RunCommand, init};

/// The panel's and its keys' debug selectors.
const POPUP: &str = "which-key-popup";
const ENTRIES: &str = "which-key-entries";

const DELAY: Duration = Duration::from_millis(200);

const TEXT: &str = "one\ntwo\nthree\nfour\nfive\nsix";

/// A focused editor in a window of its own, vim on, the popup after
/// [`DELAY`], and `remaps` set with `,` as the leader.
fn vim_editor(
    cx: &mut TestAppContext,
    remaps: Vec<Remap>,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
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
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
    (view, cx)
}

fn noremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Normal],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

fn popup(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<WhichKey> {
    view.read_with(cx, |view, _| view.which_key_shown().cloned())
}

fn wait(time: Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
}

/// Each key listed, with its title as the popup shows it.
fn titles(which: &WhichKey, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    cx.update(|_, cx| {
        (which.entries.iter())
            .map(|entry| (entry.key.clone(), which_key::title(entry, cx)))
            .collect()
    })
}

fn title_of(which: &WhichKey, key: &str, cx: &mut VisualTestContext) -> String {
    let titles = titles(which, cx);
    (titles.into_iter().find(|(listed, _)| listed == key))
        .unwrap_or_else(|| panic!("{key} not listed in {which:?}"))
        .1
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

/// After `z` nothing shows until the delay passes with no key; then the
/// popup lists `z`'s keys with their titles, and `t` closes it as `zt`
/// runs.
#[gpui::test]
fn popup_waits_for_the_delay(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, Vec::new());
    cx.simulate_keystrokes("z");
    assert_eq!(popup(&view, cx), None);
    wait(DELAY - Duration::from_millis(1), cx);
    assert_eq!(popup(&view, cx), None);

    wait(Duration::from_millis(2), cx);
    let which = popup(&view, cx).expect("the popup after the delay");
    assert_eq!(which.prefix, "z");
    assert_eq!(title_of(&which, "t", cx), "scroll top");
    assert_eq!(title_of(&which, "a", cx), "toggle fold");
    assert_eq!(title_of(&which, "<CR>", cx), "scroll top, first non-blank");
    // It paints, over the text.
    draw(cx);

    cx.simulate_keystrokes("t");
    assert_eq!(popup(&view, cx), None);
    assert_eq!(mode(&view, cx), Some("normal"));
    // `zt` ran: nothing waits.
    let pending = view.read_with(cx, |view, _| view.vim_state().unwrap().pending().kind());
    assert_eq!(pending, None);
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
}

/// A prefix completed before the delay never shows the popup, even once
/// the delay has passed; `<Esc>`, a blur and a mode change each close an
/// open one.
#[gpui::test]
fn fast_sequences_never_show_it(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, Vec::new());
    cx.simulate_keystrokes("z");
    wait(DELAY / 2, cx);
    cx.simulate_keystrokes("z");
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);

    // A second prefix's delay starts at its own key, not the first's.
    cx.simulate_keystrokes("z");
    wait(DELAY / 2, cx);
    cx.simulate_keystrokes("z z");
    wait(DELAY / 2 + Duration::from_millis(1), cx);
    assert_eq!(popup(&view, cx), None);
    wait(DELAY / 2, cx);
    assert_eq!(popup(&view, cx).expect("z's popup").prefix, "z");

    cx.simulate_keystrokes("escape");
    assert_eq!(popup(&view, cx), None);
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);

    // A blur closes it, and ends a wait.
    cx.simulate_keystrokes("shift-z");
    wait(DELAY * 2, cx);
    assert_eq!(popup(&view, cx).expect("Z's popup").prefix, "Z");
    assert!(cx.debug_bounds(POPUP).is_some(), "Z's popup paints");
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert_eq!(popup(&view, cx), None);
    // The blur came as a frame drew: the next one, without the panel,
    // follows on its own.
    assert_eq!(cx.debug_bounds(POPUP), None);
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.simulate_keystrokes("escape z");
    cx.update(|window, cx| window.blur(cx));
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);

    // A command that leaves normal mode, with no key, closes it.
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.simulate_keystrokes("escape z");
    wait(DELAY * 2, cx);
    assert!(popup(&view, cx).is_some());
    let insert = Invocation::of("vim.insert_last").unwrap();
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(insert)), cx));
    cx.run_until_parked();
    assert_eq!(mode(&view, cx), Some("insert"));
    assert_eq!(popup(&view, cx), None);

    // A mode change that keeps the prefix and its list closes it too, or
    // ends its wait, for good.
    cx.simulate_keystrokes("escape shift-z");
    assert_eq!(mode(&view, cx), Some("normal"));
    visual(cx);
    assert_eq!(mode(&view, cx), Some("visual"));
    let pending = view.read_with(cx, |view, _| view.vim_state().unwrap().which_key());
    assert_eq!(pending.expect("Z still waits").prefix, "Z");
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);

    cx.simulate_keystrokes("escape escape shift-z");
    wait(DELAY * 2, cx);
    assert!(popup(&view, cx).is_some());
    visual(cx);
    assert_eq!(mode(&view, cx), Some("visual"));
    assert_eq!(popup(&view, cx), None);
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
}

/// Dispatches `vim.visual` (charwise), as a binding or the palette would.
fn visual(cx: &mut VisualTestContext) {
    let visual = (Invocation::of("vim.visual").unwrap()).args(serde_json::json!({"kind": "char"}));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(visual)), cx));
    cx.run_until_parked();
}

/// A prefix a map's replay leaves with no key shows the popup after the
/// delay too: when a held map times out into it, and after a map's
/// `<Cmd>fe:…<CR>` command, whose keys after it run once the keystroke is
/// over.
#[gpui::test]
fn prefixes_left_by_a_replay_show_it(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap("x", "z"),
        noremap("xx", "gg"),
        noremap("<leader>u", "<Cmd>fe:editor.undo<CR>z"),
    ];
    let (view, cx) = vim_editor(cx, remaps);
    cx.simulate_keystrokes("x");
    wait(DELAY, cx);
    assert_eq!(popup(&view, cx).expect("x's popup").prefix, "x");
    // The wait for `xx` times out: `x` is `z`, which waits in turn.
    wait(CHORD_TIMEOUT - DELAY + Duration::from_millis(1), cx);
    assert_eq!(popup(&view, cx), None);
    wait(DELAY, cx);
    let which = popup(&view, cx).expect("z's popup after the timeout");
    assert_eq!(which.prefix, "z");
    assert_eq!(title_of(&which, "t", cx), "scroll top");

    cx.simulate_keystrokes("escape , u");
    cx.run_until_parked();
    assert_eq!(popup(&view, cx), None);
    wait(DELAY, cx);
    assert_eq!(
        popup(&view, cx).expect("z's popup after the map").prefix,
        "z"
    );
    cx.simulate_keystrokes("z");
    assert_eq!(popup(&view, cx), None);
}

/// The panel takes at most half a short editor's height, its keys
/// scrolling within it, with z's list and with a long list of maps.
#[gpui::test]
fn the_panel_fits_a_short_editor(cx: &mut TestAppContext) {
    let remaps = (b'a'..=b'z')
        .chain(b'A'..=b'Z')
        .map(|key| noremap(&format!("<leader>{}", key as char), "gg"))
        .collect();
    let (view, cx) = vim_editor(cx, remaps);
    cx.simulate_resize(gpui::size(gpui::px(320.), gpui::px(240.)));
    cx.run_until_parked();
    let editor = cx.update(|window, _| window.viewport_size());
    for keys in ["z", ","] {
        cx.simulate_keystrokes(keys);
        wait(DELAY, cx);
        assert!(popup(&view, cx).is_some(), "{keys}'s popup");
        let panel = cx.debug_bounds(POPUP).expect("the panel paints");
        let entries = cx.debug_bounds(ENTRIES).expect("its keys paint");
        assert!(
            panel.size.height <= editor.height * 0.5 + gpui::px(1.),
            "{panel:?}"
        );
        assert!(
            panel.top() >= editor.height * 0.5 - gpui::px(1.),
            "{panel:?}"
        );
        assert!(panel.bottom() <= editor.height + gpui::px(1.), "{panel:?}");
        assert!(
            entries.bottom() <= panel.bottom(),
            "{entries:?} in {panel:?}"
        );
        cx.simulate_keystrokes("escape");
    }
}

/// A map's command shows by its title from whichever registry has it: the
/// diff view's, or the host's handed over.
#[gpui::test]
fn command_titles_come_from_every_registry(cx: &mut TestAppContext) {
    cx.update(init);
    let next_change = crate::diff_view::registry()
        .get(&fe_command::CommandId::parse(crate::diff_view::NEXT_CHANGE).unwrap())
        .unwrap()
        .title
        .clone();
    assert_ne!(next_change, crate::diff_view::NEXT_CHANGE);
    let title = cx.update(|cx| which_key::command_title(crate::diff_view::NEXT_CHANGE, cx));
    assert_eq!(title, next_change);

    let id = "project_panel.focus";
    assert_eq!(cx.update(|cx| which_key::command_title(id, cx)), id);
    set_host_commands(vec![fe_command::CommandMeta {
        id: fe_command::CommandId::parse(id).unwrap(),
        title: "Focus Explorer".into(),
        description: String::new(),
        args_schema: None,
        context: None,
        mutates_buffer: false,
    }]);
    let title = cx.update(|cx| which_key::command_title(id, cx));
    set_host_commands(Vec::new());
    assert_eq!(title, "Focus Explorer");
}

/// With `<leader>ff` and `<leader>w` mapped, the popup after `<leader>`
/// lists `f` as a group and `w` with its command's title; after `f` it
/// repaints at once with `f`'s list. The held keys resolving at
/// [`CHORD_TIMEOUT`] close it.
#[gpui::test]
fn lists_user_remaps(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap("<leader>ff", "gg"),
        noremap("<leader>w", "<Cmd>fe:editor.undo<CR>"),
    ];
    let (view, cx) = vim_editor(cx, remaps);
    cx.simulate_keystrokes(",");
    assert_eq!(popup(&view, cx), None);
    wait(DELAY, cx);
    let which = popup(&view, cx).expect("the leader's popup");
    assert_eq!(which.prefix, ",");
    let f = (which.entries.iter())
        .find(|entry| entry.key == "f")
        .unwrap();
    assert_eq!((&f.label, f.group), (&WhichKeyLabel::Group(1), true));
    let undo = cx.update(|_, _| {
        editor_command(&fe_command::CommandId::parse("editor.undo").unwrap())
            .expect("the editor's undo")
            .title
    });
    assert_ne!(undo, "editor.undo");
    assert_eq!(
        titles(&which, cx),
        [("f".into(), "+1".into()), ("w".into(), undo)]
    );
    draw(cx);

    cx.simulate_keystrokes("f");
    let which = popup(&view, cx).expect("f's popup, at once");
    assert_eq!(which.prefix, ",f");
    assert_eq!(titles(&which, cx), [("f".into(), "gg".into())]);

    // The held keys resolve, unmapped, and the popup goes with them.
    wait(CHORD_TIMEOUT * 2, cx);
    assert_eq!(popup(&view, cx), None);

    cx.simulate_keystrokes("escape j j , f f");
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
    let row = view.read_with(cx, |view, _| {
        let model = view.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
            .row
    });
    assert_eq!(row, 0);
}

/// `vim.which_key = false` shows nothing; a delay of 0 shows the popup at
/// once; each change applies to the editors already open, closing their
/// popups when it's turned off.
#[gpui::test]
fn settings_switch_it(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, Vec::new());
    cx.update(|_, cx| set_vim_which_key(cx, false, DELAY));
    cx.simulate_keystrokes("z");
    wait(DELAY * 4, cx);
    assert_eq!(popup(&view, cx), None);
    cx.simulate_keystrokes("escape");

    cx.update(|_, cx| set_vim_which_key(cx, true, Duration::ZERO));
    cx.simulate_keystrokes("z");
    assert_eq!(popup(&view, cx).expect("the popup at once").prefix, "z");

    cx.update(|_, cx| set_vim_which_key(cx, false, Duration::ZERO));
    assert_eq!(popup(&view, cx), None);
    cx.simulate_keystrokes("escape");

    // A longer delay applies from the next prefix.
    cx.update(|_, cx| set_vim_which_key(cx, true, DELAY * 3));
    cx.simulate_keystrokes("shift-z");
    wait(DELAY * 2, cx);
    assert_eq!(popup(&view, cx), None);
    wait(DELAY, cx);
    assert_eq!(popup(&view, cx).expect("Z's popup").prefix, "Z");
}
