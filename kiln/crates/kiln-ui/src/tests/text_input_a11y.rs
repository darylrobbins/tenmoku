// SPDX-License-Identifier: Apache-2.0

//! TextInput's accessible node: its line loaded in `accesskit_consumer`,
//! which is what the platform adapters ask, for the value, the selection after
//! typing and after a screen reader's `SetTextSelection`, and UTF-16
//! lengths. [`load`] and [`select`] serve TextField's tests too.

use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler as _, Focusable as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window,
    accesskit::{self, NodeId, Role, TreeId, TreeUpdate},
    div, px,
};

use crate::{TextInput, UiTheme, text_input::A11yLine};

const ROOT: NodeId = NodeId(0);
const INPUT: NodeId = NodeId(1);
/// The run's id in the trees [`load`] makes.
pub(super) const RUN: NodeId = NodeId(2);

/// `line` as the window's focused node, loaded as an adapter loads it.
pub(super) fn load(line: &A11yLine) -> accesskit_consumer::Tree {
    let mut input = accesskit::Node::new(line.role);
    input.set_label("Find");
    let run = line.write(&mut input, RUN);
    input.set_children(vec![RUN]);
    let mut root = accesskit::Node::new(Role::Window);
    root.set_children(vec![INPUT]);
    let update = TreeUpdate {
        nodes: vec![(ROOT, root), (INPUT, input), (RUN, run)],
        tree: Some(accesskit::Tree::new(ROOT)),
        tree_id: TreeId::ROOT,
        focus: INPUT,
    };
    accesskit_consumer::Tree::new(update, true)
}

/// What an adapter reads of `line`: the value, the selection's UTF-16
/// range (anchor, focus), its text, and the whole length in UTF-16.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Read {
    pub(super) role: Role,
    pub(super) value: Option<String>,
    pub(super) selection: (usize, usize),
    pub(super) selected: String,
    pub(super) utf16_len: usize,
    pub(super) read_only: bool,
}

pub(super) fn read(line: &A11yLine) -> Read {
    let tree = load(line);
    let state = tree.state();
    let node = state.focus().expect("the input is focused");
    assert!(node.supports_text_ranges(), "{line:?}");
    let selection = node.text_selection().expect("a selection");
    let anchor = node.text_selection_anchor().expect("an anchor");
    let focus = node.text_selection_focus().expect("a focus");
    Read {
        role: node.role(),
        value: node.value(),
        selection: (
            anchor.to_global_utf16_index(),
            focus.to_global_utf16_index(),
        ),
        selected: selection.text(),
        utf16_len: node.document_range().end().to_global_utf16_index(),
        read_only: node.is_read_only(),
    }
}

/// The `SetTextSelection` an adapter sends to select UTF-16 `anchor` to
/// `focus` of `line`, as accesskit_macos's `setAccessibilitySelectedTextRange:`
/// builds it.
pub(super) fn select(line: &A11yLine, anchor: usize, focus: usize) -> accesskit::TextSelection {
    let tree = load(line);
    let state = tree.state();
    let node = state.focus().expect("the input is focused");
    let at = |utf16| {
        node.text_position_from_global_utf16_index(utf16)
            .expect("in the text")
    };
    let mut range = at(anchor.min(focus)).to_degenerate_range();
    range.set_end(at(anchor.max(focus)));
    let selection = range.to_text_selection();
    // A range is start first; a backwards selection names the end first.
    match anchor <= focus {
        true => selection,
        false => accesskit::TextSelection {
            anchor: selection.focus,
            focus: selection.anchor,
        },
    }
}

struct Host(Entity<TextInput>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(300.)).child(self.0.clone())
    }
}

/// A focused input in a window, made by `make`.
fn open(
    cx: &mut TestAppContext,
    make: impl FnOnce(TextInput) -> TextInput + 'static,
) -> (Entity<TextInput>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|_, cx| {
        Host(cx.new(|cx| make(TextInput::new("Find", &UiTheme::default(), cx))))
    });
    let input = host.read_with(cx, |host, _| host.0.clone());
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&input.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (input, cx)
}

fn line(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> A11yLine {
    input.read_with(cx, |input, _| input.a11y_line())
}

/// Gives `input` the `SetTextSelection` for UTF-16 `anchor..focus`.
fn set_selection(
    input: &Entity<TextInput>,
    anchor: usize,
    focus: usize,
    cx: &mut VisualTestContext,
) {
    let selection = select(&line(input, cx), anchor, focus);
    input.update(cx, |input, cx| input.select_from_a11y(&selection, RUN, cx));
}

#[gpui::test]
fn typing_shows_in_the_value_with_the_caret_after_it(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |input| input);
    cx.simulate_input("héllo");
    let typed = read(&line(&input, cx));
    assert_eq!(typed.role, Role::TextInput);
    assert_eq!(typed.value.as_deref(), Some("héllo"));
    assert_eq!(typed.selection, (5, 5));
    assert_eq!(typed.utf16_len, 5);

    input.update(cx, |input, cx| {
        input.move_left(cx);
        input.move_left(cx);
    });
    cx.simulate_input("X");
    let inserted = read(&line(&input, cx));
    assert_eq!(inserted.value.as_deref(), Some("hélXlo"));
    assert_eq!(inserted.selection, (4, 4));
}

#[gpui::test]
fn an_empty_input_still_has_a_run(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |input| input);
    let empty = read(&line(&input, cx));
    assert_eq!(empty.value.as_deref(), Some(""));
    assert_eq!((empty.selection, empty.utf16_len), ((0, 0), 0));
}

#[gpui::test]
fn select_all_selects_the_whole_run(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |input| input);
    cx.simulate_input("query");
    input.update(cx, |input, cx| input.select_all(cx));
    let all = read(&line(&input, cx));
    assert_eq!(all.selection, (0, 5));
    assert_eq!(all.selected, "query");
}

#[gpui::test]
fn a_search_input_says_so(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, TextInput::search);
    assert_eq!(read(&line(&input, cx)).role, Role::SearchInput);
}

#[gpui::test]
fn set_text_selection_moves_the_caret_or_selects_all(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |input| input);
    cx.simulate_input("héllo");
    let text =
        |cx: &mut VisualTestContext| input.read_with(cx, |input, _| input.text().to_string());

    set_selection(&input, 2, 2, cx);
    assert_eq!(input.read_with(cx, |input, _| input.caret()), 3, "after é");
    assert_eq!(read(&line(&input, cx)).selection, (2, 2));
    // The caret it set is where typing goes.
    cx.simulate_input("Y");
    assert_eq!(text(cx), "héYllo");
    assert_eq!(read(&line(&input, cx)).selection, (3, 3));

    set_selection(&input, 0, 6, cx);
    assert!(input.read_with(cx, |input, _| input.selected()));
    assert_eq!(read(&line(&input, cx)).selected, "héYllo");
    // Backwards over all of it selects it all too, and stays backwards.
    set_selection(&input, 6, 0, cx);
    assert!(input.read_with(cx, |input, _| input.selected()));
    assert_eq!(read(&line(&input, cx)).selection, (6, 0));
    assert_eq!(input.read_with(cx, |input, _| input.caret()), 0);

    // Part of it, then typing replaces that part.
    set_selection(&input, 1, 4, cx);
    input.read_with(cx, |input, _| {
        assert!(!input.selected());
        assert_eq!(input.selection(), 1..5, "éYl");
        assert_eq!(input.caret(), 5);
    });
    let part = read(&line(&input, cx));
    assert_eq!((part.selection, part.selected.as_str()), ((1, 4), "éYl"));
    cx.simulate_input("Z");
    assert_eq!(text(cx), "hZlo");
    assert_eq!(read(&line(&input, cx)).selection, (2, 2));

    // A backwards part keeps its direction; Left goes to its start, and
    // Backspace deletes it.
    set_selection(&input, 3, 1, cx);
    assert_eq!(read(&line(&input, cx)).selection, (3, 1));
    input.update(cx, |input, cx| input.move_left(cx));
    assert_eq!(read(&line(&input, cx)).selection, (1, 1));
    set_selection(&input, 4, 2, cx);
    input.update(cx, |input, cx| input.backspace(cx));
    assert_eq!(text(cx), "hZ");
    assert_eq!(read(&line(&input, cx)).selection, (2, 2));

    // The platform sees the same range, reversed.
    set_selection(&input, 2, 1, cx);
    input.update_in(cx, |input, window, cx| {
        let platform = input.selected_text_range(false, window, cx).unwrap();
        assert_eq!((platform.range, platform.reversed), (1..2, true));
    });
}

#[gpui::test]
fn a_selection_in_another_node_is_ignored(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |input| input);
    cx.simulate_input("abc");
    let mut selection = select(&line(&input, cx), 1, 1);
    selection.anchor.node = NodeId(99);
    input.update(cx, |input, cx| input.select_from_a11y(&selection, RUN, cx));
    assert_eq!(input.read_with(cx, |input, _| input.caret()), 3);
}

#[gpui::test]
fn non_bmp_text_counts_in_utf16(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |input| input);
    // An astral emoji is two UTF-16 units, a ZWJ family one grapheme of 8.
    cx.simulate_input("a😀b👨‍👩‍👧");
    let read_now = read(&line(&input, cx));
    assert_eq!(read_now.utf16_len, 1 + 2 + 1 + 8);
    assert_eq!(read_now.selection, (12, 12));

    input.update(cx, |input, cx| input.move_left(cx));
    assert_eq!(
        read(&line(&input, cx)).selection,
        (4, 4),
        "before the family"
    );
    input.update(cx, |input, cx| input.move_left(cx));
    assert_eq!(read(&line(&input, cx)).selection, (3, 3), "after the emoji");

    // A screen reader asking for UTF-16 offset 1 is after the "a".
    set_selection(&input, 1, 1, cx);
    assert_eq!(input.read_with(cx, |input, _| input.caret()), 1);
    set_selection(&input, 3, 3, cx);
    assert_eq!(input.read_with(cx, |input, _| input.caret()), 5);
}

#[gpui::test]
fn a_grapheme_past_a_character_length_is_split(cx: &mut TestAppContext) {
    // 1 + 200 two-byte combining marks: 401 bytes, more than a u8 holds.
    let long = format!("e{}", "\u{301}".repeat(200));
    let (input, cx) = open(cx, |input| input);
    let text = format!("{long}x");
    input.update(cx, |input, cx| input.set_text(text.clone(), cx));
    let line_now = line(&input, cx);
    let lengths = line_now.character_lengths();
    assert_eq!(
        lengths.iter().map(|len| usize::from(*len)).sum::<usize>(),
        text.len()
    );
    assert_eq!(lengths.len(), 3, "{lengths:?}");
    let read_now = read(&line_now);
    assert_eq!(read_now.utf16_len, text.encode_utf16().count());
    assert_eq!(read_now.value.as_deref(), Some(text.as_str()));
    // A caret asked for inside the grapheme lands at its start.
    set_selection(&input, 150, 150, cx);
    assert_eq!(input.read_with(cx, |input, _| input.caret()), 0);
}

#[gpui::test]
fn an_open_composition_is_in_the_value(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |input| input);
    cx.simulate_input("ab");
    input.update_in(cx, |input, window, cx| {
        input.move_left(cx);
        input.replace_and_mark_text_in_range(None, "日本", None, window, cx);
    });
    let read_now = read(&line(&input, cx));
    assert_eq!(read_now.value.as_deref(), Some("a日本b"));
    assert_eq!(read_now.selection, (3, 3), "after the composition");

    // A screen reader moving the caret drops the composition, as a move
    // does; an offset after it counts in the committed text.
    set_selection(&input, 4, 4, cx);
    input.read_with(cx, |input, _| {
        assert_eq!(input.text(), "ab");
        assert_eq!(input.caret(), 2);
    });
    assert_eq!(read(&line(&input, cx)).value.as_deref(), Some("ab"));
}

#[gpui::test]
fn it_is_read_only_while_typing_is_off(cx: &mut TestAppContext) {
    let (input, cx) = open(cx, |input| input);
    assert!(!read(&line(&input, cx)).read_only);
    input.update(cx, |input, cx| input.set_typing(false, cx));
    assert!(
        read(&line(&input, cx)).read_only,
        "a picker's vim normal mode"
    );
    input.update(cx, |input, cx| input.set_typing(true, cx));
    assert!(!read(&line(&input, cx)).read_only);
}

#[gpui::test]
fn the_recorder_sees_its_role_and_name(cx: &mut TestAppContext) {
    let (_input, cx) = open(cx, |input| input.search().name("Search files"));
    cx.update(|window, cx| {
        let nodes = crate::nodes(window, cx);
        let node = (nodes.iter())
            .find(|node| node.id == "text-input".into())
            .expect("recorded");
        assert_eq!(node.role, Role::SearchInput);
        assert_eq!(node.label, "Search files");
    });
}
