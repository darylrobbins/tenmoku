// SPDX-License-Identifier: Apache-2.0

//! Tabs: a click shows a tab's view, a close button asks the owner, who
//! removes the tab.

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Render, Styled as _, Subscription, TestAppContext, VisualTestContext,
    Window, div, point, px,
};

use crate::{Tabs, TabsEvent};

struct Page(&'static str);

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let name = self.0;
        div().size_full().debug_selector(move || name.into())
    }
}

/// Owns tabs one, two and three, removing a tab when asked to close it.
struct Host {
    tabs: Entity<Tabs>,
    events: Vec<TabsEvent>,
    _events: Subscription,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().w(px(500.)).h(px(300.)).child(self.tabs.clone()))
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn tab_bounds(
    host: &Entity<Host>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> gpui::Bounds<gpui::Pixels> {
    host.read_with(cx, |host, cx| {
        host.tabs
            .read(cx)
            .scroll_handle()
            .bounds_for_item(ix)
            .expect("tab drawn")
    })
}

#[gpui::test]
fn clicking_tabs_shows_their_views_and_closing_asks_the_owner(cx: &mut TestAppContext) {
    let (host, cx) = cx.add_window_view(|window, cx| {
        let pages = ["one", "two", "three"].map(|name| cx.new(|_| Page(name)));
        let tabs = cx.new(|_| {
            pages
                .into_iter()
                .zip(["One", "Two", "Three"])
                .fold(Tabs::new(), |tabs, (page, label)| {
                    tabs.with_tab(label, page)
                })
        });
        let events = cx.subscribe_in(&tabs, window, |host: &mut Host, tabs, event, _, cx| {
            host.events.push(*event);
            if let TabsEvent::CloseRequested(ix) = *event {
                tabs.update(cx, |tabs, cx| tabs.remove(ix, cx));
            }
        });
        Host {
            tabs,
            events: Vec::new(),
            _events: events,
        }
    });
    draw(cx);
    assert!(cx.debug_bounds("one").is_some(), "the first tab shown");
    assert!(cx.debug_bounds("two").is_none());

    let two = tab_bounds(&host, 1, cx);
    cx.simulate_click(
        point(two.left() + px(10.), two.center().y),
        Modifiers::none(),
    );
    draw(cx);
    assert!(cx.debug_bounds("two").is_some());
    assert!(cx.debug_bounds("one").is_none());

    // The close button is the last thing in the tab, before its padding.
    let two = tab_bounds(&host, 1, cx);
    cx.simulate_click(
        point(two.right() - px(15.), two.center().y),
        Modifiers::none(),
    );
    draw(cx);
    let events = host.update(cx, |host, _| std::mem::take(&mut host.events));
    assert_eq!(
        events,
        [TabsEvent::Activated(1), TabsEvent::CloseRequested(1)]
    );
    let (len, active) = host.read_with(cx, |host, cx| {
        let tabs = host.tabs.read(cx);
        (tabs.len(), tabs.active())
    });
    assert_eq!((len, active), (2, 1), "the tab after the closed one shown");
    assert!(cx.debug_bounds("three").is_some());
}

/// A strip of tabs as a screen reader sees it: one tab active, and each
/// tab's dirty, pinned and preview states and description as set.
mod a11y {
    use gpui::{
        Context, ElementId, Entity, IntoElement, ParentElement as _, Render, SharedString,
        TestAppContext, VisualTestContext, Window, div,
    };

    use crate::{Node, Role, Tab, TabBar, UiTheme, nodes};

    #[derive(Clone, Default)]
    struct Spec {
        name: &'static str,
        folder: Option<&'static str>,
        dirty: bool,
        pinned: bool,
        preview: bool,
    }

    struct Strip {
        tabs: Vec<Spec>,
        active: usize,
    }

    impl Render for Strip {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let ui = UiTheme::default();
            let tabs = self.tabs.iter().enumerate().map(|(ix, spec)| {
                Tab::new(("tab", ix), spec.name, &ui)
                    .description(spec.folder.map(SharedString::from))
                    .dirty(spec.dirty)
                    .pinned(spec.pinned)
                    .preview(spec.preview)
                    .active(ix == self.active)
            });
            div().child(TabBar::new("tabs", &ui).children(tabs))
        }
    }

    fn named(name: &'static str) -> Spec {
        Spec {
            name,
            ..Spec::default()
        }
    }

    fn strip(tabs: Vec<Spec>, cx: &mut TestAppContext) -> (Entity<Strip>, &mut VisualTestContext) {
        let (strip, cx) = cx.add_window_view(move |_, _| Strip { tabs, active: 0 });
        cx.run_until_parked();
        (strip, cx)
    }

    /// The nodes of a fresh draw.
    fn drawn(cx: &mut VisualTestContext) -> Vec<Node> {
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            nodes(window, cx)
        })
    }

    fn tabs(nodes: &[Node]) -> Vec<&Node> {
        nodes.iter().filter(|n| n.role == Role::Tab).collect()
    }

    /// Each tab's label, whether it's selected, and its description.
    fn summary(nodes: &[Node]) -> Vec<(String, Option<bool>, Option<String>)> {
        tabs(nodes)
            .into_iter()
            .map(|n| {
                let description = n.states.description.as_ref().map(|d| d.to_string());
                (n.label.to_string(), n.states.selected, description)
            })
            .collect()
    }

    #[gpui::test]
    fn the_strip_is_a_tab_list_of_tabs_in_order(cx: &mut TestAppContext) {
        let specs = vec![named("a.rs"), named("b.rs"), named("c.rs")];
        let (_, cx) = strip(specs, cx);
        let got = drawn(cx);
        let bar = got
            .iter()
            .find(|n| n.id == ElementId::from("tabs"))
            .expect("the strip's node");
        assert_eq!((bar.role, bar.label.as_ref()), (Role::TabList, "Tabs"));
        assert_eq!(
            summary(&got),
            [
                ("a.rs".into(), Some(true), None),
                ("b.rs".into(), Some(false), None),
                ("c.rs".into(), Some(false), None),
            ]
        );
        let places: Vec<_> = tabs(&got)
            .iter()
            .map(|n| (n.states.position_in_set, n.states.size_of_set))
            .collect();
        assert_eq!(
            places,
            [(Some(1), Some(3)), (Some(2), Some(3)), (Some(3), Some(3))]
        );
        // Every tab's close button is a button named for its tab, the
        // inactive ones' too, which show only on hovering.
        let closes: Vec<_> = got
            .iter()
            .filter(|n| n.role == Role::Button)
            .map(|n| n.label.to_string())
            .collect();
        assert_eq!(closes, ["Close a.rs", "Close b.rs", "Close c.rs"]);
    }

    #[gpui::test]
    fn switching_tabs_moves_the_selection(cx: &mut TestAppContext) {
        let specs = vec![named("a.rs"), named("b.rs"), named("c.rs")];
        let (strip, cx) = strip(specs, cx);
        strip.update(cx, |strip, cx| {
            strip.active = 2;
            cx.notify();
        });
        let selected: Vec<_> = summary(&drawn(cx))
            .into_iter()
            .map(|(label, selected, _)| (label, selected))
            .collect();
        assert_eq!(
            selected,
            [
                ("a.rs".into(), Some(false)),
                ("b.rs".into(), Some(false)),
                ("c.rs".into(), Some(true)),
            ]
        );
    }

    #[gpui::test]
    fn a_dirty_tab_says_modified_in_its_label(cx: &mut TestAppContext) {
        let specs = vec![named("a.rs"), named("b.rs")];
        let (strip, cx) = strip(specs, cx);
        assert_eq!(summary(&drawn(cx))[1], ("b.rs".into(), Some(false), None));

        strip.update(cx, |strip, cx| {
            strip.tabs[1].dirty = true;
            cx.notify();
        });
        let got = drawn(cx);
        assert_eq!(
            summary(&got)[1],
            (
                "b.rs, modified".into(),
                Some(false),
                Some("modified".into())
            )
        );
        // The close button keeps the tab's own name.
        assert!(
            got.iter()
                .any(|n| n.role == Role::Button && n.label.as_ref() == "Close b.rs"),
            "{got:#?}"
        );

        strip.update(cx, |strip, cx| {
            strip.tabs[1].dirty = false;
            cx.notify();
        });
        assert_eq!(summary(&drawn(cx))[1], ("b.rs".into(), Some(false), None));
    }

    #[gpui::test]
    fn pinned_preview_and_folder_are_described(cx: &mut TestAppContext) {
        let specs = vec![
            Spec {
                name: "mod.rs",
                folder: Some("src/a"),
                pinned: true,
                dirty: true,
                ..Spec::default()
            },
            Spec {
                name: "mod.rs",
                folder: Some("src/b"),
                preview: true,
                ..Spec::default()
            },
        ];
        let (_, cx) = strip(specs, cx);
        let got = drawn(cx);
        assert_eq!(
            summary(&got),
            [
                (
                    "mod.rs, src/a, modified".into(),
                    Some(true),
                    Some("modified, pinned".into())
                ),
                ("mod.rs, src/b".into(), Some(false), Some("preview".into())),
            ]
        );
        // A pinned tab has no close button.
        let closes: Vec<_> = got
            .iter()
            .filter(|n| n.role == Role::Button)
            .map(|n| n.label.to_string())
            .collect();
        assert_eq!(closes, ["Close mod.rs"]);
    }
}
