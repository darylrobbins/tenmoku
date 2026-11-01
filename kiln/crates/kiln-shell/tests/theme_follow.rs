// SPDX-License-Identifier: LicenseRef-Proprietary

//! A theme change reaches every part of the shell at once: the shell's
//! own background and text, a dock's panel and rail, the panes and the
//! borders between them, and a picker in the modal layer, all draw in the
//! active theme rather than the one they were built with.

use crate::support;

use std::collections::HashSet;

use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, Focusable, Hsla, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, TestAppContext, VisualTestContext,
    Window, div,
};
use kiln_shell::{
    Direction, Dock, DockItem, DockPosition, ItemHandle, ItemPane, ModalId, PaneGroup, PanelId,
    Picker, PickerDelegate, Shell,
};
use kiln_ui::{Icon, Theme, UiTheme, set_theme};
use support::note::note;

/// A dock's panel, empty.
struct Panel(FocusHandle);

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.0.clone()
    }
}

impl DockItem for Panel {
    fn id(&self) -> PanelId {
        PanelId("panel")
    }

    fn title(&self, _: &App) -> SharedString {
        "Panel".into()
    }

    fn icon(&self, _: &App) -> Icon {
        Icon::Files
    }
}

/// The shell's centre: a group of panes.
struct Centre {
    group: PaneGroup<ItemPane>,
    focus: FocusHandle,
}

impl Render for Centre {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let built_with = UiTheme::default_dark();
        div().size_full().flex().child(self.group.render(
            &built_with,
            |centre| &mut centre.group,
            cx,
        ))
    }
}

impl Focusable for Centre {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A picker's rows.
struct Words;

impl PickerDelegate for Words {
    fn update_matches(&mut self, _: String, _: &mut Context<Picker<Self>>) {}

    fn match_count(&self) -> usize {
        3
    }

    fn match_id(&self, ix: usize) -> SharedString {
        ix.to_string().into()
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        div().child(ix.to_string()).into_any_element()
    }

    fn confirm(&mut self, _: usize, _: &mut Window, _: &mut Context<Picker<Self>>) -> bool {
        true
    }

    fn dismissed(&mut self, _: &mut Window, _: &mut Context<Picker<Self>>) {}
}

/// Every colour the last frame painted a quad's background or border in.
fn painted(cx: &mut VisualTestContext) -> HashSet<[u32; 4]> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, _| {
        let mut colors = HashSet::new();
        for quad in window.painted_quads() {
            colors.extend(quad.background.as_solid().map(key));
            let widths = quad.border_widths;
            let bordered = [widths.top, widths.right, widths.bottom, widths.left]
                .iter()
                .any(|width| width.0 > 0.);
            if bordered {
                colors.insert(key(quad.border_color));
            }
        }
        colors
    })
}

fn key(color: Hsla) -> [u32; 4] {
    [color.h, color.s, color.l, color.a].map(f32::to_bits)
}

/// The colours each part of the shell draws in, by what draws them.
fn drawn(ui: &UiTheme) -> [(&'static str, Hsla); 4] {
    [
        ("the shell's and a pane's background", ui.background),
        ("the dock's panel", ui.panel_background),
        ("a pane's borders and the dock's", ui.border),
        ("the picker's selected row", ui.row_selected),
    ]
}

#[gpui::test]
fn the_shell_follows_a_theme_change(cx: &mut TestAppContext) {
    let (shell, cx) = cx.add_window_view(|window, cx| Shell::new("app", window, cx));
    let built_with = UiTheme::default_dark();
    // Two panes side by side, a note in each.
    let (left, right) = (cx.new(ItemPane::new), cx.new(ItemPane::new));
    for (pane, title) in [(&left, "a"), (&right, "b")] {
        let item: Box<dyn ItemHandle> = Box::new(note(title, cx));
        pane.update(cx, |pane, cx| pane.add_item(item, cx));
    }
    let centre = cx.new(|cx| {
        let mut group = PaneGroup::new(left.clone());
        group.split(&left, right.clone(), Direction::Right);
        Centre {
            group,
            focus: cx.focus_handle(),
        }
    });
    let panel = cx.new(|cx| Panel(cx.focus_handle()));
    let dock = cx.new(|cx| {
        let mut dock = Dock::new(DockPosition::Right);
        dock.add_panel(panel, cx);
        dock
    });
    let picker = cx.new(|cx| Picker::new(Words, "Pick…", built_with, cx));
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.set_center(centre, cx);
            shell.set_dock(DockPosition::Right, Some(dock.into()), cx);
            picker.update(cx, |picker, cx| picker.show(window, cx));
            shell.show_modal(ModalId("pick"), picker, window, cx);
        })
    });
    cx.run_until_parked();

    let dark = Theme::dark().ui_theme();
    let colors = painted(cx);
    for (what, color) in drawn(&dark) {
        assert!(colors.contains(&key(color)), "dark: {what}");
    }

    cx.update(|_, cx| set_theme(cx, Theme::light()));
    let light = Theme::light().ui_theme();
    let colors = painted(cx);
    for (what, color) in drawn(&light) {
        assert!(colors.contains(&key(color)), "light: {what}");
    }
    // Nothing is left in the dark theme's colours.
    let lights: HashSet<_> = drawn(&light).map(|(_, color)| key(color)).into();
    for (what, color) in drawn(&dark) {
        let stale = colors.contains(&key(color)) && !lights.contains(&key(color));
        assert!(!stale, "{what} still dark");
    }
}
