//! The dock's References and Search headers come from the catalogue: under
//! the fake locale `qps-ploc` the dock's title is the fake word, and English
//! again after switching back.

use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{DockPanel, Launch, Workbench};

/// A fake locale, `qps-ploc`, for this binary: the two docks' titles.
static FAKE_LOCALE: kiln_l10n::Resources = kiln_l10n::Resources {
    crate_name: "fe-workbench-dock-titles-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[kiln_l10n::ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/dock.ftl",
        source: "workbench-dock-references-title = ⟦Réfé⟧\nworkbench-dock-search-title = ⟦Séár⟧\n",
    }],
    dynamic_prefixes: &[],
};

kiln_l10n::__private::inventory::submit! { kiln_l10n::Registered(&FAKE_LOCALE) }

fn open(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn the_references_and_search_titles_follow_the_language(cx: &mut TestAppContext) {
    let (workbench, cx) = open(cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let title = |panel: DockPanel, cx: &mut VisualTestContext| {
        cx.update(|window, cx| dock.update(cx, |dock, cx| dock.show(panel, window, cx)));
        draw(cx);
        dock.read_with(cx, |dock, cx| {
            assert_eq!(dock.active(), panel);
            dock.title(cx).to_string()
        })
    };
    assert_eq!(title(DockPanel::References, cx), "References");
    assert_eq!(title(DockPanel::Search, cx), "Search");

    let fake: Vec<kiln_l10n::LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &fake));
    draw(cx);
    assert_eq!(title(DockPanel::References, cx), "⟦Réfé⟧");
    assert_eq!(title(DockPanel::Search, cx), "⟦Séár⟧");

    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &[]));
    draw(cx);
    assert_eq!(title(DockPanel::References, cx), "References");
    assert_eq!(title(DockPanel::Search, cx), "Search");
}
