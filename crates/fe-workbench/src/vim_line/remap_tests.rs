//! `cnoremap` on the status bar's line: each character typed there is asked
//! of the `c` maps, a held prefix resolving after `CHORD_TIMEOUT`, on blur,
//! Enter and Esc; and a recording has the line as typed.

use fe_gpui::{
    EditorSettings, EditorTheme, EditorView, keymap::CHORD_TIMEOUT, set_vim_enabled, set_vim_remaps,
};
use fe_vim::{Remap, RemapMode};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

// Declared here, a test file, so the regression check keeps it.
mod line_edits_tests;
mod unmapped_input_tests;

pub(super) fn cnoremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::CmdLine],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench showing a file holding `aaa`, focused, with vim on and
/// `remaps` set.
pub(super) fn opened(
    cx: &mut TestAppContext,
    remaps: Vec<Remap>,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "aaa").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a tab is shown");
    (dir, workbench, editor, cx)
}

pub(super) fn line(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(char, String)> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, cx| bar.line(cx))
}

pub(super) fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().text().to_string())
}

pub(super) fn time_out(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    draw(cx);
}

pub(super) fn typed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    line(workbench, cx).expect("a line is open").1
}

#[gpui::test]
fn a_map_to_enter_submits_the_line(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j");
    assert_eq!(typed(&workbench, cx), "s/a/b/", "j is held");
    cx.simulate_keystrokes("k");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
}

#[gpui::test]
fn a_key_that_breaks_the_prefix_types_both(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j x");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "s/a/b/jx");
}

#[gpui::test]
fn a_map_that_types_text_inserts_it(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "s/a/b/")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j k");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "s/a/b/");
}

#[gpui::test]
fn a_held_prefix_types_after_the_timeout(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j");
    assert_eq!(typed(&workbench, cx), "");
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "j");
}

#[gpui::test]
fn enter_types_a_held_prefix_first(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("gx", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("g enter");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(
        text(&editor, cx),
        "bbb",
        "the g was a flag of the submitted line"
    );
}

#[gpui::test]
fn escape_types_a_held_prefix_into_the_history(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j escape");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    cx.simulate_keystrokes(": up");
    assert_eq!(typed(&workbench, cx), "j");
}

#[gpui::test]
fn blur_resolves_a_held_prefix_and_cancels(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("j");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    // Nothing is still held: the editor's `j` moves down, not on a map.
    cx.simulate_keystrokes("j");
    time_out(cx);
    assert_eq!(line(&workbench, cx), None);
}

#[gpui::test]
fn backspace_breaks_a_held_prefix_and_is_not_mapped(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    cx.simulate_keystrokes("j backspace");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "ab");
}

#[gpui::test]
fn a_line_without_maps_types_as_before(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("jk");
    assert_eq!(typed(&workbench, cx), "jk");
}

#[gpui::test]
fn a_recording_across_a_mapped_line_replays_identically(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j k q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
    // The jk was recorded once, as typed.
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(text(&editor, cx), "bba");
    assert_eq!(line(&workbench, cx), None);
}

#[gpui::test]
fn overlapping_prefixes_resolve_one_after_another(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) =
        opened(cx, vec![cnoremap("abc", "X"), cnoremap("bc", "Y")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("a b");
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "ab");
}

#[gpui::test]
fn enter_resolves_overlapping_prefixes_first(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("sg", "X"), cnoremap("gg", "Y")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("s g enter");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    let _ = editor;
}

#[gpui::test]
fn a_mapped_line_records_a_backspace(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/b/x");
    cx.simulate_keystrokes("backspace enter q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(text(&editor, cx), "bba");
}

/// An edit the maps never saw, a `<BS>` before the map's keys, is recorded
/// before them: a replay takes the `x` out before `jk` submits.
#[gpui::test]
fn a_bypassed_edit_is_recorded_before_the_mapping_keys(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/b/x");
    cx.simulate_keystrokes("backspace j k q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "baa");
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "bba");
}

#[gpui::test]
fn keys_after_a_map_s_enter_run_in_normal_mode(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>x")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/b/");
    cx.simulate_keystrokes("j k");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    // `x` deleted the character under the caret, in normal mode.
    assert_eq!(text(&editor, cx).len(), 2, "{}", text(&editor, cx));
}

#[gpui::test]
fn a_cmd_map_runs_its_command(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("zz", "<Cmd>s/a/b/<CR>")]);
    cx.simulate_keystrokes(":");
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(text(&editor, cx), "baa");
    // As in nvim, a `<Cmd>` leaves the command line open.
    assert_eq!(line(&workbench, cx), Some((':', String::new())));
}

fn cmap(from: &str, to: &str) -> Remap {
    Remap {
        recursive: true,
        ..cnoremap(from, to)
    }
}

fn nnoremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Normal],
        ..cnoremap(from, to)
    }
}

fn register(name: char, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        let registers = fe_gpui::vim_registers(cx)?;
        registers.get(name).map(|register| register.text.clone())
    })
}

/// Text no typed key made, as a recording has it: each character after the
/// literal marker a replay reads unmapped.
fn literal(text: &str) -> String {
    text.chars().flat_map(|ch| ['\u{F8FF}', ch]).collect()
}

/// A map's `<BS>` on an empty line cancels it, and every key after it is
/// the editor's: both `x`s delete.
#[gpui::test]
fn a_map_s_cancelling_backspace_keeps_every_key_after_it(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<BS>xx")]);
    cx.simulate_keystrokes(": j k");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "a");
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
}

/// The keys after a recursive map's cancelling `<BS>` are matched against
/// the maps of the mode it leaves, normal's.
#[gpui::test]
fn keys_after_a_map_s_cancel_match_the_mode_it_leaves(cx: &mut TestAppContext) {
    let maps = vec![cmap("jk", "<BS>Q"), nnoremap("Q", "x")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(": j k");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "aa");
}

/// The cancel is recorded once, in the map's keys: a replay cancels and
/// deletes as the line did.
#[gpui::test]
fn a_recorded_mapped_cancel_replays_identically(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<BS>x")]);
    cx.simulate_keystrokes("q a : j k q");
    draw(cx);
    assert_eq!(text(&editor, cx), "aa");
    assert_eq!(register('a', cx).as_deref(), Some(":jk"));
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "a");
}

/// A map's `<CR>` that submits `:w` hands the write to the workbench, and
/// the map's keys after it wait for it: the file has the text from before
/// the `x`.
#[gpui::test]
fn keys_after_a_mapped_submit_wait_for_its_write(cx: &mut TestAppContext) {
    let (dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>x")]);
    cx.simulate_keystrokes("x : w j k");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    let saved = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(saved, "aa");
    assert_eq!(text(&editor, cx), "a");
}

/// A `<Cmd>`'s keys type into the line as it shows, and it shows what they
/// typed; Enter then submits it.
#[gpui::test]
fn a_cmd_map_s_keys_type_into_the_visible_line(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("zz", "<Cmd>s/a/b/<CR>x/")]);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/");
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(text(&editor, cx), "baa");
    assert_eq!(line(&workbench, cx), Some((':', "s/a/x/".to_owned())));
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "bxa");
}

/// A `<Cmd>`'s keys that end the line submit the text it showed with
/// theirs, and a recording has the map's keys once, as typed.
#[gpui::test]
fn a_cmd_map_s_keys_submit_the_visible_line(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("zz", "<Cmd>s/a/b/<CR>x/<CR>")]);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/");
    cx.simulate_keystrokes("z z q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "bxa");
    assert_eq!(register('a', cx).as_deref(), Some(":s/a/zz"));
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "bxb", "the `:s/a/x/` finds no `a`");
}

/// A `<Cmd>`'s keys run on the line's text, taking it out and cancelling
/// it on an empty line, then go on in the mode that leaves: the `x` deletes.
#[gpui::test]
fn a_cmd_map_s_backspaces_run_on_the_visible_line(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("zz", "<Cmd>s/a/b/<CR><BS><BS><BS>x")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "aa");
}

/// The keys after a `<Cmd>fe:…<CR>` wait for the command, then type into
/// the line as it shows.
#[gpui::test]
fn a_cmd_map_s_keys_after_an_fe_command_type_into_the_line(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("zz", "<Cmd>fe:editor.save<CR>x/")];
    let (dir, workbench, _editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes("x :");
    cx.simulate_input("s/a/");
    cx.simulate_keystrokes("z z");
    draw(cx);
    let saved = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(saved, "aa");
    assert_eq!(line(&workbench, cx), Some((':', "s/a/x/".to_owned())));
}

/// A replayed `<CR>` is the line's, as the typed one is: the held `j`
/// resolves first, and the map `j<CR>` doesn't take it.
#[gpui::test]
fn enter_resolves_a_held_prefix_live_and_in_a_replay(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("j<CR>", "Z")]);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/");
    cx.simulate_keystrokes("j enter q");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "jaa");
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "jja");
}

/// A recursive map's special key goes through the maps, live and in a
/// replay.
#[gpui::test]
fn a_recursive_map_s_special_key_maps_live_and_in_a_replay(cx: &mut TestAppContext) {
    let maps = vec![cmap("zz", "x<C-x>"), cnoremap("<C-x>", "y")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/");
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "s/a/xy");
    cx.simulate_keystrokes("enter q");
    draw(cx);
    assert_eq!(text(&editor, cx), "xyaa");
    cx.simulate_keystrokes("@ a");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "xyxya");
}

/// Edits the maps never saw, a `<BS>`, a paste and a recall, are recorded
/// before the map's keys that end the line, which a replay then submits.
#[gpui::test]
fn bypassed_edits_are_recorded_before_a_map_that_ends_the_line(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx, vec![cnoremap("jk", "<CR>")]);
    let replays = |name: char, before: &str, after: &str, cx: &mut VisualTestContext| {
        assert_eq!(text(&editor, cx), after, "@{name}");
        cx.simulate_keystrokes("u");
        draw(cx);
        assert_eq!(text(&editor, cx), before, "@{name}");
        cx.simulate_keystrokes(&format!("@ {name}"));
        draw(cx);
        assert_eq!(line(&workbench, cx), None);
        assert_eq!(text(&editor, cx), after, "@{name}");
    };
    // A `<BS>`.
    cx.simulate_keystrokes("q a :");
    cx.simulate_input("s/a/b/x");
    cx.simulate_keystrokes("backspace j k q");
    draw(cx);
    assert_eq!(register('a', cx).as_deref(), Some(":s/a/b/x<BS>jk"));
    replays('a', "aaa", "baa", cx);
    // A `<BS>` that takes out a held `j`, which goes in first.
    cx.simulate_keystrokes("q b :");
    cx.simulate_input("s/a/c/");
    cx.simulate_keystrokes("j backspace j k q");
    draw(cx);
    assert_eq!(register('b', cx).as_deref(), Some(":s/a/c/j<BS>jk"));
    replays('b', "baa", "bca", cx);
    // A paste.
    cx.simulate_keystrokes("q c :");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("s/a/d/".into()));
    crate::tests::find_bar::run("text_input.paste", cx);
    cx.simulate_keystrokes("j k q");
    draw(cx);
    let pasted = format!(":{}jk", literal("s/a/d/"));
    assert_eq!(register('c', cx), Some(pasted.clone()));
    replays('c', "bca", "bcd", cx);
    // A recall of the last line, the paste's.
    cx.simulate_keystrokes("u q d : up j k q");
    draw(cx);
    assert_eq!(register('d', cx), Some(pasted));
    replays('d', "bca", "bcd", cx);
}

fn paste(string: &str, cx: &mut VisualTestContext) {
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(string.into()));
    crate::tests::find_bar::run("text_input.paste", cx);
    draw(cx);
}

/// A map's `<BS>` takes out what the line's does, a grapheme of more than
/// one character: the second `<BS>` cancels the line, and the `x` after it
/// is the editor's, not matched against the `c` maps.
#[gpui::test]
fn a_map_s_backspace_takes_a_grapheme_out(cx: &mut TestAppContext) {
    let maps = vec![cmap("zz", "<BS><BS>x"), cnoremap("x", "Y")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    paste("e\u{301}", cx);
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "aa");
}

/// A map's `<BS>` takes the selection out, as the line's does.
#[gpui::test]
fn a_map_s_backspace_takes_the_selection_out(cx: &mut TestAppContext) {
    let maps = vec![cmap("zz", "<BS><BS>x"), cnoremap("x", "Y")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("ab");
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let input = bar.read_with(cx, |bar, _| bar.line_input().cloned().unwrap());
    input.update(cx, |input, cx| input.select_all(cx));
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    assert_eq!(text(&editor, cx), "aa");
}

/// Keys typed on the line while a `<Cmd>`'s keys wait for the write they
/// ran wait too, and go in after them.
#[gpui::test]
fn keys_typed_while_a_cmd_waits_go_in_after_it(cx: &mut TestAppContext) {
    let (dir, workbench, _editor, cx) = opened(cx, vec![cnoremap("zz", "<Cmd>w<CR>x/")]);
    cx.simulate_keystrokes("x :");
    cx.simulate_input("s/a/");
    // One go: the write is still on its way when `a` and `b` are typed.
    cx.simulate_keystrokes("z z a b");
    draw(cx);
    let saved = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(saved, "aa");
    assert_eq!(line(&workbench, cx), Some((':', "s/a/x/ab".to_owned())));
}

/// A `<Cmd>`'s keys that cancel the line and open another: the bar shows
/// the new one, its prefix and its text.
#[gpui::test]
fn a_cmd_s_keys_that_open_another_line_show_it(cx: &mut TestAppContext) {
    let maps = vec![cnoremap("zz", "<Cmd>s/a/b/<CR><BS>/a")];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(": z z");
    draw(cx);
    assert_eq!(text(&editor, cx), "baa");
    assert_eq!(line(&workbench, cx), Some(('/', "a".to_owned())));
    let preview = editor.read_with(cx, |editor, _| editor.search_preview());
    assert_eq!(preview, Some(1..2), "the new line previews its search");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(line(&workbench, cx), None);
    let caret = editor.read_with(cx, |editor, _| editor.model().selections()[0].range.start);
    assert_eq!(caret, 1, "the search landed on the `a` after `b`");
}

/// Keys a map's `<BS>` leaves that are held for a longer map wait
/// `CHORD_TIMEOUT` as typed ones do, then go in.
#[gpui::test]
fn a_prefix_after_a_map_s_backspace_times_out(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(cx, vec![cmap("zz", "<BS>j"), cnoremap("jk", "Z")]);
    cx.simulate_keystrokes(": a z z");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "");
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "j");
}

/// A `<Cmd>`'s keys that close the line, open another and then wait for a
/// command: the bar shows the new line once they're over, with what they
/// typed into it, and keys typed meanwhile go in after them.
#[gpui::test]
fn a_cmd_s_reopened_line_waits_for_the_keys_after_it(cx: &mut TestAppContext) {
    let maps = vec![
        cnoremap("zz", "<Cmd>s/a/b/<CR><BS>/a<Cmd>fe:editor.save<CR>b"),
        cnoremap("yy", "<Cmd>s/a/b/<CR><BS>/a<Cmd>w<CR>b"),
    ];
    let (dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(": z z");
    draw(cx);
    assert_eq!(text(&editor, cx), "baa");
    assert_eq!(line(&workbench, cx), Some(('/', "ab".to_owned())));
    cx.simulate_keystrokes("escape");
    draw(cx);
    // One go: the write is still on its way when `c` is typed.
    cx.simulate_keystrokes(": y y c");
    draw(cx);
    assert_eq!(text(&editor, cx), "bba");
    let saved = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(saved, "bba");
    assert_eq!(line(&workbench, cx), Some(('/', "abc".to_owned())));
}

/// A `<Cmd>`'s keys that reopen a line, then fail at a command: the new
/// line shows what they typed into it before.
#[gpui::test]
fn a_cmd_s_reopened_line_keeps_its_text_when_a_command_fails(cx: &mut TestAppContext) {
    let maps = vec![
        cnoremap("zz", "<Cmd>s/a/b/<CR><BS>/a<Cmd>fe:no.such_command<CR>b"),
        cnoremap("yy", "<Cmd>s/a/b/<CR><BS>/a<Cmd>w<CR>b"),
    ];
    let (dir, workbench, _editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(": z z");
    draw(cx);
    assert_eq!(line(&workbench, cx), Some(('/', "a".to_owned())));
    cx.simulate_keystrokes("escape");
    draw(cx);
    // The file's folder is gone, so the write fails.
    std::fs::remove_dir_all(dir.path()).unwrap();
    cx.simulate_keystrokes(": y y");
    draw(cx);
    assert_eq!(line(&workbench, cx), Some(('/', "a".to_owned())));
}

/// The keys a `<Cmd>`'s tail leaves held for a longer map stay the bar's
/// line's: they resolve against its input after `CHORD_TIMEOUT`, and a key
/// typed meanwhile completes the longer map.
#[gpui::test]
fn a_cmd_tail_s_held_keys_resolve_in_the_line(cx: &mut TestAppContext) {
    let maps = vec![
        cmap("zz", "<Cmd>s/a/b/<CR>j"),
        cnoremap("j", "J"),
        cnoremap("jk", "Z"),
    ];
    let (_dir, workbench, editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/");
    cx.simulate_keystrokes("z z");
    draw(cx);
    assert_eq!(text(&editor, cx), "baa");
    assert_eq!(typed(&workbench, cx), "s/a/", "j is held");
    time_out(cx);
    assert_eq!(typed(&workbench, cx), "s/a/J");
    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.simulate_keystrokes(":");
    cx.simulate_input("s/a/");
    cx.simulate_keystrokes("z z k");
    draw(cx);
    assert_eq!(typed(&workbench, cx), "s/a/Z");
}

/// The same when the tail closes the line and opens another: the held keys
/// are the new line's.
#[gpui::test]
fn a_cmd_tail_s_held_keys_resolve_in_the_line_it_opened(cx: &mut TestAppContext) {
    let maps = vec![
        cmap("zz", "<Cmd>s/a/b/<CR><BS>/aj"),
        cnoremap("j", "J"),
        cnoremap("jk", "Z"),
    ];
    let (_dir, workbench, _editor, cx) = opened(cx, maps);
    cx.simulate_keystrokes(": z z");
    draw(cx);
    assert_eq!(line(&workbench, cx), Some(('/', "a".to_owned())));
    time_out(cx);
    assert_eq!(line(&workbench, cx), Some(('/', "aJ".to_owned())));
}
