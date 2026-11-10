//! Diff views in B3 (`docs/design/design-system.md`, Review diff): the
//! theme's diff tokens behind changed rows and, stronger, their changed
//! words; a hatch where a line does not exist; each hunk headed by the
//! symbol around it and its count of changes; `+` and `−` in each side's
//! gutter; and proposal blocks in the same tokens.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, Origin, ProposedChange};
use fe_editor::EditorModel;
use fe_gpui::{
    DiffSide, DiffView, EditorSettings, EditorTheme, EditorView, GlyphKind, GutterAnnotation,
    GutterAnnotations, GutterGlyph, HostBlock, HostBlockKind, diff_view::Glyph,
};
use gpui::{Bounds, Entity, Focusable as _, Hsla, Pixels, TestAppContext, VisualTestContext, hsla};

/// Diff tokens no theme ships, so a colour drawn in one came from the
/// theme.
fn theme(cx: &gpui::App) -> EditorTheme {
    let mut theme = EditorTheme::default_dark(cx);
    theme.diff_added_row = hsla(0.30, 0.5, 0.2, 1.);
    theme.diff_removed_row = hsla(0.01, 0.5, 0.2, 1.);
    theme.diff_added_word = hsla(0.31, 0.6, 0.4, 1.);
    theme.diff_removed_word = hsla(0.02, 0.6, 0.4, 1.);
    theme.diff_filler = hsla(0.1, 0.1, 0.9, 0.5);
    theme
}

/// The old text: a method of an impl.
const LEFT: &str = "struct MinimapTile;

impl MinimapTile {
    fn render(&self) -> u32 {
        let a = 1;
        let b = 2;
        a + b
    }
}

fn other() {}
";

/// [`LEFT`] with two rows of `render` changed, and a comment added after
/// `other`, in no symbol.
const RIGHT: &str = "struct MinimapTile;

impl MinimapTile {
    fn render(&self) -> u32 {
        let a = 10;
        let b = 20;
        a + b
    }
}

fn other() {}
// The end.
";

/// A diff view of [`LEFT`] against [`RIGHT`], as Rust, in [`theme`],
/// every row shown, parsed and drawn.
fn diff_view(cx: &mut TestAppContext) -> (Entity<DiffView>, &mut VisualTestContext) {
    diff_view_of(LEFT, RIGHT, cx)
}

/// A diff view of `left` against `right`, as [`diff_view`]'s.
fn diff_view_of<'a>(
    left: &str,
    right: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    let right = EditorModel::new(Buffer::new(right), Some("tile.rs".into()));
    diff_view_with(left, right, cx)
}

/// A diff view of `left` against `right`, a model the caller made (one it
/// may have split first), as [`diff_view`]'s.
fn diff_view_with<'a>(
    left: &str,
    right: EditorModel,
    cx: &'a mut TestAppContext,
) -> (Entity<DiffView>, &'a mut VisualTestContext) {
    let left = left.to_owned();
    cx.update(|cx| {
        fe_gpui::init(cx);
        let theme = theme(cx);
        fe_gpui::set_restore_style(cx, theme, EditorSettings::default());
    });
    let (diff, cx) = cx.add_window_view(|_, cx| {
        let left = EditorModel::new(Buffer::new(&left), None);
        let left = DiffSide::new("HEAD", left).read_only();
        DiffView::new("tile.rs", left, DiffSide::new("Working Tree", right), cx).with_folding(false)
    });
    cx.update(|window, cx| window.focus(&diff.focus_handle(cx), cx));
    for _ in 0..3 {
        draw(cx);
    }
    (diff, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn side(diff: &Entity<DiffView>, ix: usize, cx: &mut VisualTestContext) -> Entity<EditorView> {
    diff.read_with(cx, |diff, _| match ix {
        0 => diff.left().clone(),
        _ => diff.right().clone(),
    })
}

/// Where the last frame drew a caret at the start of buffer row `row` of
/// `view`, in window coordinates: the row's top and height.
fn row_bounds(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        let offset = view.model().text().row_start(row);
        view.caret_bounds_at(offset).expect("row in view")
    })
}

#[gpui::test]
fn a_missing_line_is_a_hatch(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(cx);
    let left = side(&diff, 0, cx);
    // The comment row has no partner on the left: a filler where it
    // would be, below `fn other() {}`, which paints the hatch.
    let fillers: Vec<HostBlock> = left.read_with(cx, |view, _| {
        (view.blocks().iter())
            .filter(|block| block.kind == HostBlockKind::Filler)
            .cloned()
            .collect()
    });
    assert_eq!(fillers.len(), 1, "{fillers:?}");
    assert_eq!((fillers[0].above, fillers[0].rows), (11, 1));
    // Drawn in the theme's hatch colour, seen through: not a flat band.
    let theme = left.read_with(cx, |view, _| view.theme().clone());
    assert_eq!(theme.diff_filler, self::theme_of(cx).diff_filler);
    assert!(theme.diff_filler.a > 0. && theme.diff_filler.a < 1.);
}

fn theme_of(cx: &mut VisualTestContext) -> EditorTheme {
    cx.update(|_, cx| theme(cx))
}

#[gpui::test]
fn changed_words_are_stronger_over_the_row_tint(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(cx);
    let theme = theme_of(cx);
    for (ix, row_tint, word_tint) in [
        (0, theme.diff_removed_row, theme.diff_removed_word),
        (1, theme.diff_added_row, theme.diff_added_word),
    ] {
        let view = side(&diff, ix, cx);
        let tints = diff.read_with(cx, |diff, cx| diff.drawn_tints(ix, cx));
        // `let a = 1;` / `let a = 10;`: the row, and over it the number.
        let row = row_bounds(&view, 4, cx);
        let band = (tints.iter())
            .find(|(bounds, _, word)| !word && bounds.top() == row.top())
            .unwrap_or_else(|| panic!("side {ix}: no band on row 4: {tints:?}"));
        assert_eq!(band.1, row_tint, "side {ix}");
        let words: Vec<_> = (tints.iter())
            .filter(|(bounds, _, word)| *word && bounds.top() == row.top())
            .collect();
        assert_eq!(words.len(), 1, "side {ix}: {tints:?}");
        assert_eq!(words[0].1, word_tint, "side {ix}");
        assert!(words[0].0.size.width < band.0.size.width, "side {ix}");
        // Painted after the band, so over it.
        let at = |item| tints.iter().position(|tint| tint == item).unwrap();
        assert!(at(words[0]) > at(band), "side {ix}");
    }
    // An unchanged row has neither.
    let right = side(&diff, 1, cx);
    let row = row_bounds(&right, 6, cx);
    let tints = diff.read_with(cx, |diff, cx| diff.drawn_tints(1, cx));
    assert!(!tints.iter().any(|(bounds, _, _)| bounds.top() == row.top()));
}

#[gpui::test]
fn a_hunk_is_headed_by_its_symbol_and_changes(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(cx);
    let headers: Vec<_> = diff.read_with(cx, |diff, cx| {
        (0..diff.diff().hunks().len())
            .map(|n| diff.hunk_header(n, cx).map(|text| text.to_string()))
            .collect()
    });
    assert_eq!(
        headers,
        [
            Some("impl MinimapTile › render · 4 changes".to_owned()),
            Some("1 change".to_owned()),
        ]
    );
    // Drawn on both sides, in a row of its own just above the hunk's first,
    // the sides still lined up.
    for ix in 0..2 {
        let view = side(&diff, ix, cx);
        let header = cx
            .debug_bounds(["diff-hunk:0:0", "diff-hunk:1:0"][ix])
            .unwrap_or_else(|| panic!("side {ix}: no header"));
        let first = row_bounds(&view, 4, cx);
        assert_eq!(header.bottom(), first.top(), "side {ix}");
        assert_eq!(header.size.height, first.size.height, "side {ix}");
    }
    let [left, right] = [0, 1].map(|ix| side(&diff, ix, cx));
    assert_eq!(
        row_bounds(&left, 10, cx).top(),
        row_bounds(&right, 10, cx).top()
    );
}

#[gpui::test]
fn each_changed_row_has_its_glyph(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(cx);
    for (ix, glyph, rows) in [
        (0, Glyph::Removed, vec![4, 5]),
        (1, Glyph::Added, vec![4, 5, 11]),
    ] {
        let view = side(&diff, ix, cx);
        let drawn = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(ix, cx));
        assert!(drawn.iter().all(|(drawn, _)| *drawn == glyph), "side {ix}");
        let tops: Vec<_> = rows
            .iter()
            .map(|&row| row_bounds(&view, row, cx).top())
            .collect();
        let drawn: Vec<_> = drawn.iter().map(|(_, bounds)| bounds.top()).collect();
        assert_eq!(drawn, tops, "side {ix}");
    }
    assert_eq!(Glyph::Added.text(), "+");
    assert_eq!(Glyph::Removed.text(), "−");
}

/// Rows `r0` to `r3`.
const TEXT: &str = "r0\nr1\nr2\nr3\n";

#[gpui::test]
fn proposals_take_the_themes_diff_tokens(cx: &mut TestAppContext) {
    cx.update(fe_gpui::init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
        EditorView::new(model, theme(cx), cx)
    });
    view.update(cx, |view, cx| {
        let snapshot = view.model().snapshot();
        let ai = Origin::Ai {
            agent: "agent".into(),
            session: "session".into(),
        };
        let proposed = "r0\nnew\nr2\nr3\n";
        let proposal = ProposedChange::from_text(snapshot, 0..TEXT.len(), proposed, ai);
        view.set_proposals(vec![Rc::new(RefCell::new(proposal))], cx);
    });
    draw(cx);
    let theme = theme_of(cx);
    let tints: Option<[Hsla; 4]> = view.read_with(cx, |view, _| view.drawn_proposal_tints());
    assert_eq!(
        tints,
        Some([
            theme.diff_added_row,
            theme.diff_removed_row,
            theme.diff_added_word,
            theme.diff_removed_word,
        ])
    );
}

#[gpui::test]
fn inline_the_removed_rows_have_theirs(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(cx);
    cx.update(|window, cx| diff.update(cx, |diff, cx| diff.set_inline(true, window, cx)));
    draw(cx);
    let right = side(&diff, 1, cx);
    let drawn = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(1, cx));
    let first = row_bounds(&right, 4, cx);
    let line = first.size.height;
    // The removed rows' block above `let a = 10;`, a `−` a row, after the
    // `+` of each row the right side has in a hunk.
    let mut want: Vec<_> = [4, 5, 11]
        .map(|row| (Glyph::Added, row_bounds(&right, row, cx).top()))
        .into();
    want.extend([2., 1.].map(|rows| (Glyph::Removed, first.top() - line * rows)));
    let drawn: Vec<_> = drawn
        .iter()
        .map(|(glyph, bounds)| (*glyph, bounds.top()))
        .collect();
    assert_eq!(drawn, want);
    // The header above the removed rows.
    let header = cx.debug_bounds("diff-hunk:1:0").expect("a header");
    assert_eq!(header.bottom(), first.top() - line * 2.);
}

#[gpui::test]
fn the_glyphs_have_a_column_of_their_own(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(cx);
    let right = side(&diff, 1, cx);
    // A lightbulb on a changed row, where the gutter's marks go.
    right.update(cx, |view, _| {
        let glyph = GutterGlyph {
            row: 4,
            kind: GlyphKind::Lightbulb,
        };
        view.set_gutter_glyph(Some(glyph), None);
    });
    draw(cx);
    let bulb = diff.read_with(cx, |diff, cx| diff.drawn_lightbulb(1, cx));
    let bulb = bulb.expect("a lightbulb");
    let glyphs = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(1, cx));
    let row = row_bounds(&right, 4, cx);
    let (_, glyph) = (glyphs.iter())
        .find(|(_, bounds)| bounds.top() == row.top())
        .expect("row 4's glyph");
    // In the annotation column, left of the gutter's slot: apart.
    let column = right.read_with(cx, |view, _| view.annotation_bounds(4));
    assert_eq!(column, Some(*glyph));
    assert!(glyph.right() <= bulb.left(), "{glyph:?} and {bulb:?}");
    // A host's column, such as blame, takes the place: no glyphs drawn
    // over it, and they're back once it goes.
    right.update(cx, |view, cx| {
        let snapshot = view.model().snapshot();
        let rows = snapshot.anchor_before(0)..snapshot.anchor_after(snapshot.len());
        let annotations = GutterAnnotations {
            version: snapshot.version(),
            entries: vec![GutterAnnotation {
                rows,
                text: "3 days ago someone".into(),
                heat: None,
            }],
            hover: Rc::new(|_, _| None),
            on_click: Rc::new(|_, _, _, _| {}),
        };
        view.set_gutter_annotations(Some(annotations), cx);
    });
    draw(cx);
    let text = |right: &Entity<EditorView>, cx: &mut VisualTestContext| {
        right.read_with(cx, |view, _| {
            let annotations = view.gutter_annotations()?;
            Some(annotations.entries[0].text.to_string())
        })
    };
    assert_eq!(text(&right, cx).as_deref(), Some("3 days ago someone"));
    let glyphs = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(1, cx));
    assert_eq!(glyphs, [], "none over the host's column");
    let left = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(0, cx));
    assert_eq!(left.len(), 2, "the other side keeps its own");
    right.update(cx, |view, cx| view.set_gutter_annotations(None, cx));
    draw(cx);
    draw(cx);
    let glyphs = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(1, cx));
    assert_eq!(glyphs.len(), 3, "{glyphs:?}");
}

#[gpui::test]
fn rows_added_or_removed_above_a_symbol_are_not_in_it(cx: &mut TestAppContext) {
    let left = "fn existing() {\n    1;\n}\n";
    let right = "// A note.\nfn existing() {\n    1;\n}\n";
    for (left, right) in [(left, right), (right, left)] {
        let (diff, cx) = diff_view_of(left, right, cx);
        let header = diff.read_with(cx, |diff, cx| diff.hunk_header(0, cx));
        assert_eq!(header.as_deref(), Some("1 change"), "{left:?} to {right:?}");
    }
}

#[gpui::test]
fn a_large_removed_block_draws_only_the_glyphs_in_view(cx: &mut TestAppContext) {
    let left: String = (0..20_000).map(|row| format!("gone {row}\n")).collect();
    let (diff, cx) = diff_view_of(&left, "kept\n", cx);
    cx.update(|window, cx| diff.update(cx, |diff, cx| diff.set_inline(true, window, cx)));
    draw(cx);
    let right = side(&diff, 1, cx);
    // The rows the view shows, and one cut by each edge.
    let shown = right.read_with(cx, |view, _| view.viewport_rows().expect("drawn").1) as usize + 2;
    for top in [0., 5_000.5, 19_990.] {
        right.update(cx, |view, cx| view.set_scroll_top(top, cx));
        draw(cx);
        let glyphs = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(1, cx));
        assert!(!glyphs.is_empty(), "at {top}");
        assert!(
            glyphs.len() <= shown,
            "at {top}: {} of {shown}",
            glyphs.len()
        );
    }
}

#[gpui::test]
fn many_nearby_hunks_reserve_nothing(cx: &mut TestAppContext) {
    // Every other row changed: a hunk each, with no fillers or folds.
    let rows = 4_000;
    let left: String = (0..rows)
        .map(|row| format!("same {row}\nold {row}\n"))
        .collect();
    let right: String = (0..rows)
        .map(|row| format!("same {row}\nnew {row}\n"))
        .collect();
    let (diff, cx) = diff_view_of(&left, &right, cx);
    assert_eq!(
        diff.read_with(cx, |diff, _| diff.diff().hunks().len()),
        rows
    );
    for ix in 0..2 {
        let view = side(&diff, ix, cx);
        // The headers aren't blocks the editor places every frame.
        let reserved = view.read_with(cx, |view, _| view.reserved_blocks().len());
        assert_eq!(reserved, 0, "side {ix}");
        // Those in view are drawn all the same.
        let header = cx.debug_bounds(["diff-hunk:0:0", "diff-hunk:1:0"][ix]);
        assert!(header.is_some(), "side {ix}");
    }
}

#[gpui::test]
fn the_glyph_column_keeps_up_with_edits(cx: &mut TestAppContext) {
    let (diff, cx) = diff_view(cx);
    let right = side(&diff, 1, cx);
    // Many edits apart, each drawn: the column is made again as of each
    // version, so laying it out maps no edits from an older one.
    for edit in 0..200 {
        right.update(cx, |view, cx| {
            let text = view.model().text();
            let at = text.row_start((edit % 10) as u32);
            view.model_mut().set_cursor(at);
            view.model_mut().insert("x");
            cx.notify();
        });
        draw(cx);
        let (version, shown) = right.read_with(cx, |view, _| {
            let version = view.model().buffer().version();
            (
                version,
                view.gutter_annotations()
                    .map(|annotations| annotations.version),
            )
        });
        assert_eq!(shown, Some(version), "after edit {edit}");
    }
    let glyphs = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(1, cx));
    assert!(!glyphs.is_empty());
}

#[gpui::test]
fn the_glyph_column_is_current_after_an_edit_through_another_model(cx: &mut TestAppContext) {
    // Over 64 KiB a side: diffed off the main thread.
    let left: String = (0..8000).map(|row| format!("row {row:05}\n")).collect();
    let right = left.replacen("row 04000\n", "row 04000 changed\n", 1);
    assert!(left.len() > 64 * 1024);
    let model = EditorModel::new(Buffer::new(&right), Some("a.txt".into()));
    let mut other = model.split();
    let (diff, cx) = diff_view_with(&left, model, cx);
    let right = side(&diff, 1, cx);
    let current = |right: &Entity<EditorView>, cx: &mut VisualTestContext| {
        right.read_with(cx, |view, _| {
            let shown = view
                .gutter_annotations()
                .map(|annotations| annotations.version);
            (shown, view.model().buffer().version())
        })
    };
    assert!(!diff.read_with(cx, |diff, _| diff.diff().hunks().is_empty()));
    let (shown, version) = current(&right, cx);
    assert_eq!(shown, Some(version), "the glyph column, once diffed");

    // Edits through the other model only, each drawn once, the diff of each
    // left pending: the side's own model lags the buffer meanwhile.
    for edit in 0..60 {
        let at = other.buffer().text().row_start(edit * 97 % 7000);
        other.buffer_mut().edit([(at..at, "x")], Origin::User);
        // Read right after the one draw, in its update: the test platform
        // draws again as an update ends, once the draw has asked for it.
        let (shown, version, pending) = cx.update(|window, cx| {
            diff.update(cx, |_, cx| cx.notify());
            window.draw(cx).clear(cx);
            let view = right.read(cx);
            let shown = view
                .gutter_annotations()
                .map(|annotations| annotations.version);
            let version = view.model().buffer().version();
            (shown, version, diff.read(cx).diff_pending())
        });
        assert!(pending, "edit {edit}");
        assert_eq!(shown, Some(version), "after edit {edit}");
    }

    // The diff lands; the glyphs show.
    draw(cx);
    let glyphs = diff.read_with(cx, |diff, cx| diff.drawn_glyphs(1, cx));
    assert!(!glyphs.is_empty());
}
