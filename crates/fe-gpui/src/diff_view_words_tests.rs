#![allow(clippy::single_range_in_vec_init)]

//! A [`DiffView`]'s changed words: within a modified row pair the words
//! that differ highlighted over the row's tint on both sides, a row with no
//! partner highlighted whole, the filler rows across from it hatched, and
//! inline the removed rows' block likewise; all in the theme's named diff
//! colours, none of them blue.

use fe_buffer::Buffer;
use fe_config::theme::EditorPalette;
use gpui::{Bounds, Hsla, Pixels, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorSettings, EditorTheme, element::PaintedBlock, init};

fn text(rows: &[&str]) -> String {
    rows.join("\n")
}

/// A focused diff view of `left`, read-only, against `right`, side by side,
/// drawn.
fn diff_view<'a>(
    left: &str,
    right: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    cx.update(init);
    let left = EditorModel::new(Buffer::new(left), None);
    let right = EditorModel::new(Buffer::new(right), Some("a.rs".into()));
    let (diff, cx) = cx.add_window_view(move |_, cx| {
        let left = DiffSide::new("Index", left).read_only();
        DiffView::new("a.rs", left, DiffSide::new("Working Tree", right), cx)
    });
    cx.update(|window, cx| {
        window.focus(&diff.focus_handle(cx), cx);
        diff.update(cx, |diff, cx| diff.set_inline(false, window, cx));
    });
    draw(cx);
    (diff, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn side(diff: &Entity<DiffView>, ix: usize, cx: &mut VisualTestContext) -> Entity<EditorView> {
    diff.read_with(cx, |diff, _| diff.views[ix].clone())
}

fn theme(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> EditorTheme {
    view.read_with(cx, |view, _| view.theme.clone())
}

/// The model's highlights, as the text each covers and its tint.
fn words(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(String, Tint)> {
    view.read_with(cx, |view, _| {
        let text = view.model().text();
        (view.model().decorations(0..text.len()).into_iter())
            .filter(|item| item.kind == DecorationKind::Highlight)
            .map(|item| (text.slice_to_string(item.range), item.tint))
            .collect()
    })
}

/// The bounds and colour of each decoration of `kind` the last frame
/// painted.
fn painted(
    view: &Entity<EditorView>,
    kind: DecorationKind,
    cx: &mut VisualTestContext,
) -> Vec<(Bounds<Pixels>, Hsla)> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        (layout.decorations.placed.iter())
            .filter(|placed| placed.kind == kind)
            .map(|placed| (placed.bounds, placed.color))
            .collect()
    })
}

fn painted_blocks(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<PaintedBlock> {
    view.read_with(cx, |view, _| view.painted_blocks.clone())
}

/// Whether `inner` lies across `outer`'s row, inside its left and right.
fn within(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.top() == outer.top()
        && inner.bottom() == outer.bottom()
        && inner.left() > outer.left()
        && inner.right() < outer.right()
        && inner.size.width > px(0.)
}

#[gpui::test]
fn one_word(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(
        &text(&["a", "let x = foo(1);", "d"]),
        &text(&["a", "let x = bar(1);", "d"]),
        cx,
    );
    for (ix, word, tint) in [(0, "foo", Tint::DiffRemoved), (1, "bar", Tint::DiffAdded)] {
        let view = side(&diff, ix, cx);
        let theme = theme(&view, cx);
        let (row, word_color) = match ix {
            0 => (theme.diff_removed_row, theme.diff_removed_word),
            _ => (theme.diff_added_row, theme.diff_added_word),
        };
        // The changed word alone is highlighted, in the side's tint.
        assert_eq!(words(&view, cx), [(word.to_string(), tint)], "side {ix}");
        let start = view.read_with(cx, |view, _| view.model().text().row_start(1));
        let highlights: Vec<_> = (view.read_with(cx, |view, _| view.model().decorations(0..100)))
            .into_iter()
            .filter(|item| item.kind == DecorationKind::Highlight)
            .map(|item| item.range)
            .collect();
        assert_eq!(highlights, [start + 8..start + 11]);
        // Painted: the whole row in the row colour, the word in the word
        // colour inside it.
        let rows = painted(&view, DecorationKind::RowBackground, cx);
        let words = painted(&view, DecorationKind::Highlight, cx);
        assert_eq!(rows.len(), 1, "side {ix}");
        assert_eq!(words.len(), 1, "side {ix}");
        assert_eq!(rows[0].1, row);
        assert_eq!(words[0].1, word_color);
        assert!(within(words[0].0, rows[0].0), "{words:?} in {rows:?}");
        // "let x = " before it, 8 of the row's columns, and 3 for the word.
        let column = words[0].0.size.width / 3.;
        let left = words[0].0.left() - rows[0].0.left();
        assert!((left - column * 8.).abs() < px(0.5), "{left:?} {column:?}");
    }
}

#[gpui::test]
fn pure_rows(cx: &mut TestAppContext) {
    // Removed on the left, added on the right, each across from a filler.
    let (diff, cx) = diff_view(
        &text(&["a", "gone", "b", "c", "d"]),
        &text(&["a", "b", "c", "new", "d"]),
        cx,
    );
    let (left, right) = (side(&diff, 0, cx), side(&diff, 1, cx));
    assert_eq!(words(&left, cx), [("gone".to_string(), Tint::DiffRemoved)]);
    assert_eq!(words(&right, cx), [("new".to_string(), Tint::DiffAdded)]);
    for (view, ix) in [(&left, 0), (&right, 1)] {
        let theme = theme(view, cx);
        let (row, word) = match ix {
            0 => (theme.diff_removed_row, theme.diff_removed_word),
            _ => (theme.diff_added_row, theme.diff_added_word),
        };
        let rows = painted(view, DecorationKind::RowBackground, cx);
        let words = painted(view, DecorationKind::Highlight, cx);
        assert_eq!(
            rows.iter().map(|(_, color)| *color).collect::<Vec<_>>(),
            [row]
        );
        assert_eq!(
            words.iter().map(|(_, color)| *color).collect::<Vec<_>>(),
            [word]
        );
        // The text, from the row's start.
        assert_eq!(words[0].0.left(), rows[0].0.left());
        assert!(words[0].0.right() < rows[0].0.right());

        // The filler across from the other side's row, hatched in the
        // theme's filler colour.
        let blocks = painted_blocks(view, cx);
        assert_eq!(blocks.len(), 1, "side {ix}");
        assert_eq!(blocks[0].band, None);
        assert!(blocks[0].hatch > 0);
        assert_eq!(blocks[0].hatch_color, Some(theme.diff_filler));
    }

    // A modified hunk's row past the other side's: its partner's words,
    // then the rest whole.
    let (diff, cx) = diff_view(
        &text(&["a", "let x = foo(1);", "d"]),
        &text(&["a", "let x = bar(1);", "new", "d"]),
        cx,
    );
    let right = side(&diff, 1, cx);
    let added = |word: &str| (word.to_string(), Tint::DiffAdded);
    assert_eq!(words(&right, cx), [added("bar"), added("new")]);
    assert!(painted_blocks(&right, cx).is_empty());
    let left = side(&diff, 0, cx);
    assert_eq!(words(&left, cx), [("foo".to_string(), Tint::DiffRemoved)]);
    assert!(painted_blocks(&left, cx)[0].hatch > 0);

    // An added empty row has no text to highlight; its row is still tinted.
    let (diff, cx) = diff_view(&text(&["a", "d"]), &text(&["a", "", "d"]), cx);
    let right = side(&diff, 1, cx);
    assert_eq!(words(&right, cx), []);
    assert_eq!(painted(&right, DecorationKind::Highlight, cx), []);
    assert_eq!(painted(&right, DecorationKind::RowBackground, cx).len(), 1);
}

#[gpui::test]
fn inline_mode(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(
        &text(&["a", "let x = foo(1);", "\tgone", "d"]),
        &text(&["a", "let x = bar(1);", "d"]),
        cx,
    );
    cx.update(|window, cx| diff.update(cx, |diff, cx| diff.set_inline(true, window, cx)));
    draw(cx);
    let right = side(&diff, 1, cx);
    let theme = theme(&right, cx);

    // The removed rows' block: its paired row's changed word, and the row
    // with no partner whole.
    let blocks = right.read_with(cx, |view, _| view.blocks().to_vec());
    assert_eq!(blocks.len(), 1);
    let HostBlockKind::Lines {
        lines,
        tint,
        words: block_words,
    } = &blocks[0].kind
    else {
        panic!("{:?}", blocks[0].kind)
    };
    assert_eq!(lines, &["let x = foo(1);", "\tgone"]);
    assert_eq!(*tint, Tint::DiffRemoved);
    assert_eq!(block_words, &[vec![8..11], vec![0..5]]);
    let painted_blocks = painted_blocks(&right, cx);
    let [block] = &painted_blocks[..] else {
        panic!("{painted_blocks:?}")
    };
    assert_eq!(block.band, Some(theme.diff_removed_row));
    let [(foo, foo_color), (gone, gone_color)] = block.words[..] else {
        panic!("{:?}", block.words)
    };
    assert_eq!([foo_color, gone_color], [theme.diff_removed_word; 2]);
    // Each on its line, inside the block: foo after "let x = ", the
    // second line from its start, its tab expanded.
    let line_height = block.bounds.size.height / 2.;
    assert_eq!(foo.top(), block.bounds.top());
    assert_eq!(gone.top(), block.bounds.top() + line_height);
    let column = foo.size.width / 3.;
    let (foo_left, (_, first_line)) = (foo.left(), &block.lines[0]);
    assert!((foo_left - first_line.x - column * 8.).abs() < px(0.5));
    assert_eq!(gone.left(), block.lines[1].1.x);
    assert!(gone.size.width > column * 5.);

    // The right row below it as side by side: its word on its row's tint.
    assert_eq!(words(&right, cx), [("bar".to_string(), Tint::DiffAdded)]);
    let rows = painted(&right, DecorationKind::RowBackground, cx);
    let words = painted(&right, DecorationKind::Highlight, cx);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].1, words[0].1),
        (theme.diff_added_row, theme.diff_added_word)
    );
    assert!(within(words[0].0, rows[0].0));
}

/// Whether `color` reads as blue: a hue between cyan and violet, saturated
/// enough to show it.
fn is_blue(color: Hsla) -> bool {
    (0.45..0.75).contains(&color.h) && color.s > 0.15
}

#[gpui::test]
fn theme_colours(cx: &mut TestAppContext) {
    let palettes = [
        EditorPalette::fastedit_dark(),
        EditorPalette::fastedit_light(),
    ];
    for (palette, name) in palettes.into_iter().zip(["dark", "light"]) {
        let theme =
            cx.update(|cx| EditorTheme::from_palette(palette, &EditorSettings::default(), cx));
        let named = [
            ("diff_added_row", theme.diff_added_row),
            ("diff_removed_row", theme.diff_removed_row),
            ("diff_added_word", theme.diff_added_word),
            ("diff_removed_word", theme.diff_removed_word),
            ("diff_filler", theme.diff_filler),
        ];
        for (key, color) in named {
            let listed = palette.editor.get(key).expect("a colour key").to_rgba_u32();
            assert_eq!(Hsla::from(gpui::rgba(listed)), color, "{name} {key}");
            assert!(!is_blue(color), "{name} {key} is blue: {color:?}");
            assert!(color.a > 0., "{name} {key}");
            // A word and the filler go over a row band or the text's
            // background: see-through. Yohen's and Hakuji's row bands are
            // opaque, mixed with the background ahead.
            match key.ends_with("_row") {
                true => assert_eq!(color.a, 1., "{name} {key}"),
                false => assert!(color.a < 1., "{name} {key}"),
            }
        }
        // A word stands out from its row: its tint moves the band's colour,
        // in the same side's hue (green added, red removed).
        for (row, word) in [
            (theme.diff_added_row, theme.diff_added_word),
            (theme.diff_removed_row, theme.diff_removed_word),
        ] {
            let (row, tint) = (gpui::Rgba::from(row), gpui::Rgba::from(word));
            let mix = |u: f32, o: f32| o * tint.a + u * (1. - tint.a);
            let worded = [mix(row.r, tint.r), mix(row.g, tint.g), mix(row.b, tint.b)];
            let moved = [row.r, row.g, row.b]
                .iter()
                .zip(worded)
                .map(|(row, worded)| (row - worded).abs())
                .fold(0., f32::max);
            // At least 8 of 255 in some channel.
            assert!(moved >= 8. / 255., "{name}: a word shows on its row");
        }
        let green = |color: Hsla| (0.2..0.45).contains(&color.h);
        let red = |color: Hsla| color.h < 0.05 || color.h > 0.9;
        assert!(
            green(theme.diff_added_row) && green(theme.diff_added_word),
            "{name}"
        );
        assert!(
            red(theme.diff_removed_row) && red(theme.diff_removed_word),
            "{name}"
        );
    }
}

/// A DiffView's changed words stay readable. The word tint is painted with
/// the row bands in the background layer, before any glyph
/// (`Layer::Backgrounds`, ahead of the text), so what decides legibility is
/// its colour: composited over the row band and the editor background it
/// must leave the text its contrast.
mod contrast {
    use super::*;
    use gpui::{Hsla, Rgba};

    /// `over` laid on `under`, as the painter blends a translucent fill.
    fn over(under: Rgba, over: Hsla) -> Rgba {
        let over = Rgba::from(over);
        let mix = |u: f32, o: f32| o * over.a + u * (1. - over.a);
        Rgba {
            r: mix(under.r, over.r),
            g: mix(under.g, over.g),
            b: mix(under.b, over.b),
            a: 1.,
        }
    }

    /// WCAG relative luminance.
    fn luminance(color: Rgba) -> f32 {
        let lin = |c: f32| match c <= 0.03928 {
            true => c / 12.92,
            false => ((c + 0.055) / 1.055).powf(2.4),
        };
        0.2126 * lin(color.r) + 0.7152 * lin(color.g) + 0.0722 * lin(color.b)
    }

    fn contrast(a: Rgba, b: Rgba) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[gpui::test]
    fn a_changed_word_keeps_its_text_readable(cx: &mut TestAppContext) {
        let palettes = [
            (EditorPalette::fastedit_dark(), "dark"),
            (EditorPalette::fastedit_light(), "light"),
        ];
        for (palette, name) in palettes {
            let theme =
                cx.update(|cx| EditorTheme::from_palette(palette, &EditorSettings::default(), cx));
            let background = Rgba::from(theme.background);
            let comment = palette.syntax_color("comment").expect("a comment colour");
            let comment = Rgba::from(Hsla::from(gpui::rgba(comment.to_rgba_u32())));
            let foreground = Rgba::from(theme.foreground);
            for (side, row, word) in [
                ("added", theme.diff_added_row, theme.diff_added_word),
                ("removed", theme.diff_removed_row, theme.diff_removed_word),
            ] {
                // No stronger than VS Code's #..33, where it's see-through:
                // an opaque band (Yohen's, Hakuji's) is held to the contrast
                // checks alone.
                assert!(word.a <= 0.2 + 0.005, "{name} {side}");
                if row.a < 1. {
                    assert!(row.a <= 0.2 + 0.005, "{name} {side}");
                }
                let banded = over(background, row);
                let worded = over(banded, word);
                // The default text keeps WCAG AA on the word.
                let ratio = contrast(foreground, worded);
                assert!(ratio >= 4.5, "{name} {side} foreground {ratio}");
                // A comment has less than 4.5 even on the bare background in the
                // dark theme, so it must keep most of what it has there.
                let (plain, tinted) = (contrast(comment, background), contrast(comment, worded));
                assert!(
                    tinted >= 0.65 * plain,
                    "{name} {side} comment {tinted} of {plain}"
                );
            }
        }
    }
}
