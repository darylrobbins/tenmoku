//! The Problems panel in B3 (`docs/design/design-system.md#problems`): the
//! scope switch (This file lists the active editor's problems alone,
//! Project every file's), the severity filters that show their counts and
//! hide their severity while off, rows grouped by file with a count badge,
//! every problem row's severity glyph, the centred empty state, and the
//! counts' plural messages under a fake locale. The problems are put on the
//! open editors as a language server's would be
//! (`Workbench::set_diagnostics_for_audit`).

use std::{
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::diagnostics::{Diagnostic, Severity};
use fe_gpui::commands::RunCommand;
use gpui::{
    AppContext as _, Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext,
    VisualTestContext,
};

use super::open_root;
use crate::{
    Workbench,
    dock::problems_panel::{
        Filter, ProblemsPanel, Scope, ShownRow, badge_selector, filter_selector, glyph_selector,
        severity_glyph,
    },
};

/// `a.rs`'s text: each problem marks a word of it.
const A: &str = "fn alpha() {}\nfn beta() {}\nfn gamma() {}\nfn delta() {}\n";
/// `b.rs`'s.
const B: &str = "fn one() {}\n";

/// `word`'s bytes in `text`.
fn word(text: &str, word: &str) -> Range<usize> {
    let start = text.find(word).expect("the word is there");
    start..start + word.len()
}

fn diagnostic(severity: Severity, message: &str) -> Diagnostic {
    Diagnostic {
        source: Some("rustc".to_owned()),
        ..Diagnostic::new(severity, message)
    }
}

/// A file's problems, as `set_diagnostics_for_audit` takes them.
type Problems = Vec<(Range<usize>, Diagnostic)>;

/// `a.rs`'s problems: an error, a warning, an information and a hint;
/// `b.rs`'s: an error and a warning.
fn problems(root: &Path) -> Vec<(PathBuf, Problems)> {
    vec![
        (
            root.join("a.rs"),
            vec![
                (word(A, "alpha"), diagnostic(Severity::Error, "a error")),
                (word(A, "beta"), diagnostic(Severity::Warning, "a warning")),
                (
                    word(A, "gamma"),
                    diagnostic(Severity::Information, "a info"),
                ),
                (word(A, "delta"), diagnostic(Severity::Hint, "a hint")),
            ],
        ),
        (
            root.join("b.rs"),
            vec![
                (word(B, "one"), diagnostic(Severity::Error, "b error")),
                (word(B, "fn"), diagnostic(Severity::Warning, "b warning")),
            ],
        ),
    ]
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A workbench on a folder with `a.rs` and `b.rs`, both open, `b.rs`
/// active, with `problems` on them (unless it's false) and the Problems
/// panel shown.
fn opened(
    cx: &mut TestAppContext,
    with_problems: bool,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<ProblemsPanel>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    fs::write(root.join("a.rs"), A).unwrap();
    fs::write(root.join("b.rs"), B).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for name in ["a.rs", "b.rs"] {
        let path = root.join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    if with_problems {
        for (path, diagnostics) in problems(&root) {
            workbench.update(cx, |workbench, cx| {
                workbench.set_diagnostics_for_audit(&path, diagnostics, cx)
            });
        }
    }
    let run = RunCommand(
        Invocation::of("problems.show")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).problems().clone()
    });
    (dir, workbench, panel, cx)
}

fn rows(panel: &Entity<ProblemsPanel>, cx: &mut VisualTestContext) -> Vec<ShownRow> {
    panel.read_with(cx, |panel, _| panel.shown_rows())
}

/// The file rows' paths and counts.
fn files(panel: &Entity<ProblemsPanel>, cx: &mut VisualTestContext) -> Vec<(String, usize)> {
    (rows(panel, cx).into_iter())
        .filter_map(|row| match row {
            ShownRow::File(path, count) => Some((path, count)),
            _ => None,
        })
        .collect()
}

/// The problem rows' severity letters and messages.
fn problem_rows(
    panel: &Entity<ProblemsPanel>,
    cx: &mut VisualTestContext,
) -> Vec<(&'static str, String)> {
    (rows(panel, cx).into_iter())
        .filter_map(|row| match row {
            ShownRow::Problem(mark, message, ..) => Some((mark, message)),
            _ => None,
        })
        .collect()
}

fn bounds(selector: String, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(selector.leak())
}

fn click(selector: String, cx: &mut VisualTestContext) {
    let at = bounds(selector.clone(), cx).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(at.center(), Modifiers::none());
    draw(cx);
}

/// Whether `inner` lies inside `outer`.
fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.left() >= outer.left()
        && inner.right() <= outer.right()
        && inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
}

#[gpui::test]
fn this_file_lists_the_active_editors_problems_and_project_every_files(cx: &mut TestAppContext) {
    let (_dir, workbench, panel, cx) = opened(cx, true);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.scope()),
        Scope::Project
    );
    let both = vec![("a.rs".to_owned(), 4), ("b.rs".to_owned(), 2)];
    assert_eq!(files(&panel, cx), both);

    click("segment:This file".to_owned(), cx);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.scope()),
        Scope::ThisFile
    );
    // b.rs is the active tab.
    assert_eq!(files(&panel, cx), [("b.rs".to_owned(), 2)]);
    assert_eq!(
        problem_rows(&panel, cx),
        [("W", "b warning".to_owned()), ("E", "b error".to_owned())]
    );
    // The filters count the scope's problems.
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.filter_counts()),
        [1, 1, 0]
    );

    // Another tab active: its file's problems.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    assert_eq!(files(&panel, cx), [("a.rs".to_owned(), 4)]);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.filter_counts()),
        [1, 1, 2]
    );

    click("segment:Project".to_owned(), cx);
    assert_eq!(files(&panel, cx), both);
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.filter_counts()),
        [2, 2, 2]
    );
}

#[gpui::test]
fn each_filter_shows_its_count_and_hides_its_severity_when_off(cx: &mut TestAppContext) {
    let (_dir, _workbench, panel, cx) = opened(cx, true);
    // Information and hints count together.
    assert_eq!(
        panel.read_with(cx, |panel, _| panel.filter_counts()),
        [2, 2, 2]
    );
    let names: Vec<String> = (Filter::ALL.iter())
        .map(|filter| filter.name(2).to_string())
        .collect();
    assert_eq!(names, ["2 errors", "2 warnings", "2 infos"]);
    for filter in Filter::ALL {
        assert!(bounds(filter_selector(filter), cx).is_some(), "{filter:?}");
    }
    let all = problem_rows(&panel, cx);
    assert_eq!(all.len(), 6);

    for filter in Filter::ALL {
        click(filter_selector(filter), cx);
        let shown = problem_rows(&panel, cx);
        let marks: &[&str] = match filter {
            Filter::Errors => &["E"],
            Filter::Warnings => &["W"],
            Filter::Infos => &["I", "H"],
        };
        let expected: Vec<_> = (all.iter())
            .filter(|(mark, _)| !marks.contains(mark))
            .cloned()
            .collect();
        assert_eq!(shown, expected, "{filter:?} off");
        // Off, a filter still counts what it hides.
        assert_eq!(
            panel.read_with(cx, |panel, _| panel.filter_counts()),
            [2, 2, 2]
        );
        click(filter_selector(filter), cx);
        assert_eq!(problem_rows(&panel, cx), all, "{filter:?} back on");
    }

    // With errors and warnings off, b.rs shows none and isn't listed, and
    // a.rs's badge counts what's left.
    click(filter_selector(Filter::Errors), cx);
    click(filter_selector(Filter::Warnings), cx);
    assert_eq!(files(&panel, cx), [("a.rs".to_owned(), 2)]);
    // Every one off: nothing listed, and the empty state says why.
    click(filter_selector(Filter::Infos), cx);
    assert!(rows(&panel, cx).is_empty());
    let message = panel.read_with(cx, |panel, _| panel.message_text());
    let message = message.map(|(heading, _)| heading.to_string());
    assert_eq!(message.as_deref(), Some("No problems shown"));
}

#[gpui::test]
fn rows_are_grouped_by_file_with_a_count_badge(cx: &mut TestAppContext) {
    let (_dir, _workbench, panel, cx) = opened(cx, true);
    let rustc = Some("rustc".to_owned());
    let problem = |mark, message: &str, at: &str| {
        ShownRow::Problem(mark, message.to_owned(), rustc.clone(), at.to_owned())
    };
    assert_eq!(
        rows(&panel, cx),
        [
            ShownRow::File("a.rs".to_owned(), 4),
            problem("E", "a error", "1:4"),
            problem("W", "a warning", "2:4"),
            problem("I", "a info", "3:4"),
            problem("H", "a hint", "4:4"),
            ShownRow::File("b.rs".to_owned(), 2),
            problem("W", "b warning", "1:1"),
            problem("E", "b error", "1:4"),
        ]
    );
    for ix in [0, 5] {
        let row = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
        let badge = bounds(badge_selector(ix), cx).expect("the badge is drawn");
        assert!(inside(badge, row), "{badge:?} outside {row:?}");
    }
    // Problem rows have none.
    assert!(bounds(badge_selector(1), cx).is_none());
}

#[gpui::test]
fn every_row_carries_its_severity_glyph(cx: &mut TestAppContext) {
    let (_dir, _workbench, panel, cx) = opened(cx, true);
    let severities = [
        ("E", Severity::Error),
        ("W", Severity::Warning),
        ("I", Severity::Information),
        ("H", Severity::Hint),
    ];
    let mut glyphs = Vec::new();
    for (ix, row) in rows(&panel, cx).into_iter().enumerate() {
        let ShownRow::Problem(mark, ..) = row else {
            continue;
        };
        let (_, severity) = *(severities.iter())
            .find(|(letter, _)| *letter == mark)
            .expect("a severity");
        let glyph = severity_glyph(severity);
        let drawn = bounds(glyph_selector(ix, glyph), cx).expect("the glyph is drawn");
        let row = panel.read_with(cx, |panel, _| panel.row_bounds(ix).expect("drawn"));
        assert!(inside(drawn, row), "{drawn:?} outside {row:?}");
        assert!(drawn.size.width > gpui::px(0.), "{drawn:?}");
        glyphs.push(glyph);
    }
    assert_eq!(glyphs.len(), 6);
    // Each severity has a glyph of its own.
    let mut distinct: Vec<_> = severities
        .iter()
        .map(|(_, severity)| severity_glyph(*severity).name())
        .collect();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 4);
}

#[gpui::test]
fn the_empty_state_is_centred(cx: &mut TestAppContext) {
    let (_dir, _workbench, panel, cx) = opened(cx, false);
    let message = panel.read_with(cx, |panel, _| panel.message_text());
    let message = message.map(|(heading, hint)| (heading.to_string(), hint.to_string()));
    let expected = (
        "No problems",
        "Errors and warnings in the open files show here",
    );
    assert_eq!(
        message,
        Some((expected.0.to_owned(), expected.1.to_owned()))
    );
    let panel_bounds = cx
        .debug_bounds("sidebar-panel")
        .expect("the panel is drawn");
    let empty = cx
        .debug_bounds("problems-empty")
        .expect("the empty state is drawn");
    assert!(
        inside(empty, panel_bounds),
        "{empty:?} outside {panel_bounds:?}"
    );
    // Below the header, it fills the rest of the panel, its content
    // centred in it.
    let centre = |bounds: Bounds<Pixels>| bounds.center().x;
    assert!(
        (centre(empty) - centre(panel_bounds)).abs() <= gpui::px(1.),
        "{empty:?} off centre in {panel_bounds:?}"
    );
    assert_eq!(empty.bottom(), panel_bounds.bottom());
    assert!(empty.size.height > panel_bounds.size.height / 2.);
}

/// A fake locale, `qps-ploc`, for this binary: the Problems panel's counts.
static FAKE_LOCALE: kiln_l10n::Resources = kiln_l10n::Resources {
    crate_name: "fe-workbench-problems-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[kiln_l10n::ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/problems.ftl",
        source: "\
workbench-problems-filter-errors = { $count ->
    [one] ⟦{ $count } érrör⟧
   *[other] ⟦{ $count } érrörs⟧
}
workbench-problems-status-tooltip = ⟦{ $errors ->
    [one] { $errors } érrör
   *[other] { $errors } érrörs
} · { $warnings ->
    [one] { $warnings } wärning
   *[other] { $warnings } wärnings
}⟧
",
    }],
    dynamic_prefixes: &[],
};

kiln_l10n::__private::inventory::submit! { kiln_l10n::Registered(&FAKE_LOCALE) }

#[gpui::test]
fn the_counts_are_plural_messages(cx: &mut TestAppContext) {
    let (dir, workbench, _panel, cx) = opened(cx, true);
    let tooltip = |cx: &mut VisualTestContext| {
        let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
        let items = cx.read_entity(&bar, |bar, cx| bar.items(cx));
        let item = (items.into_iter())
            .find(|item| item.id.as_ref() == crate::dock::problems_panel::ITEM)
            .expect("the errors' item");
        item.tooltip.map(|tooltip| tooltip.to_string())
    };
    assert_eq!(tooltip(cx).as_deref(), Some("2 errors, 2 warnings"));
    assert_eq!(Filter::Errors.name(1).as_ref(), "1 error");

    let fake: Vec<kiln_l10n::LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &fake));
    draw(cx);
    assert_eq!(Filter::Errors.name(1).as_ref(), "⟦1 érrör⟧");
    assert_eq!(Filter::Errors.name(2).as_ref(), "⟦2 érrörs⟧");
    // The status items' tooltip is made as the counts change: b.rs's
    // problems cleared, one of each is left.
    let b = dir.path().join("b.rs");
    workbench.update(cx, |workbench, cx| {
        workbench.set_diagnostics_for_audit(&b, Vec::new(), cx)
    });
    draw(cx);
    assert_eq!(tooltip(cx).as_deref(), Some("⟦1 érrör · 1 wärning⟧"));

    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &[]));
    draw(cx);
    assert_eq!(Filter::Errors.name(1).as_ref(), "1 error");
}
