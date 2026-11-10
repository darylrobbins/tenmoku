//! The status bar's cursor item is one whole message per shape, its count a
//! plural selector: `gpui-status-cursor`, `gpui-status-cursor-selected` and
//! `gpui-status-cursor-selections`, read through an editor's status items in
//! en-US and in a fake locale (`tests/fixtures/l10n/ru`). The locale is
//! process-wide, so this is the only test in its binary.

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView, pane_item::status};
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_l10n::LanguageIdentifier;
use kiln_shell::PaneItem as _;

kiln_l10n::resources! {
    "ru" => ["fixtures/l10n/ru/status.ftl"],
}

/// The cursor item's text.
fn cursor(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, cx| {
        (view.status_items(cx).into_iter())
            .find(|item| item.id == status::CURSOR)
            .map(|item| item.text.to_string())
            .expect("an editor has a cursor item")
    })
}

/// `count` carets, one at the start of each of the first rows: the last,
/// the primary, on row `count`.
fn carets(view: &Entity<EditorView>, count: usize, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        view.model_mut()
            .set_selections((0..count).map(|row| ResolvedSelection {
                range: row * 2..row * 2,
                reversed: false,
            }))
    });
}

/// One selection of the first `chars` characters, the caret after them.
fn selected(view: &Entity<EditorView>, chars: usize, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        view.model_mut().set_selections([ResolvedSelection {
            range: 0..chars,
            reversed: false,
        }])
    });
}

#[gpui::test]
fn cursor_messages_are_whole_with_plural_counts(cx: &mut TestAppContext) {
    cx.update(fe_gpui::init);
    // 30 rows of `a`: a caret per row, and up to 59 characters to select.
    let text = "a\n".repeat(30);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });

    let en: LanguageIdentifier = "en-US".parse().unwrap();
    kiln_l10n::set_locales(std::slice::from_ref(&en));
    carets(&view, 1, cx);
    assert_eq!(cursor(&view, cx), "Ln 1, Col 1");
    carets(&view, 3, cx);
    assert_eq!(cursor(&view, cx), "Ln 3, Col 1 (3 selections)");
    selected(&view, 1, cx);
    assert_eq!(cursor(&view, cx), "Ln 1, Col 2 (1 selected)");
    selected(&view, 17, cx);
    assert_eq!(cursor(&view, cx), "Ln 9, Col 2 (17 selected)");

    let ru: LanguageIdentifier = "ru".parse().unwrap();
    assert_eq!(
        kiln_l10n::set_locales(std::slice::from_ref(&ru)).first(),
        Some(&ru)
    );
    carets(&view, 1, cx);
    assert_eq!(cursor(&view, cx), "⟦1:1⟧");
    for (count, shown) in [
        (2, "⟦few: 2 carets, 2:1⟧"),
        (4, "⟦few: 4 carets, 4:1⟧"),
        (5, "⟦many: 5 carets, 5:1⟧"),
        (11, "⟦many: 11 carets, 11:1⟧"),
        (21, "⟦one: 21 carets, 21:1⟧"),
        (22, "⟦few: 22 carets, 22:1⟧"),
    ] {
        carets(&view, count, cx);
        assert_eq!(cursor(&view, cx), shown, "{count} carets");
    }
    for (chars, shown) in [
        (1, "⟦one: 1 chars, 1:2⟧"),
        (3, "⟦few: 3 chars, 2:2⟧"),
        (12, "⟦many: 12 chars, 7:1⟧"),
    ] {
        selected(&view, chars, cx);
        assert_eq!(cursor(&view, cx), shown, "{chars} characters");
    }

    kiln_l10n::set_locales(&[en]);
    carets(&view, 3, cx);
    assert_eq!(cursor(&view, cx), "Ln 3, Col 1 (3 selections)");
}
