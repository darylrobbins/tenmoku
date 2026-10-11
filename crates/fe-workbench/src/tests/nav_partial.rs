//! Go to definition that finds nothing says whether the project is still
//! being indexed.

use std::{fs, path::PathBuf, sync::atomic::AtomicBool};

use fe_nav::{
    Definitions, NavError, NavRequest, NavigationProvider, Precision, Readiness, WorkspaceSymbol,
};
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use super::open_root;

/// Answers every definitions request with no items.
struct Empty {
    partial: bool,
}

impl NavigationProvider for Empty {
    fn name(&self) -> &str {
        "fake"
    }

    fn precision(&self) -> Precision {
        Precision::Search
    }

    fn readiness(&self, _: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(&self, _: &NavRequest, _: &AtomicBool) -> Result<Definitions, NavError> {
        Ok(Definitions {
            word: Some("helper".into()),
            items: Vec::new(),
            unambiguous: false,
            partial: self.partial,
        })
    }

    fn workspace_symbols(
        &self,
        _: &str,
        _: usize,
        _: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        Ok(Vec::new())
    }
}

/// The picker's status after f12 on `helper`, given an empty answer.
fn status_after(partial: bool, cx: &mut TestAppContext) -> String {
    let dir = tempfile::tempdir().unwrap();
    let root: PathBuf = fs::canonicalize(dir.path()).unwrap();
    fs::write(root.join("main.rs"), "fn main() {\n    helper();\n}\n").unwrap();
    let (workbench, cx) = open_root(cx, &root);
    let path = root.join("main.rs");
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(path, window, cx));
    });
    settle(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let navigation = workbench.read_with(cx, |workbench, _| workbench.navigation().clone());
    navigation.add(std::sync::Arc::new(Empty { partial }), 10);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .unwrap();
    editor.update(cx, |editor, cx| {
        let at = editor.model().text().to_string().find("helper").unwrap() + 1;
        editor.model_mut().set_cursor(at);
        cx.notify();
    });
    cx.simulate_keystrokes("f12");
    settle(cx);
    workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.nav_state.definitions.read(cx);
        assert!(picker.delegate.items().is_empty());
        picker.delegate.status_text()
    })
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn an_empty_partial_answer_says_the_project_is_still_being_indexed(cx: &mut TestAppContext) {
    assert_eq!(
        status_after(true, cx),
        "No definition found for `helper` yet: the project is still being indexed (by fake)"
    );
}

#[gpui::test]
fn an_empty_complete_answer_says_none_found(cx: &mut TestAppContext) {
    assert_eq!(
        status_after(false, cx),
        "No definition found for `helper` (by fake)"
    );
}
