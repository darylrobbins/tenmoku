//! The Markdown preview through its keys and the palette: a read-only tab
//! after its source's, or in the pane to the right with focus left in the
//! source; re-rendered once edits settle, in the editor's colours; and
//! its links opening files, the browser and headings.

use std::{fs, path::Path, sync::Arc, time::Duration};

use fe_config::theme::EditorPalette;
use fe_gpui::EditorView;
use fe_markdown_view::MarkdownViewEvent;
use fe_ui::{Theme, ThemeMode};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{draw, palette};
use crate::{
    Pane, Workbench,
    tests::open_root,
    workbench_commands::markdown_preview::{DELAY, MarkdownPreview},
};

/// A workbench rooted at a folder of `files`, with each opened in turn in
/// its pane, the last active and focused.
fn opened<'a>(
    files: &[(&str, &str)],
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    let pane = active_pane(&workbench, cx);
    for (name, _) in files.iter().filter(|(name, _)| !name.contains('/')) {
        let path = dir.path().join(name);
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    focus_editor(&workbench, cx);
    (dir, workbench, cx)
}

fn focus_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
}

fn active_pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench
            .pane_group(cx)
            .panes()
            .into_iter()
            .cloned()
            .collect()
    })
}

/// Each pane's tab titles and active tab.
fn layout(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Vec<String>, usize)> {
    let panes = panes(workbench, cx);
    panes
        .iter()
        .map(|pane| {
            pane.read_with(cx, |pane, cx| {
                let titles = (pane.tabs().iter())
                    .map(|tab| tab.item().tab_title(cx).to_string())
                    .collect();
                (titles, pane.active().unwrap())
            })
        })
        .collect()
}

/// The previews open, in layout order.
fn previews(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Entity<MarkdownPreview>> {
    let panes = panes(workbench, cx);
    panes
        .iter()
        .flat_map(|pane| {
            pane.read_with(cx, |pane, _| {
                (pane.tabs().iter())
                    .filter_map(|tab| tab.item().downcast_ref::<MarkdownPreview>().cloned())
                    .collect::<Vec<_>>()
            })
        })
        .collect()
}

fn active_editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).expect("an editor").clone()
    })
}

fn is_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// The slugs of the preview's document's headings.
fn headings(preview: &Entity<MarkdownPreview>, cx: &mut VisualTestContext) -> Vec<String> {
    preview.read_with(cx, |preview, cx| {
        let document = preview.view().read(cx).document().clone();
        document
            .headings()
            .map(|(slug, _)| slug.to_owned())
            .collect()
    })
}

#[gpui::test]
fn open_preview_opens_a_read_only_preview_tab(cx: &mut TestAppContext) {
    let files = [
        ("notes.txt", "notes\n"),
        ("README.md", "# Readme\n\nHello.\n"),
        ("main.rs", "fn main() {}\n"),
    ];
    let (_dir, workbench, cx) = opened(&files, cx);
    let pane = active_pane(&workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(1, window, cx)));
    focus_editor(&workbench, cx);
    let source = active_editor(&workbench, cx);

    cx.simulate_keystrokes("cmd-shift-v");
    draw(cx);
    let titles = ["notes.txt", "README.md", "Preview README.md", "main.rs"];
    assert_eq!(
        layout(&workbench, cx),
        [(titles.map(String::from).to_vec(), 2)]
    );
    let (path, dirty) = pane.read_with(cx, |pane, cx| {
        let tab = &pane.tabs()[2];
        (tab.path.clone(), tab.item().is_dirty(cx))
    });
    assert_eq!((path, dirty), (None, false));
    let preview = previews(&workbench, cx).remove(0);
    assert!(cx.update(|window, cx| preview.focus_handle(cx).is_focused(window)));
    let ours = preview.read_with(cx, |preview, _| preview.source().clone());
    assert_eq!(ours, source.downgrade());
    assert_eq!(headings(&preview, cx), ["readme"]);
    assert_eq!(
        cx.window_title().as_deref(),
        Some("Preview README.md — Tenmoku")
    );

    // The palette lists both, from the source.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(1, window, cx)));
    focus_editor(&workbench, cx);
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker = palette(&workbench, cx).expect("the palette shows");
    cx.simulate_input("markdown: open preview");
    draw(cx);
    let titles: Vec<String> = picker.read_with(cx, |picker, _| {
        (picker.delegate.matches().iter())
            .map(|found| found.title.to_string())
            .collect()
    });
    for title in [
        "Markdown: Open Preview",
        "Markdown: Open Preview to the Side",
    ] {
        assert!(titles.iter().any(|found| found == title), "{titles:?}");
    }
    cx.simulate_keystrokes("escape");
    draw(cx);

    // A Rust file has none.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(3, window, cx)));
    focus_editor(&workbench, cx);
    let before = layout(&workbench, cx);
    cx.simulate_keystrokes("cmd-shift-v");
    draw(cx);
    cx.simulate_keystrokes("cmd-k v");
    draw(cx);
    assert_eq!(layout(&workbench, cx), before);
    assert_eq!(previews(&workbench, cx).len(), 1);
}

#[gpui::test]
fn open_preview_to_the_side_splits_right_and_keeps_focus(cx: &mut TestAppContext) {
    let files = [("README.md", "# Readme\n"), ("other.md", "# Other\n")];
    let (dir, workbench, cx) = opened(&files[..1], cx);
    let source = active_editor(&workbench, cx);
    let left = active_pane(&workbench, cx);

    cx.simulate_keystrokes("cmd-k v");
    draw(cx);
    let readme = (vec!["README.md".to_owned()], 0);
    let preview = (vec!["Preview README.md".to_owned()], 0);
    assert_eq!(layout(&workbench, cx), [readme.clone(), preview.clone()]);
    assert_eq!(active_pane(&workbench, cx), left);
    assert!(is_focused(&source, cx));

    // Again: the preview there is shown, not a second.
    cx.simulate_keystrokes("cmd-k v");
    draw(cx);
    assert_eq!(layout(&workbench, cx), [readme, preview]);
    assert_eq!(active_pane(&workbench, cx), left);
    assert!(is_focused(&source, cx));

    // Another file's goes to the pane already to the right.
    let path = dir.path().join(files[1].0);
    fs::write(&path, files[1].1).unwrap();
    cx.update(|window, cx| left.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let other = active_editor(&workbench, cx);
    cx.simulate_keystrokes("cmd-k v");
    draw(cx);
    let titles = |names: &[&str]| {
        names
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        layout(&workbench, cx),
        [
            (titles(&["README.md", "other.md"]), 1),
            (titles(&["Preview README.md", "Preview other.md"]), 1),
        ]
    );
    assert_eq!(active_pane(&workbench, cx), left);
    assert!(is_focused(&other, cx));
}

#[gpui::test]
fn preview_rerenders_after_edits_settle(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&[("README.md", "Hello.\n")], cx);
    let source = active_editor(&workbench, cx);
    cx.simulate_keystrokes("cmd-k v");
    draw(cx);
    let preview = previews(&workbench, cx).remove(0);
    assert!(is_focused(&source, cx));
    assert_eq!(headings(&preview, cx), Vec::<String>::new());
    let document = |cx: &mut VisualTestContext| {
        preview.read_with(cx, |preview, cx| preview.view().read(cx).document().clone())
    };
    let before = document(cx);

    cx.simulate_input("# Title\n");
    draw(cx);
    cx.executor()
        .advance_clock(DELAY - Duration::from_millis(1));
    draw(cx);
    assert!(
        Arc::ptr_eq(&document(cx), &before),
        "re-rendered before 300 ms"
    );
    cx.executor().advance_clock(Duration::from_millis(1));
    draw(cx);
    assert_eq!(headings(&preview, cx), ["title"]);

    // A reload to the light theme, as fastedit's applies it: kiln's theme,
    // then the editor palette from it. The open source keeps its colours.
    let dark = preview.read_with(cx, |preview, cx| preview.view().read(cx).style().clone());
    let palette = Arc::new(EditorPalette::fastedit_light().clone());
    cx.update(|_, cx| {
        fe_ui::set_themes(cx, ThemeMode::Light, Theme::dark(), Theme::light());
        fe_gpui::set_editor_palette(cx, palette.clone());
    });
    draw(cx);
    let light = preview.read_with(cx, |preview, cx| preview.view().read(cx).style().clone());
    let mut theme = source.read_with(cx, |source, _| source.theme().clone());
    theme.set_colors(&palette);
    assert_ne!(light.background, dark.background);
    assert_eq!(light.background, theme.background);
    assert_eq!(light.code_font_size, theme.font_size);
    assert_eq!(light.border, cx.update(|_, cx| fe_ui::ui_theme(cx).border));
}

#[gpui::test]
fn preview_links_open_files_urls_and_anchors(cx: &mut TestAppContext) {
    let filler: String = (0..80).map(|n| format!("Paragraph {n}.\n\n")).collect();
    let readme = format!("# Readme\n\n[guide](docs/guide.md#L3)\n\n{filler}## Usage\n\nRun it.\n");
    let files = [
        ("README.md", readme.as_str()),
        ("docs/guide.md", "one\ntwo\nthree\nfour\n"),
    ];
    let (dir, workbench, cx) = opened(&files, cx);
    cx.simulate_keystrokes("cmd-k v");
    draw(cx);
    let preview = previews(&workbench, cx).remove(0);
    let right = panes(&workbench, cx)[1].clone();
    let view = preview.read_with(cx, |preview, _| preview.view().clone());
    let click = |dest: &str, cx: &mut VisualTestContext| {
        let dest = dest.to_owned();
        view.update(cx, |_, cx| cx.emit(MarkdownViewEvent::LinkClicked { dest }));
        draw(cx);
    };

    // A file, in the preview's pane, at its line.
    click("docs/guide.md#L3", cx);
    assert_eq!(active_pane(&workbench, cx), right);
    let guide = active_editor(&workbench, cx);
    let (path, row) = guide.read_with(cx, |guide, _| {
        let model = guide.model();
        let head = model.primary_selection().head();
        (
            model.path().map(Path::to_owned),
            model.text().offset_to_point(head).row,
        )
    });
    assert_eq!(path, Some(dir.path().join("docs/guide.md")));
    assert_eq!(row, 2);
    assert_eq!(
        layout(&workbench, cx)[1].0,
        ["Preview README.md", "guide.md"]
    );

    // A web page, in the browser.
    click("https://example.com", cx);
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com"));

    // A heading, scrolled to.
    cx.update(|window, cx| right.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    let offset =
        |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.scroll_handle().offset().y);
    assert_eq!(offset(cx), gpui::px(0.));
    click("#usage", cx);
    draw(cx);
    let scrolled = offset(cx);
    assert!(scrolled < gpui::px(0.), "{scrolled:?}");

    // Nothing for a script.
    let before = (layout(&workbench, cx), cx.opened_url());
    click("javascript:alert(1)", cx);
    assert_eq!((layout(&workbench, cx), cx.opened_url()), before);
    assert_eq!(offset(cx), scrolled);
}

#[gpui::test]
fn a_preview_follows_the_root_and_outlives_its_source(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&[("README.md", "# Readme\n")], cx);
    let source = active_editor(&workbench, cx);
    cx.simulate_keystrokes("cmd-shift-v");
    draw(cx);
    let preview = previews(&workbench, cx).remove(0);
    let view = preview.read_with(cx, |preview, _| preview.view().clone());
    let paths = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            (view.doc_path().to_owned(), view.root().map(Path::to_owned))
        })
    };

    // A Save As into another folder moves what relative links and images
    // resolve against.
    let moved = dir.path().join("docs/README.md");
    cx.update(|_, cx| {
        source.update(cx, |source, cx| {
            source.model_mut().set_path(Some(moved.clone()));
            cx.notify();
        })
    });
    draw(cx);
    assert_eq!(paths(cx), (moved.clone(), Some(dir.path().to_owned())));
    drop(source);

    // A rooted link, or image, resolves against the folder opened since.
    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("guide.md"), "guide\n").unwrap();
    let root = other.path().to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
    });
    draw(cx);
    assert_eq!(paths(cx), (moved, Some(other.path().to_owned())));
    let dest = "/guide.md".to_owned();
    view.update(cx, |_, cx| cx.emit(MarkdownViewEvent::LinkClicked { dest }));
    draw(cx);
    let opened = active_editor(&workbench, cx)
        .read_with(cx, |guide, _| guide.model().path().map(Path::to_owned));
    assert_eq!(opened, Some(other.path().join("guide.md")));

    // With the source closed, a split copies the last document.
    let pane = active_pane(&workbench, cx);
    for title in ["guide.md", "README.md"] {
        let ix = layout(&workbench, cx)[0]
            .0
            .iter()
            .position(|tab| tab == title);
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.activate(ix.unwrap(), window, cx);
                pane.close_active(window, cx);
            })
        });
        draw(cx);
    }
    assert!(preview.read_with(cx, |preview, _| preview.source().upgrade().is_none()));
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let preview = (vec!["Preview README.md".to_owned()], 0);
    assert_eq!(layout(&workbench, cx), [preview.clone(), preview]);
    let headings: Vec<_> = (previews(&workbench, cx).iter())
        .map(|preview| headings(preview, cx))
        .collect();
    assert_eq!(headings, [["readme"], ["readme"]]);
}
