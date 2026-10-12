//! The include and exclude fields: their globs narrow the search, and one
//! that doesn't parse is the panel's error.

use std::fs;

use fe_gpui::{EditorSettings, EditorTheme};
use fe_project::SearchOptions;
use fe_ui::TextInput;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::ShownRow;
use crate::{Launch, SearchPanel, Workbench};

/// A search panel over a project of a few files all containing `needle`,
/// searching on one thread.
fn searching(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<SearchPanel>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "a.rs",
        "b.md",
        "c.txt",
        "docs/d.rs",
        "docs/e.txt",
        "src/f.rs",
    ] {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "a needle\n").unwrap();
    }
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let search = workbench.read_with(cx, |workbench: &Workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    search.update(cx, |search, _| {
        search.set_options(SearchOptions {
            threads: 1,
            ..SearchOptions::default()
        })
    });
    (dir, search, cx)
}

/// Replaces `input`'s text with `text` by typing, as the keyboard does.
fn type_into(input: &Entity<TextInput>, text: &str, cx: &mut VisualTestContext) {
    input.update(cx, |input, cx| {
        input.clear(cx);
        input.insert(text, cx);
    });
    cx.run_until_parked();
}

/// Searches for `needle`, as Enter does, then types `include` and
/// `exclude` into the fields: only their edits rerun the search.
fn run(search: &Entity<SearchPanel>, include: &str, exclude: &str, cx: &mut VisualTestContext) {
    let (query, inc, exc) = search.read_with(cx, |search, _| {
        (
            search.input().clone(),
            search.include_input().clone(),
            search.exclude_input().clone(),
        )
    });
    type_into(&query, "needle", cx);
    search.update(cx, |search, cx| search.run(cx));
    cx.run_until_parked();
    type_into(&inc, include, cx);
    type_into(&exc, exclude, cx);
}

fn files(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    search.read_with(cx, |search, _| {
        search
            .shown_rows()
            .into_iter()
            .filter_map(|row| match row {
                ShownRow::File(rel, _) => Some(rel),
                ShownRow::Line(..) => None,
            })
            .collect()
    })
}

fn status(search: &Entity<SearchPanel>, cx: &mut VisualTestContext) -> Option<String> {
    search.read_with(cx, |search, _| search.status_text())
}

#[gpui::test]
fn include_narrows(cx: &mut TestAppContext) {
    let (_dir, search, cx) = searching(cx);
    run(&search, "", "", cx);
    assert_eq!(files(&search, cx).len(), 6);
    run(&search, "*.rs", "", cx);
    assert_eq!(
        files(&search, cx),
        ["a.rs", "docs/d.rs", "src/f.rs"],
        "{:?}",
        status(&search, cx)
    );
}

#[gpui::test]
fn exclude_lists(cx: &mut TestAppContext) {
    let (_dir, search, cx) = searching(cx);
    run(&search, "", "docs/, *.md", cx);
    assert_eq!(files(&search, cx), ["a.rs", "c.txt", "src/f.rs"]);
}

#[gpui::test]
fn invalid_glob(cx: &mut TestAppContext) {
    for in_include in [true, false] {
        let (_dir, search, cx) = searching(cx);
        let (include, exclude) = if in_include { ("a[", "") } else { ("", "a[") };
        run(&search, include, exclude, cx);
        let text = status(&search, cx).expect("an error line");
        assert!(text.starts_with("Invalid glob a[: "), "{text}");
        assert!(files(&search, cx).is_empty());
        run(&search, "", "", cx);
        assert_eq!(files(&search, cx).len(), 6);
        assert!(!status(&search, cx).unwrap().starts_with("Invalid"));
    }
}

#[gpui::test]
fn a_filter_with_focus_leaves_with_its_dock(cx: &mut TestAppContext) {
    let (_dir, search, cx) = searching(cx);
    let (query, include) = search.read_with(cx, |search, _| {
        (search.input().clone(), search.include_input().clone())
    });
    let focused = |input: &Entity<TextInput>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
    };
    cx.simulate_keystrokes("cmd-shift-f");
    assert!(focused(&query, cx));
    cx.update(|window, cx| window.focus(&include.focus_handle(cx), cx));
    assert!(focused(&include, cx));
    cx.simulate_keystrokes("cmd-shift-e");
    assert!(!focused(&include, cx), "focus left the hidden filter");
}
