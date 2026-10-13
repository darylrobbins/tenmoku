//! The status bar's icons, tones and tooltips: the diagnostics pair, the
//! language server's and config problems' items, an editor's clickable
//! items and the file viewer's `Read-only`.

use std::{fs, sync::Arc};

use fe_fileview::{FileView, LineIndex};
use fe_gpui::{EditorTheme, FileViewer};
use fe_lsp::ServerStatus;
use gpui::{AppContext as _, Context, IntoElement, Render, TestAppContext, Window};
use kiln_shell::{Icon, ItemHandle as _, StatusItem, StatusTone};

use super::open_root;
use crate::{
    config_problems,
    dock::problems_panel::{ITEM, WARNINGS_ITEM},
    lsp::{lsp_status_item, status_text},
};

/// The bar's items now, by id.
fn bar_items(
    workbench: &gpui::Entity<crate::Workbench>,
    cx: &mut gpui::VisualTestContext,
) -> Vec<StatusItem> {
    let bar = workbench.read_with(cx, |w, _| w.status_bar().clone());
    cx.read_entity(&bar, |bar, cx| bar.items(cx))
}

fn find<'a>(items: &'a [StatusItem], id: &str) -> Option<&'a StatusItem> {
    items.iter().find(|item| item.id == id)
}

#[gpui::test]
fn diagnostics_pair(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let counts = |errors, warnings| fe_editor::diagnostics::DiagnosticCounts {
        errors,
        warnings,
        ..Default::default()
    };
    let set = |counts, cx: &mut gpui::VisualTestContext| {
        workbench.update(cx, |w, cx| {
            w.show_problem_counts(counts, cx);
        });
        bar_items(&workbench, cx)
    };

    let items = set(counts(2, 1), cx);
    let (errors, warnings) = (
        find(&items, ITEM).unwrap(),
        find(&items, WARNINGS_ITEM).unwrap(),
    );
    assert_eq!(errors.text, "2");
    assert_eq!(
        (errors.icon, errors.tone),
        (Some(Icon::CircleX), StatusTone::Error)
    );
    assert_eq!(warnings.text, "1");
    assert_eq!(
        (warnings.icon, warnings.tone),
        (Some(Icon::TriangleAlert), StatusTone::Warning)
    );
    for item in [errors, warnings] {
        assert_eq!(item.tooltip.as_deref(), Some("2 errors, 1 warning"));
        assert_eq!(
            item.command.as_ref().unwrap().command.as_str(),
            "problems.show"
        );
    }

    let items = set(counts(0, 1), cx);
    let errors = find(&items, ITEM).unwrap();
    assert_eq!(
        (errors.text.as_ref(), errors.tone),
        ("0", StatusTone::Default)
    );
    assert_eq!(
        find(&items, WARNINGS_ITEM).unwrap().tone,
        StatusTone::Warning
    );

    let items = set(counts(0, 0), cx);
    assert!(find(&items, ITEM).is_none() && find(&items, WARNINGS_ITEM).is_none());
}

#[gpui::test]
fn lsp_and_config(_: &mut TestAppContext) {
    let tone = |status: ServerStatus, message: Option<&str>| {
        let (text, tone) = status_text("rust-analyzer", &status, message).unwrap();
        let item = lsp_status_item(text.clone(), tone);
        assert_eq!(item.text, text);
        assert_eq!(item.icon, Some(Icon::Server));
        assert_eq!(item.tooltip.as_deref(), Some("Show Language Server Log"));
        item.tone
    };
    assert_eq!(
        tone(ServerStatus::Stopped(Some("boom".into())), None),
        StatusTone::Error
    );
    assert_eq!(
        tone(ServerStatus::Starting, Some("a message")),
        StatusTone::Error
    );
    assert_eq!(tone(ServerStatus::Restarting(1), None), StatusTone::Warning);
    assert_eq!(tone(ServerStatus::Starting, None), StatusTone::Default);
    assert_eq!(
        tone(ServerStatus::Indexing { title: "x".into() }, None),
        StatusTone::Default
    );

    let config = config_problems::status_item(2).unwrap();
    assert_eq!(config.text, "2 config problems");
    assert_eq!(config.icon, Some(Icon::FileCog));
    assert_eq!(config.tone, StatusTone::Warning);
    assert_eq!(config.tooltip.as_deref(), Some("Show Config Problems"));
}

struct Host(gpui::Entity<FileViewer>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0.clone()
    }
}

#[gpui::test]
fn editor_and_viewer(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo").unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let pane = workbench.read_with(cx, |w, _| w.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    cx.run_until_parked();
    let editor = workbench
        .read_with(cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a file is shown");
    let items = cx.update(|_, cx| editor.status_items(cx));
    let tooltip = |id: &str| {
        find(&items, id)
            .unwrap()
            .tooltip
            .as_deref()
            .map(str::to_owned)
    };
    assert_eq!(tooltip("cursor").as_deref(), Some("Go to Line/Column"));
    assert_eq!(tooltip("language").as_deref(), Some("Change Language Mode"));

    let view = Arc::new(FileView::open(&path).unwrap());
    let index = Arc::new(LineIndex::build(view.clone()));
    let (host, cx) = cx.add_window_view(|_, cx| {
        let theme = EditorTheme::default_dark(cx);
        Host(cx.new(|cx| FileViewer::new(view, index, "Read only".into(), theme, cx)))
    });
    let viewer = host.read_with(cx, |host, _| host.0.clone());
    let items = cx.update(|_, cx| viewer.status_items(cx));
    let read_only = items.iter().find(|item| item.text == "Read-only").unwrap();
    assert_eq!(read_only.icon, Some(Icon::Lock));
}

#[gpui::test]
fn lsp_tone_follows_a_same_text_message(cx: &mut TestAppContext) {
    let mut s = super::lsp::Lsp::new(cx);
    std::fs::write(s.path("a.rs"), "fn main() {}\n").unwrap();
    s.open("a.rs");
    let uri = s.uri("a.rs");
    s.fake
        .wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    let tone = |s: &mut super::lsp::Lsp, message: Option<&str>| {
        let message = message.map(str::to_owned);
        s.workbench.update(&mut s.cx, |w, cx| {
            let status = ServerStatus::Indexing { title: "x".into() };
            w.force_lsp_status(status, message, cx);
        });
        let items = bar_items(&s.workbench, &mut s.cx);
        find(&items, "lsp").map(|item| (item.text.to_string(), item.tone))
    };
    let (text, _) = tone(&mut s, None).unwrap();
    assert!(text.ends_with("indexing x"), "{text}");
    assert_eq!(
        tone(&mut s, Some(&text)),
        Some((text.clone(), StatusTone::Error))
    );
    assert_eq!(tone(&mut s, None), Some((text, StatusTone::Default)));
}
