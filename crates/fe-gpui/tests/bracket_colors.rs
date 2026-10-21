//! Bracket pairs coloured by nesting depth, with `editor.bracket_pair_colors`.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{Entity, Hsla, TestAppContext, VisualTestContext};

/// An editor on `text` as a Rust file, parsed and drawn, bracket pairs
/// coloured if `colored`.
fn editor<'a>(
    text: &str,
    colored: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(fe_gpui::init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        let settings = EditorSettings {
            bracket_pair_colors: colored,
            ..EditorSettings::default()
        };
        EditorView::new(model, EditorTheme::from_settings(&settings, cx), cx)
    });
    cx.run_until_parked();
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.syntax().is_some()));
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The colour each of `offsets` was drawn in.
fn drawn(view: &Entity<EditorView>, offsets: &[usize], cx: &mut VisualTestContext) -> Vec<Hsla> {
    view.read_with(cx, |view, _| {
        offsets
            .iter()
            .map(|&offset| view.drawn_color(offset).expect("drawn"))
            .collect()
    })
}

fn brackets(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> [Hsla; 3] {
    view.read_with(cx, |view, _| view.theme().brackets)
}

/// The offsets of each char of `chars` in `text`, in order.
fn offsets(text: &str, chars: &str) -> Vec<usize> {
    text.char_indices()
        .filter(|&(_, c)| chars.contains(c))
        .map(|(at, _)| at)
        .collect()
}

#[gpui::test]
fn depths_take_the_three_colours_in_turn(cx: &mut TestAppContext) {
    let text = "fn f() {\n    g(h([1, (2, [3])]));\n}\n";
    let (view, cx) = editor(text, true, cx);
    let [one, two, three] = brackets(&view, cx);
    assert!(one != two && two != three && one != three);
    // `f()` and the body at depth 1, `g(` 2, `h(` 3, then `[` round to 1
    // again, `(2` 2 and `[3]` 3.
    let expected = [
        one, one, one, // `()` `{`
        two, three, one, two, three, three, two, one, three, two, // `g(h([1, (2, [3])]))`
        one, // `}`
    ];
    assert_eq!(drawn(&view, &offsets(text, "()[]{}"), cx), expected);
}

#[gpui::test]
fn on_until_the_setting_turns_it_off(cx: &mut TestAppContext) {
    assert!(EditorSettings::default().bracket_pair_colors);
    let text = "fn f() {\n    g(h([1]));\n}\n";
    let (view, cx) = editor(text, false, cx);
    let colors = brackets(&view, cx);
    let drawn = drawn(&view, &offsets(text, "()[]{}"), cx);
    assert!(
        drawn.iter().all(|color| !colors.contains(color)),
        "{drawn:?}"
    );

    // Turned on, the editor draws them coloured.
    view.update(cx, |view, cx| {
        let settings = EditorSettings {
            bracket_pair_colors: true,
            ..EditorSettings::default()
        };
        view.apply_settings(&settings, cx);
    });
    draw(cx);
    let [one, two, ..] = colors;
    assert_eq!(drawn_at(&view, text, "g(", cx), two);
    assert_eq!(drawn_at(&view, text, "{", cx), one);
}

fn drawn_at(
    view: &Entity<EditorView>,
    text: &str,
    needle: &str,
    cx: &mut VisualTestContext,
) -> Hsla {
    let at = text.find(needle).unwrap() + needle.len() - 1;
    drawn(view, &[at], cx)[0]
}

#[gpui::test]
fn strings_and_comments_keep_their_colour(cx: &mut TestAppContext) {
    let text = "fn f() {\n    let s = \"([{x}])\"; // ({[\n    g((s));\n}\n";
    let (view, cx) = editor(text, true, cx);
    let colors = brackets(&view, cx);
    let quote = text.find('"').unwrap();
    let comment = text.find("//").unwrap();
    let string = drawn(&view, &[quote], cx)[0];
    let commented = drawn(&view, &[comment], cx)[0];
    assert!(!colors.contains(&string) && !colors.contains(&commented));

    let in_string = offsets(&text[..comment], "([{}])")
        .into_iter()
        .filter(|&at| at > quote);
    for at in in_string {
        assert_eq!(drawn(&view, &[at], cx)[0], string, "string bracket at {at}");
    }
    let line_end = comment + text[comment..].find('\n').unwrap();
    for at in offsets(&text[comment..line_end], "({[") {
        let at = comment + at;
        assert_eq!(
            drawn(&view, &[at], cx)[0],
            commented,
            "comment bracket at {at}"
        );
    }
    // The code around them is still coloured: `g((s))` is two and three deep.
    let [_, two, three] = colors;
    assert_eq!(drawn_at(&view, text, "g(", cx), two);
    assert_eq!(drawn_at(&view, text, "g((", cx), three);
}
