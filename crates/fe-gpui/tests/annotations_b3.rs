//! Language-server annotations in B3 (`docs/design/design-system.md#editor`,
//! Language-server formatting; Find and replace): inlay hints as pills in UI
//! small on the raised surface in faint text, radius 4; the inline lens at
//! the end of a row, a warning's at 70% and an error's at full strength; the
//! identifier's uses in `identifier_highlight`; the current find match in
//! `find_match` and the others in `find_match_other`; lens rows in UI small
//! sans, faint, `·` separated; a mutable binding's underline thin, solid and
//! in its token's colour, a diagnostic's squiggle wavy in its severity's.
//! A pill takes the room its UI-small label needs; a match is current
//! wherever it is and whatever the find counted.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_command::Invocation;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps},
    diagnostics::{Diagnostic, Severity},
    occurrences::OccurrenceHighlights,
};
use fe_gpui::{
    DrawnAnnotations, DrawnText, EditorTheme, EditorView, InlayHint, Lens, LensItem, LensProvider,
};
use fe_vim::search::{FindQuery, Pattern};
use gpui::{
    Bounds, Entity, Focusable as _, FontWeight, Pixels, TestAppContext, VisualTestContext, px,
};

const TEXT: &str = "fn main() {\n    let budget = total;\n    let row = budget;\n    budget\n}\n";

/// Fixed decorations, as a semantic-token provider would hand out.
struct Fixed(Vec<Decoration>);

impl DecorationProvider for Fixed {
    fn decorations(&self, _: &BufferSnapshot, range: Range<usize>, out: &mut Vec<Decoration>) {
        out.extend(
            (self.0.iter())
                .filter(|item| overlaps(&item.range, &range))
                .cloned(),
        );
    }
}

/// Fixed lenses.
struct Lenses(Vec<Lens>);

impl LensProvider for Lenses {
    fn lenses(&mut self, _: &BufferSnapshot) -> Vec<Lens> {
        self.0.clone()
    }
}

/// A focused Rust editor on [`TEXT`] in the dark theme at 13 px, every
/// diagnostic's message shown after its row, set up by `setup` first.
fn editor(
    setup: impl FnOnce(&mut EditorModel) + 'static,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(fe_gpui::init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), Some("main.rs".into()));
        setup(&mut model);
        let mut theme = EditorTheme::default_dark(cx);
        theme.font_size = px(13.);
        theme.inline_diagnostics = fe_gpui::InlineDiagnostics::All;
        EditorView::new(model, theme, cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn annotations(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> DrawnAnnotations {
    draw(cx);
    view.read_with(cx, |view, _| {
        view.drawn_annotations().expect("drawn").clone()
    })
}

fn theme(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> EditorTheme {
    view.read_with(cx, |view, _| view.theme().clone())
}

/// The design's UI small: the UI's sans family at 500, 11 px at 13 px code.
fn assert_ui_small(text: &DrawnText, cx: &mut VisualTestContext) {
    let ui = cx.update(|_, cx| fe_gpui::ui_font_family(cx));
    assert_eq!(text.font.family, ui, "{text:?}");
    assert_eq!(text.font.weight, FontWeight::MEDIUM, "{text:?}");
    assert_eq!(text.font_size, px(11.), "{text:?}");
}

/// The window bounds of `range` on its row, from the carets at its ends.
fn range_bounds(
    view: &Entity<EditorView>,
    range: Range<usize>,
    cx: &mut VisualTestContext,
) -> (Pixels, Pixels) {
    view.update(cx, |view, _| {
        let start = view.caret_bounds_at(range.start).expect("in view");
        let end = view.caret_bounds_at(range.end).expect("in view");
        (start.left(), end.left())
    })
}

fn span(bounds: &Bounds<Pixels>) -> (Pixels, Pixels) {
    (bounds.left(), bounds.right())
}

fn nth(word: &str, n: usize) -> Range<usize> {
    let start = TEXT.match_indices(word).nth(n).expect("in TEXT").0;
    start..start + word.len()
}

#[gpui::test]
fn an_inlay_hint_is_a_pill_on_the_raised_surface_in_faint_ui_small(cx: &mut TestAppContext) {
    let (view, cx) = editor(|_| {}, cx);
    let at = nth("budget", 0).end;
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        let hint = InlayHint {
            offset: at,
            label: ": Budget".into(),
            kind: Some(fe_gpui::InlayHintKind::Type),
            padding_left: false,
            padding_right: false,
            data: None,
            tooltip: None,
            edits: Vec::new(),
        };
        view.set_inlay_hints(version, vec![hint]);
        cx.notify();
    });
    let drawn = annotations(&view, cx);
    let theme = theme(&view, cx);
    let [pill] = &drawn.inlay_hints[..] else {
        panic!("one pill: {:?}", drawn.inlay_hints);
    };
    assert_eq!(pill.text.text.as_ref(), ": Budget");
    // The design's raised surface, as the hover card's; radius 4.
    assert_eq!(pill.background, theme.popover_background);
    assert_eq!(pill.corner_radius, px(4.));
    // Faint text: the line numbers' colour, as the hover card's footer.
    assert_eq!(pill.text.color, theme.gutter_foreground);
    assert_ui_small(&pill.text, cx);
    // Where the hint sits, after `budget`.
    let (_, end) = range_bounds(&view, nth("budget", 0), cx);
    assert_eq!(pill.bounds.left(), end);
}

#[gpui::test]
fn a_warning_lens_is_warning_at_70_percent_and_an_error_lens_error_at_full(
    cx: &mut TestAppContext,
) {
    let (view, cx) = editor(|_| {}, cx);
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        let items = vec![
            (
                nth("budget", 0),
                Diagnostic::new(Severity::Warning, "unused variable"),
            ),
            (
                nth("total", 0),
                Diagnostic::new(Severity::Error, "cannot find value `total`"),
            ),
        ];
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, (1, 1), version, items, Some((1, 1)))
        );
        cx.notify();
    });
    let drawn = annotations(&view, cx);
    let theme = theme(&view, cx);
    // Row 1 has both: the error's message wins.
    let row = |row: u32| {
        (drawn.inline_diagnostics.iter())
            .find(|(at, ..)| *at == row)
            .map(|&(_, _, color)| color)
    };
    assert_eq!(row(1), Some(theme.error));
    assert_eq!(theme.error.a, 1.);

    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        let items = vec![(
            nth("budget", 0),
            Diagnostic::new(Severity::Warning, "unused variable"),
        )];
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, (1, 1), version, items, Some((1, 1)))
        );
        cx.notify();
    });
    let drawn = annotations(&view, cx);
    let lens = (drawn.inline_diagnostics.iter()).find(|(at, ..)| *at == 1);
    assert_eq!(
        lens.map(|&(_, _, color)| color),
        Some(theme.warning.opacity(0.7))
    );
    // The squiggle under it is wavy, in the warning colour.
    let (left, right) = range_bounds(&view, nth("budget", 0), cx);
    let squiggle = (drawn.underlines.iter())
        .find(|(bounds, ..)| span(bounds) == (left, right))
        .expect("a squiggle under `budget`");
    assert_eq!((squiggle.1, squiggle.2), (theme.warning, true));
}

#[gpui::test]
fn document_highlights_use_identifier_highlight(cx: &mut TestAppContext) {
    let (view, cx) = editor(
        |model| {
            // The occurrences of the word at the caret, as the workbench
            // adds them until a language server's highlights come.
            let mut occurrences = OccurrenceHighlights::new();
            let at = nth("budget", 0);
            model.select_range(at.start..at.start, false);
            occurrences.set_target(model.snapshot(), model.occurrence_target());
            model.add_decoration_provider(Rc::new(RefCell::new(occurrences)));
        },
        cx,
    );
    let drawn = annotations(&view, cx);
    let theme = theme(&view, cx);
    let mut expected = Vec::new();
    for n in 0..3 {
        expected.push(range_bounds(&view, nth("budget", n), cx));
    }
    let highlights: Vec<_> = (drawn.highlights.iter())
        .map(|(bounds, color)| (span(bounds), *color))
        .collect();
    assert_eq!(
        highlights,
        (expected.into_iter())
            .map(|span| (span, theme.identifier_highlight))
            .collect::<Vec<_>>()
    );
    // Its own colour, not a fade of the foreground as before.
    assert_ne!(theme.identifier_highlight, theme.foreground.opacity(0.22));
}

fn pattern(text: &str) -> Pattern {
    Pattern::from_query(&FindQuery {
        text: text.into(),
        match_case: true,
        ..FindQuery::default()
    })
    .unwrap()
}

#[gpui::test]
fn the_current_find_match_is_find_match_and_the_others_find_match_other(cx: &mut TestAppContext) {
    let (view, cx) = editor(|_| {}, cx);
    let theme = theme(&view, cx);
    // Yohen's, which tell them apart.
    assert_ne!(theme.find_match, theme.find_match_other);
    // The find bar's Find Next from the second `budget`: it selects it.
    view.update(cx, |view, cx| {
        view.set_find(Some(pattern("budget")), cx);
        assert!(view.find_from(nth("budget", 1).start, cx));
    });
    let drawn = annotations(&view, cx);
    let current = range_bounds(&view, nth("budget", 1), cx);
    let others = [0, 2].map(|n| range_bounds(&view, nth("budget", n), cx));
    let spans = |found: &[(Bounds<Pixels>, gpui::Hsla)]| -> Vec<_> {
        (found.iter())
            .map(|(bounds, color)| (span(bounds), *color))
            .collect()
    };
    assert_eq!(spans(&drawn.current_match), [(current, theme.find_match)]);
    assert_eq!(
        spans(&drawn.matches),
        others.map(|span| (span, theme.find_match_other))
    );

    // With none of them current, every match is a find match.
    view.update(cx, |view, cx| {
        view.model_mut().select_range(0..0, false);
        cx.notify();
    });
    let drawn = annotations(&view, cx);
    assert!(drawn.current_match.is_empty(), "{drawn:?}");
    let all = [0, 1, 2].map(|n| range_bounds(&view, nth("budget", n), cx));
    assert_eq!(
        spans(&drawn.matches),
        all.map(|span| (span, theme.find_match))
    );
}

#[gpui::test]
fn code_lenses_are_ui_small_sans_faint_and_dot_separated(cx: &mut TestAppContext) {
    let (view, cx) = editor(|_| {}, cx);
    let item = |title: &str| LensItem {
        title: title.to_owned().into(),
        invocation: Invocation::of("editor.select_all").expect("a command"),
    };
    let lenses = Lenses(vec![Lens {
        row: 0,
        items: vec![item("▷ Run test"), item("3 references")],
    }]);
    view.update(cx, |view, cx| {
        view.add_lens_provider(Rc::new(RefCell::new(lenses)));
        cx.notify();
    });
    let drawn = annotations(&view, cx);
    let theme = theme(&view, cx);
    let texts: Vec<&str> = drawn.lenses.iter().map(|text| text.text.as_ref()).collect();
    assert_eq!(texts, ["▷ Run test", " · ", "3 references"]);
    for text in &drawn.lenses {
        assert_eq!(text.color, theme.gutter_foreground, "{text:?}");
        assert_ui_small(text, cx);
    }
}

#[gpui::test]
fn a_mutable_underline_is_thin_solid_and_in_its_tokens_colour(cx: &mut TestAppContext) {
    let at = nth("budget", 0);
    let underline = Decoration::new(
        at.clone(),
        DecorationKind::SyntaxUnderline("keyword".into()),
        Tint::Muted,
    );
    let (view, cx) = editor(
        move |model| {
            model.add_decoration_provider(Rc::new(RefCell::new(Fixed(vec![underline]))));
        },
        cx,
    );
    let drawn = annotations(&view, cx);
    let theme = theme(&view, cx);
    let keyword = fe_syntax::Highlight::from_capture("keyword")
        .and_then(|highlight| theme.syntax_color(highlight))
        .expect("the theme colours keywords");
    let span_at = range_bounds(&view, at, cx);
    let [(bounds, color, wavy)] = &drawn.underlines[..] else {
        panic!("one underline: {:?}", drawn.underlines);
    };
    assert_eq!(span(bounds), span_at);
    assert_eq!((*color, *wavy), (keyword, false));
    assert_eq!(bounds.size.height, px(1.));
}

fn hint(offset: usize, label: &str, padding_left: bool) -> InlayHint {
    InlayHint {
        offset,
        label: label.into(),
        kind: Some(fe_gpui::InlayHintKind::Type),
        padding_left,
        padding_right: false,
        data: None,
        tooltip: None,
        edits: Vec::new(),
    }
}

#[gpui::test]
fn a_wide_label_fits_its_pill_and_moves_the_code_after_it(cx: &mut TestAppContext) {
    let (view, cx) = editor(|_| {}, cx);
    let at = nth("budget", 0).end;
    // Wide sans letters, two hints side by side at one offset, the second
    // padded on its left.
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        let wide = format!(": {}", "W".repeat(30));
        let hints = vec![hint(at, &wide, false), hint(at, "MMMM", true)];
        view.set_inlay_hints(version, hints);
        cx.notify();
    });
    let drawn = annotations(&view, cx);
    let [first, second] = &drawn.inlay_hints[..] else {
        panic!("two pills: {:?}", drawn.inlay_hints);
    };
    // An editor space, which the row reserves room in.
    let space = view.update(cx, |view, _| {
        let mut at = |offset| view.caret_bounds_at(offset).expect("in view").left();
        at(3) - at(2)
    });
    for pill in [first, second] {
        let text = &pill.text.bounds;
        assert!(
            pill.bounds.left() < text.left() && text.right() < pill.bounds.right(),
            "{pill:?}"
        );
        // As wide as the spaces that cover the label in UI small and 4 px
        // each side, whatever its length in the editor's font.
        let spaces = ((text.size.width + px(8.)) / space).ceil();
        let width = space * spaces;
        assert!(
            (pill.bounds.size.width - width).abs() < px(0.01),
            "{pill:?} {width:?}"
        );
    }
    // Side by side, the padding between them.
    assert!(second.bounds.left() > first.bounds.right(), "{drawn:?}");
    // The code after them starts past the second.
    let (code, _) = range_bounds(&view, at..at + 1, cx);
    let after = view.update(cx, |view, _| {
        view.caret_bounds_at(at + 1).expect("in view").left()
    });
    assert!(code >= first.bounds.left(), "{code:?}");
    assert!(after > second.bounds.right(), "{after:?} {second:?}");
    // A hint's hitbox covers its pill.
    let hitboxes = view.read_with(cx, |view, _| view.inlay_hint_bounds());
    assert_eq!(hitboxes.len(), 2);
    for ((_, hitbox), pill) in hitboxes.iter().zip([first, second]) {
        assert!(
            hitbox.left() <= pill.bounds.left() && pill.bounds.right() <= hitbox.right(),
            "{hitbox:?} {pill:?}"
        );
    }
}

fn regex(text: &str) -> Pattern {
    Pattern::from_query(&FindQuery {
        text: text.into(),
        match_case: true,
        regex: true,
        ..FindQuery::default()
    })
    .unwrap()
}

/// The colours of the matches drawn and of the current one.
fn match_colors(drawn: &DrawnAnnotations) -> (Vec<gpui::Hsla>, Vec<gpui::Hsla>) {
    let colors = |found: &[(Bounds<Pixels>, gpui::Hsla)]| found.iter().map(|&(_, c)| c).collect();
    (colors(&drawn.matches), colors(&drawn.current_match))
}

#[gpui::test]
fn a_selected_empty_match_is_current(cx: &mut TestAppContext) {
    let (view, cx) = editor(|_| {}, cx);
    let theme = theme(&view, cx);
    let row_start = |row: u32| {
        TEXT.split_inclusive('\n')
            .take(row as usize)
            .map(str::len)
            .sum()
    };
    // The empty row after the last line break too.
    let rows = TEXT.matches('\n').count() + 1;
    // `^` at each row's start, stepped to from row 1's.
    view.update(cx, |view, cx| {
        view.set_find(Some(regex("^")), cx);
        assert!(view.find_from(row_start(1), cx));
    });
    for row in [1, 2] {
        let drawn = annotations(&view, cx);
        let current = view.read_with(cx, |view, _| view.find_count().and_then(|c| c.current));
        assert!(current.is_some(), "the find bar counts it current");
        let (others, current) = match_colors(&drawn);
        assert_eq!(current, [theme.find_match], "row {row}");
        assert_eq!(others, vec![theme.find_match_other; rows - 1], "row {row}");
        let (left, _) = range_bounds(&view, row_start(row)..row_start(row), cx);
        assert_eq!(drawn.current_match[0].0.left(), left, "row {row}");
        view.update(cx, |view, cx| assert!(view.find_next(cx)));
    }
}

/// `budget` on each of 300 rows.
fn long_text() -> String {
    "let budget = 1;\n".repeat(300)
}

fn long_editor(
    text: String,
    vim: bool,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(fe_gpui::init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
        fe_gpui::set_vim_enabled(cx, vim);
    });
    draw(cx);
    (view, cx)
}

fn scroll_to(view: &Entity<EditorView>, top: f32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.set_scroll_top(top, cx));
}

#[gpui::test]
fn the_others_stay_find_match_other_with_the_current_find_match_out_of_view(
    cx: &mut TestAppContext,
) {
    let (view, cx) = long_editor(long_text(), false, cx);
    let theme = theme(&view, cx);
    view.update(cx, |view, cx| {
        view.set_find(Some(pattern("budget")), cx);
        assert!(view.find_from(0, cx));
    });
    let (others, current) = match_colors(&annotations(&view, cx));
    assert_eq!(current, [theme.find_match]);
    assert!(others.iter().all(|&c| c == theme.find_match_other));

    scroll_to(&view, 200., cx);
    let (others, current) = match_colors(&annotations(&view, cx));
    assert!(current.is_empty());
    assert!(!others.is_empty());
    assert!(
        others.iter().all(|&c| c == theme.find_match_other),
        "{others:?}"
    );
}

#[gpui::test]
fn a_find_match_past_the_count_cap_is_current(cx: &mut TestAppContext) {
    // More matches than the find counts.
    let text = "a\n".repeat(10_100);
    let (view, cx) = long_editor(text, false, cx);
    let theme = theme(&view, cx);
    view.update(cx, |view, cx| {
        view.set_find(Some(pattern("a")), cx);
        assert!(view.find_from(2 * 10_050, cx));
    });
    let drawn = annotations(&view, cx);
    let count = view
        .read_with(cx, |view, _| view.find_count())
        .expect("counted");
    assert!(count.capped && count.current.is_none(), "{count:?}");
    let (others, current) = match_colors(&drawn);
    assert_eq!(current, [theme.find_match]);
    assert!(!others.is_empty());
    assert!(
        others.iter().all(|&c| c == theme.find_match_other),
        "{others:?}"
    );

    // Out of view, it's still current.
    scroll_to(&view, 100., cx);
    let (others, current) = match_colors(&annotations(&view, cx));
    assert!(current.is_empty());
    assert!(!others.is_empty());
    assert!(
        others.iter().all(|&c| c == theme.find_match_other),
        "{others:?}"
    );
}

/// Opens vim's search line and submits `text` on it.
fn vim_search(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("/");
    let invocation = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": text }));
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(fe_gpui::commands::RunCommand(invocation)), cx)
    });
    cx.run_until_parked();
}

#[gpui::test]
fn a_vim_search_keeps_its_current_match_out_of_view_and_in_a_preview(cx: &mut TestAppContext) {
    let (view, cx) = long_editor(long_text(), true, cx);
    let theme = theme(&view, cx);
    vim_search("budget", cx);
    // The caret on the next match: current, as nvim's CurSearch.
    let (others, current) = match_colors(&annotations(&view, cx));
    assert_eq!(current, [theme.find_match]);
    assert!(others.iter().all(|&c| c == theme.find_match_other));

    scroll_to(&view, 200., cx);
    let (others, current) = match_colors(&annotations(&view, cx));
    assert!(current.is_empty());
    assert!(!others.is_empty());
    assert!(
        others.iter().all(|&c| c == theme.find_match_other),
        "{others:?}"
    );

    // A search line's preview is current wherever it is.
    view.update(cx, |view, cx| view.preview_search("budget", cx));
    draw(cx);
    scroll_to(&view, 100., cx);
    let (others, _) = match_colors(&annotations(&view, cx));
    assert!(!others.is_empty());
    assert!(
        others.iter().all(|&c| c == theme.find_match_other),
        "{others:?}"
    );
}
