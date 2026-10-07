//! M3 "Browse" signed off end to end: the tree, Cmd-P, search, the outline
//! and reveal on one 100,000-file project. Counts only; no timing is
//! asserted. It's one test so the fixture is generated once.

use std::fs;

use fe_project::{SearchOptions, fixture};
use gpui::{Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{click_row, open_root, panel};
use crate::{OutlinePanel, SearchPanel, Workbench, search_panel::ShownRow};

const FILES: usize = 100_000;
const RUST_NAME: &str = "zebra_outline_target.rs";
const RUST: &str = "fn one() {}\nfn two() {}\n// m3_unique_needle\nfn three() {}\n";
const COMMON_CAP: usize = 25;

fn listings(panel: &gpui::Entity<crate::ProjectPanel>, cx: &mut VisualTestContext) -> u64 {
    panel.read_with(cx, |panel, _| panel.tree().unwrap().stats().listings)
}

fn search_of(
    workbench: &gpui::Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> gpui::Entity<SearchPanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    })
}

#[gpui::test]
fn a_100k_file_project_end_to_end(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture::generate(root, 100, 1_000).unwrap();
    let target_dir = root.join(fixture::dir_name(7));
    fs::write(target_dir.join(RUST_NAME), RUST).unwrap();
    // A common needle in a few files of one directory.
    for ix in 0..5 {
        fs::write(
            root.join(fixture::dir_name(3))
                .join(format!("common_{ix}.md")),
            "m3_common_needle\n".repeat(10),
        )
        .unwrap();
    }
    // A deep file under directories nobody has opened.
    let deep_dir = root.join(fixture::dir_name(42)).join("a").join("b");
    fs::create_dir_all(&deep_dir).unwrap();
    let deep = deep_dir.join("deep_marker.txt");
    fs::write(&deep, "").unwrap();

    let (workbench, cx) = open_root(cx, root);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let panel = panel(&workbench, cx);
    cx.run_until_parked();

    // 1. Tree: only the root is listed; one expansion lists once.
    assert_eq!(listings(&panel, cx), 1, "only the root");
    let rows_of = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| panel.tree().unwrap().row_count())
    };
    let collapsed = rows_of(cx);
    click_row(&panel, &fixture::dir_name(0), cx);
    assert_eq!(listings(&panel, cx), 2, "one expansion lists once");
    assert_eq!(rows_of(cx), collapsed + 1_000, "the directory's files");
    let fit = panel.read_with(cx, |panel, _| {
        let row = panel.row_bounds(0).unwrap();
        (panel.list_bounds().size.height / row.size.height).ceil() as usize
    });
    let built: usize = panel
        .read_with(cx, |panel, _| panel.built())
        .iter()
        .map(|range| range.len())
        .sum();
    assert!(
        built <= fit + 1 + 2,
        "{built} rows built for {fit} that fit"
    );

    // 2. Cmd-P: the walk indexes every file, a name query ranks it first.
    cx.simulate_keystrokes("cmd-p");
    cx.run_until_parked();
    let picker = workbench.read_with(cx, |workbench, _| workbench.quick_open().clone());
    let progress = picker.read_with(cx, |picker, _| picker.delegate.index_progress());
    let indexed = progress.expect("indexing started");
    assert!(indexed.1, "the walk finished");
    assert_eq!(
        indexed.0,
        FILES + 1 + 7,
        "the fixture's files and .gitignore, and the seven files added"
    );
    let before = picker
        .read_with(cx, |picker, _| picker.delegate.index_stats())
        .unwrap()
        .indices_computed;
    cx.simulate_input("zebra_outline_target");
    let first = picker.read_with(cx, |picker, _| {
        picker
            .delegate
            .matches()
            .first()
            .map(|found| found.path.clone())
    });
    assert_eq!(
        first.as_deref(),
        Some(format!("{}/{RUST_NAME}", fixture::dir_name(7)).as_str())
    );
    let after = picker
        .read_with(cx, |picker, _| picker.delegate.index_stats())
        .unwrap()
        .indices_computed;
    assert!(after - before <= 50, "{} indices computed", after - before);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let rust = target_dir.join(RUST_NAME);
    let tabs = workbench.read_with(cx, |workbench, cx| {
        workbench
            .pane()
            .read(cx)
            .tabs()
            .iter()
            .map(|tab| tab.path.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(tabs, [Some(rust.clone())]);

    // 3. Search: a unique needle in one file, a common one capped.
    cx.simulate_keystrokes("cmd-shift-f");
    let search = search_of(&workbench, cx);
    let run = |query: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes("cmd-backspace");
        cx.simulate_input(query);
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
    };
    run("m3_unique_needle", cx);
    let rows = search.read_with(cx, |search, _| search.shown_rows());
    let expected_rel = format!("{}/{RUST_NAME}", fixture::dir_name(7));
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        matches!(&rows[0], ShownRow::File(rel, 1) if *rel == expected_rel),
        "{rows:?}"
    );
    assert!(matches!(&rows[1], ShownRow::Line(3, ..)), "{rows:?}");
    assert_eq!(
        search
            .read_with(cx, |search, _| search.status_text())
            .as_deref(),
        Some("1 result in 1 file")
    );

    // Clicking the match selects it.
    let bounds = search.read_with(cx, |search, _| search.row_bounds(1).expect("drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown");
    let (text, selection) = editor.read_with(cx, |editor, _| {
        let model = editor.model();
        (
            model.text().to_string(),
            model.selections()[0].range.clone(),
        )
    });
    let start = text.find("m3_unique_needle").unwrap();
    assert_eq!(selection, start..start + "m3_unique_needle".len());

    // The common needle stops at the cap and says so.
    search.update(cx, |search, _| {
        search.set_options(SearchOptions {
            max_matches: COMMON_CAP,
            threads: 1,
        })
    });
    cx.update(|window, cx| window.focus(&search.read(cx).input().focus_handle(cx), cx));
    run("m3_common_needle", cx);
    let rows = search.read_with(cx, |search, _| search.shown_rows());
    let lines = rows
        .iter()
        .filter(|row| matches!(row, ShownRow::Line(..)))
        .count();
    assert_eq!(lines, COMMON_CAP, "{rows:?}");
    assert_eq!(
        search
            .read_with(cx, |search, _| search.status_text())
            .as_deref(),
        Some(format!("Showing the first {COMMON_CAP} results").as_str())
    );

    // 4. Outline: three fns.
    cx.simulate_keystrokes("cmd-shift-o");
    cx.run_until_parked();
    let outline: gpui::Entity<OutlinePanel> = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    });
    let outline_rows = outline.read_with(cx, |outline, _| outline.rows());
    assert_eq!(outline_rows.len(), 3, "{outline_rows:?}");

    // 5. Reveal: Cmd-P opens a deep file; only its unloaded ancestors list.
    let before = listings(&panel, cx);
    cx.simulate_keystrokes("cmd-p");
    cx.simulate_input("deep_marker");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let active = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active().and_then(|ix| pane.tabs()[ix].path.clone())
    });
    assert_eq!(active, Some(deep));
    assert_eq!(
        listings(&panel, cx),
        before + 3,
        "dir_0042, a and b, once each"
    );
}
