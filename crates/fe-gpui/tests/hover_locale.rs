//! A diagnostic's hover with a related location stays open when the locale
//! changes, and says it in the new one: the popover compares diagnostics by
//! what they are, not by their translated text (T549 review F3). The
//! locale is process-wide, so this is the only test in its binary.

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, RelatedInfo, Severity},
};
use fe_gpui::{EditorTheme, EditorView, init};
use gpui::{Focusable as _, TestAppContext};
use kiln_l10n::LanguageIdentifier;

kiln_l10n::resources! {
    "ru" => ["fixtures/l10n/ru/hover.ftl"],
}

const TEXT: &str = "let a = 1;\nlet b = a;\n";

#[gpui::test]
fn a_locale_switch_keeps_a_diagnostic_hover(cx: &mut TestAppContext) {
    let en: LanguageIdentifier = "en-US".parse().unwrap();
    kiln_l10n::set_locales(std::slice::from_ref(&en));
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), None);
        let caret = TEXT.rfind('a').unwrap();
        model.set_selections(vec![ResolvedSelection {
            range: caret..caret,
            reversed: false,
        }]);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    view.update(cx, |view, cx| {
        let used = TEXT.rfind('a').unwrap();
        let mut diagnostic = Diagnostic::new(Severity::Warning, "moved");
        diagnostic.related = vec![RelatedInfo {
            path: None,
            row: 0,
            range: Some(4..5),
            message: "defined here".into(),
        }];
        let version = view.model().snapshot().version();
        let items = vec![(used..used + 1, diagnostic)];
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, (1, 1), version, items, Some((1, 1)))
        );
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-k cmd-i");
    cx.run_until_parked();
    let shown =
        |cx: &mut gpui::VisualTestContext| view.read_with(cx, |view, _| view.popover_text());
    assert_eq!(shown(cx).as_deref(), Some("moved\n↳ line 1: defined here"));

    let ru: LanguageIdentifier = "ru".parse().unwrap();
    assert_eq!(
        kiln_l10n::set_locales(std::slice::from_ref(&ru)).first(),
        Some(&ru)
    );
    // Still standing, now in the new locale, and drawn again so.
    assert_eq!(
        shown(cx).as_deref(),
        Some("moved\n⟦⟦row 1⟧ → defined here⟧")
    );
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(
        shown(cx).as_deref(),
        Some("moved\n⟦⟦row 1⟧ → defined here⟧")
    );

    kiln_l10n::set_locales(std::slice::from_ref(&en));
    assert_eq!(shown(cx).as_deref(), Some("moved\n↳ line 1: defined here"));
}
