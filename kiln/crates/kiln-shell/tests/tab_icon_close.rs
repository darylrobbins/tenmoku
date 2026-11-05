// SPDX-License-Identifier: Apache-2.0

//! A tab's icon and close button: a file item's tab draws its file-type
//! icon before its title, and only the active tab, or the one the pointer
//! is over, draws its ×; a dirty tab's dot shows either way. Read from the
//! tabs the pane builds, and from the elements of the frame it paints.

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    Modifiers, ParentElement as _, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, point, px,
};
use kiln_shell::{Icon, ItemHandle, ItemPane, PaneItem, PaneView as _, file_icon};

struct Host(Entity<ItemPane>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().child(self.0.clone())
    }
}

/// A file's item, its icon the project tree's for its name, as an
/// editor's is.
struct File {
    name: SharedString,
    dirty: bool,
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
        self.name.clone()
    }

    fn tab_icon(&self, _: &gpui::App) -> Option<Icon> {
        Some(file_icon(&self.name))
    }

    fn is_dirty(&self, _: &gpui::App) -> bool {
        self.dirty
    }
}

/// A pane of `main.rs`, `notes.md` and a dirty `todo.txt`, `main.rs`
/// active, drawn with the pointer outside it.
fn pane(cx: &mut TestAppContext) -> (Entity<ItemPane>, &mut VisualTestContext) {
    let (host, cx) = cx.add_window_view(|_, cx| Host(cx.new(ItemPane::new)));
    let pane = host.read_with(cx, |host, _| host.0.clone());
    for (name, dirty) in [("main.rs", false), ("notes.md", false), ("todo.txt", true)] {
        let file = cx.new(|cx| File {
            name: name.into(),
            dirty,
            focus: cx.focus_handle(),
        });
        let item: Box<dyn ItemHandle> = Box::new(file);
        pane.update(cx, |pane, cx| pane.add_item(item, cx));
    }
    pane.update(cx, |pane, _| pane.pane_mut().set_active(0));
    cx.run_until_parked();
    cx.simulate_mouse_move(point(px(400.), px(200.)), None, Modifiers::none());
    draw(cx);
    (pane, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Which of the tabs drew their ×, and which their dirty dot, in the last
/// frame.
fn drawn(cx: &mut VisualTestContext) -> [[bool; 3]; 2] {
    let close = [
        "tab-close:main.rs",
        "tab-close:notes.md",
        "tab-close:todo.txt",
    ];
    let dirty = [
        "tab-dirty:main.rs",
        "tab-dirty:notes.md",
        "tab-dirty:todo.txt",
    ];
    [close, dirty].map(|selectors| selectors.map(|selector| cx.debug_bounds(selector).is_some()))
}

#[gpui::test]
fn a_file_tab_draws_its_file_type_icon(cx: &mut TestAppContext) {
    let (pane, cx) = pane(cx);
    let icons = cx.update(|_, cx| {
        let tabs = pane.read(cx).pane().tabs(cx);
        tabs.iter().map(|tab| tab.icon_kind()).collect::<Vec<_>>()
    });
    assert_eq!(
        icons,
        [
            Some(Icon::FileCode),
            Some(Icon::FileText),
            Some(Icon::FileText)
        ],
        "the project tree's icons for the names"
    );
    assert_eq!(file_icon("main.rs"), Icon::FileCode);

    // Painted in its tab, active or not.
    let selectors = [
        "tab-icon:main.rs:file-code",
        "tab-icon:notes.md:file-text",
        "tab-icon:todo.txt:file-text",
    ];
    for (ix, selector) in selectors.into_iter().enumerate() {
        let icon = cx.debug_bounds(selector).expect(selector);
        let tab = cx.update(|_, cx| pane.read(cx).pane().tab_bounds(ix).expect("drawn"));
        assert!(
            tab.contains(&icon.center()),
            "{selector}: {icon:?} in {tab:?}"
        );
    }
    // First in the tab, the × last.
    let icon = cx.debug_bounds(selectors[0]).unwrap();
    let close = cx
        .debug_bounds("tab-close:main.rs")
        .expect("the active tab's ×");
    assert!(icon.right() < close.left(), "{icon:?} {close:?}");
}

#[gpui::test]
fn only_the_active_or_hovered_tab_draws_its_close_button(cx: &mut TestAppContext) {
    let (pane, cx) = pane(cx);
    assert_eq!(
        drawn(cx),
        [[true, false, false], [false, false, true]],
        "the active tab's ×, and the inactive dirty tab's dot"
    );

    // Over the inactive `notes.md`, its × shows.
    let notes = cx.update(|_, cx| pane.read(cx).pane().tab_bounds(1).expect("drawn"));
    cx.simulate_mouse_move(notes.center(), None, Modifiers::none());
    draw(cx);
    assert_eq!(drawn(cx), [[true, true, false], [false, false, true]]);
    let close = cx.debug_bounds("tab-close:notes.md").expect("drawn");
    assert!(notes.contains(&close.center()));

    // Over the dirty `todo.txt`, its dot stays, and `notes.md` hides its ×.
    let todo = cx.update(|_, cx| pane.read(cx).pane().tab_bounds(2).expect("drawn"));
    cx.simulate_mouse_move(todo.center(), None, Modifiers::none());
    draw(cx);
    assert_eq!(drawn(cx), [[true, false, false], [false, false, true]]);

    // The shown × closes its tab, showing the one after it.
    cx.simulate_mouse_move(notes.center(), None, Modifiers::none());
    draw(cx);
    let close = cx.debug_bounds("tab-close:notes.md").expect("drawn");
    cx.simulate_click(close.center(), Modifiers::none());
    cx.run_until_parked();
    draw(cx);
    let titles = cx.update(|_, cx| {
        let pane = pane.read(cx).pane();
        let titles: Vec<_> = (pane.items().iter())
            .map(|item| item.tab_title(cx).to_string())
            .collect();
        (
            titles,
            pane.active_item()
                .map(|item| item.tab_title(cx).to_string()),
        )
    });
    assert_eq!(
        titles,
        (
            vec!["main.rs".into(), "todo.txt".into()],
            Some("todo.txt".into())
        )
    );
}
