//! Diagnostic tags in a headless window: Unnecessary code faded, with no
//! squiggle for a hint, and Deprecated code struck through.

use std::ops::Range;

use fe_buffer::Buffer;
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, DiagnosticTag, Severity},
};
use gpui::{Entity, Hsla, TestAppContext, VisualTestContext};

use crate::*;

const TEXT: &str = "fn main() {\n    let unused = old();\n}";

/// The identity the host applies them from: server 1's (1, 1).
const ID: (u64, u64) = (1, 1);

fn word(word: &str) -> Range<usize> {
    let start = TEXT.find(word).unwrap();
    start..start + word.len()
}

fn tagged(severity: Severity, tag: Option<DiagnosticTag>) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(severity, "tagged");
    diagnostic.tags.extend(tag);
    diagnostic
}

fn editor<'a>(
    language: Option<&str>,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, _| window.activate_window());
    if let Some(language) = language {
        view.update(cx, |view, cx| view.set_language(Some(language), cx));
    }
    cx.run_until_parked();
    (view, cx)
}

fn diagnose(
    view: &Entity<EditorView>,
    items: Vec<(Range<usize>, Diagnostic)>,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, ID, version, items, Some(ID))
        );
        cx.notify();
    });
    cx.run_until_parked();
}

/// The colour of each byte of the second row's text, as laid out.
fn colours(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Hsla> {
    view.read_with(cx, |view, _| {
        let row = &view.layout.as_ref().unwrap().frame.rows[1];
        (0..row.end - row.start)
            .map(|ix| row.line.color_at(ix).unwrap())
            .collect()
    })
}

fn x(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> gpui::Pixels {
    view.update(cx, |view, _| {
        let caret = view.layout.as_mut().unwrap().caret_bounds(offset).unwrap();
        caret.left()
    })
}

fn check(language: Option<&str>, cx: &mut TestAppContext) {
    let (view, cx) = editor(language, cx);
    let row_start = TEXT.find("    let").unwrap();
    let unused = word("unused");
    let plain = colours(&view, cx);
    if language.is_some() {
        assert!(
            plain.windows(2).any(|pair| pair[0] != pair[1]),
            "highlighted"
        );
    }

    // An untagged hint keeps its squiggle.
    diagnose(
        &view,
        vec![(unused.clone(), tagged(Severity::Hint, None))],
        cx,
    );
    assert_eq!(
        view.read_with(cx, |view, _| view.painted_squiggles.len()),
        1
    );
    assert_eq!(colours(&view, cx), plain);

    // Unnecessary: faded, no squiggle, the mark stays.
    diagnose(
        &view,
        vec![(
            unused.clone(),
            tagged(Severity::Hint, Some(DiagnosticTag::Unnecessary)),
        )],
        cx,
    );
    let faded = colours(&view, cx);
    assert_eq!(faded.len(), plain.len());
    for (ix, (now, before)) in faded.iter().zip(&plain).enumerate() {
        let inside = unused.contains(&(row_start + ix));
        match inside {
            true => assert_eq!(
                *now,
                Hsla {
                    a: before.a * 0.67,
                    ..*before
                },
                "byte {ix}"
            ),
            false => assert_eq!(now, before, "byte {ix}"),
        }
    }
    view.read_with(cx, |view, _| {
        assert!(view.painted_squiggles.is_empty());
        assert_eq!(view.painted_marks.len(), 1);
    });

    // Overlapping ones fade once.
    diagnose(
        &view,
        vec![
            (
                unused.clone(),
                tagged(Severity::Hint, Some(DiagnosticTag::Unnecessary)),
            ),
            (
                unused.start + 1..unused.end + 1,
                tagged(Severity::Hint, Some(DiagnosticTag::Unnecessary)),
            ),
        ],
        cx,
    );
    let twice = colours(&view, cx);
    let expected = unused.start..unused.end + 1;
    for (ix, (now, before)) in twice.iter().zip(&plain).enumerate() {
        match expected.contains(&(row_start + ix)) {
            true => assert_eq!(now.a, before.a * 0.67, "byte {ix}"),
            false => assert_eq!(now, before, "byte {ix}"),
        }
    }

    // Deprecated: struck through and still squiggled.
    let old = word("old");
    diagnose(
        &view,
        vec![(
            old.clone(),
            tagged(Severity::Warning, Some(DiagnosticTag::Deprecated)),
        )],
        cx,
    );
    assert_eq!(colours(&view, cx), plain);
    let (left, right) = (x(&view, old.start, cx), x(&view, old.end, cx));
    view.read_with(cx, |view, _| {
        assert_eq!(view.painted_squiggles.len(), 1);
        let [strike] = view.painted_strikes[..] else {
            panic!("one strike, got {:?}", view.painted_strikes);
        };
        assert_eq!((strike.left(), strike.right()), (left, right));
        assert_eq!(strike.size.height, gpui::px(1.));
        let squiggle = view.painted_squiggles[0].0;
        assert!(strike.top() < squiggle.top());
    });
}

#[gpui::test]
fn unnecessary_fades_and_deprecated_strikes(cx: &mut TestAppContext) {
    check(None, cx);
    check(Some("rust"), cx);
}
