// SPDX-License-Identifier: LicenseRef-Proprietary

//! A tab's description: tabs that share a title are told apart by the
//! fewest trailing folders of their paths that differ, as VS Code does,
//! and a tab with a title of its own, or no path, has none. Read from the
//! tabs the pane builds.

use std::path::{Path, PathBuf};

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, TestAppContext, VisualTestContext,
    Window, div,
};
use kiln_shell::{ItemHandle, ItemPane, PaneItem, PaneView as _};

struct Host(Entity<ItemPane>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

/// An item titled by its path's file name, or `untitled` with no path.
struct File {
    path: Option<PathBuf>,
    focus: FocusHandle,
}

impl Render for File {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for File {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for File {
    const KIND: &'static str = "test.file";

    fn tab_title(&self, _: &gpui::App) -> SharedString {
        let name = self.path.as_deref().and_then(Path::file_name);
        name.map_or("untitled".into(), |name| {
            name.to_string_lossy().into_owned().into()
        })
    }

    fn path(&self, _: &gpui::App) -> Option<&Path> {
        self.path.as_deref()
    }
}

/// A pane holding a tab per path in `paths`, `None` for an untitled item.
fn pane_of<'a>(
    paths: &[Option<&str>],
    cx: &'a mut TestAppContext,
) -> (Entity<ItemPane>, &'a mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(ItemPane::new)));
    let pane = host.read_with(cx, |host, _| host.0.clone());
    for path in paths {
        let path = path.map(PathBuf::from);
        let file = cx.new(|cx| File {
            path,
            focus: cx.focus_handle(),
        });
        let item: Box<dyn ItemHandle> = Box::new(file);
        pane.update(cx, |pane, cx| pane.add_item(item, cx));
    }
    cx.run_until_parked();
    (pane, cx)
}

/// Each tab's title and description, as the pane builds them.
fn described(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Vec<(String, Option<String>)> {
    cx.update(|_, cx| {
        let tabs = pane.read(cx).pane().tabs(cx);
        let items = pane.read(cx).pane().items();
        (tabs.iter().zip(items))
            .map(|(tab, item)| {
                let description = tab.description_text().map(ToString::to_string);
                (item.tab_title(cx).to_string(), description)
            })
            .collect()
    })
}

fn pair(title: &str, description: Option<&str>) -> (String, Option<String>) {
    (title.to_owned(), description.map(str::to_owned))
}

#[gpui::test]
fn same_titled_tabs_show_their_shortest_distinguishing_folders(cx: &mut TestAppContext) {
    let (pane, cx) = pane_of(
        &[
            Some("crates/vim/src/ex.rs"),
            Some("crates/vim/src/tests/ex.rs"),
            Some("a/src/y.rs"),
            Some("b/src/y.rs"),
            Some("z.rs"),
            None,
        ],
        cx,
    );
    assert_eq!(
        described(&pane, cx),
        [
            pair("ex.rs", Some("src")),
            pair("ex.rs", Some("tests")),
            pair("y.rs", Some("a/src")),
            pair("y.rs", Some("b/src")),
            pair("z.rs", None),
            pair("untitled", None),
        ]
    );
}

// A path whose folders end another's is described by its whole parent; two
// views of one file, or a same-titled item with no path, tell nothing apart.
#[gpui::test]
fn a_tab_out_of_folders_shows_its_whole_parent(cx: &mut TestAppContext) {
    let (pane, cx) = pane_of(
        &[
            Some("a/src/x.rs"),
            Some("src/x.rs"),
            Some("one/w.rs"),
            Some("one/w.rs"),
            Some("untitled/untitled"),
            None,
        ],
        cx,
    );
    assert_eq!(
        described(&pane, cx),
        [
            pair("x.rs", Some("a/src")),
            pair("x.rs", Some("src")),
            pair("w.rs", None),
            pair("w.rs", None),
            pair("untitled", None),
            pair("untitled", None),
        ]
    );
}

#[gpui::test]
fn a_description_goes_when_its_twin_closes(cx: &mut TestAppContext) {
    let (pane, cx) = pane_of(&[Some("src/ex.rs"), Some("src/tests/ex.rs")], cx);
    assert_eq!(
        described(&pane, cx),
        [pair("ex.rs", Some("src")), pair("ex.rs", Some("tests"))]
    );

    // The second tab, the one just added, is active.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.close_active_item(window, cx)));
    cx.run_until_parked();
    assert_eq!(described(&pane, cx), [pair("ex.rs", None)]);
}
