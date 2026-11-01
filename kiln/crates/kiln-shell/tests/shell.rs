// SPDX-License-Identifier: LicenseRef-Proprietary

//! The shell around a host's centre: docks either side of it, the status
//! bar below, hidden and cached, the app's root key context its commands
//! run in, the chrome as each platform has it, and the modal layer, which
//! gives focus back to the centre.

use crate::support;

use std::path::Path;

use gpui::{
    AppContext as _, Bounds, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, canvas, div,
};
use kiln_core::{Keymap, KeymapLayer, MenuBar};
use kiln_platform::{commands, keys, menus};
use kiln_shell::{Dock, DockItem, DockPosition, ModalId, ModalView, PanelId, Shell};
use kiln_ui::{Icon, ROW_HEIGHT};
use std::{cell::Cell, rc::Rc};
use support::{APP, KEYS, MENUS, Pad, linux, mac, pad};

/// Records where it was laid out.
struct Probe {
    bounds: Rc<Cell<Bounds<Pixels>>>,
    renders: Rc<Cell<usize>>,
    focus: FocusHandle,
}

impl Probe {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            bounds: Rc::default(),
            renders: Rc::default(),
            focus: cx.focus_handle(),
        }
    }
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        let bounds = self.bounds.clone();
        div()
            .track_focus(&self.focus)
            .size_full()
            .child(canvas(move |laid_out, _, _| bounds.set(laid_out), |_, _, _, _| {}).size_full())
    }
}

impl Focusable for Probe {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DockItem for Probe {
    fn id(&self) -> PanelId {
        PanelId("probe")
    }

    fn title(&self, _: &gpui::App) -> gpui::SharedString {
        "Probe".into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        Icon::Files
    }
}

impl EventEmitter<DismissEvent> for Probe {}

impl ModalView for Probe {}

struct Opened<'a> {
    shell: Entity<Shell>,
    pad: Entity<Pad>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn pad_bounds(&mut self) -> Bounds<Pixels> {
        self.cx.debug_bounds("pad").expect("the pad is drawn")
    }
}

/// The app's commands, keymap and menus, then a shell around a pad, under
/// the app's context, its chrome as `capabilities` has it.
fn open(cx: &mut TestAppContext, capabilities: kiln_platform::Capabilities) -> Opened<'_> {
    cx.update(|cx| {
        commands::init(cx, "Pad");
        pad::register(cx);
        kiln_shell::register(cx);
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), KEYS, None);
        assert_eq!(diagnostics, []);
        keys::init(cx, Keymap::from_layers([layer.unwrap()]));
        let (bar, diagnostics) = MenuBar::parse(Path::new("menus.toml"), MENUS, None);
        assert_eq!(diagnostics, []);
        menus::install(cx, bar.unwrap());
    });
    let made = Rc::new(Cell::new(None));
    let sink = made.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let pad = cx.new(|cx| Pad {
            focus: cx.focus_handle(),
            ran: Vec::new(),
        });
        sink.set(Some(pad.clone()));
        let mut shell = Shell::new(APP, window, cx).platform_chrome(capabilities, cx);
        shell.set_center(pad, cx);
        shell.set_title("Notes", cx);
        shell
    });
    let pad = made.take().unwrap();
    cx.update(|window, cx| window.focus(&pad.focus_handle(cx), cx));
    let mut opened = Opened { shell, pad, cx };
    opened.draw();
    opened
}

#[gpui::test]
fn docks_beside_the_centre_and_the_status_bar_below(cx: &mut TestAppContext) {
    let mut s = open(cx, linux());
    let alone = s.pad_bounds();
    let (left, right, bar) = (
        s.cx.new(Probe::new),
        s.cx.new(Probe::new),
        s.cx.new(Probe::new),
    );
    let docks = [(DockPosition::Left, &left), (DockPosition::Right, &right)].map(|(at, panel)| {
        let panel = panel.clone();
        let dock = s.cx.new(|cx| {
            let mut dock = Dock::new(at);
            dock.add_panel(panel, cx);
            dock
        });
        (at, dock)
    });
    s.shell.update(s.cx, |shell, cx| {
        for (at, dock) in &docks {
            shell.set_dock(*at, Some(dock.clone().into()), cx);
        }
        shell.set_status_bar(Some(bar.clone().into()), cx);
    });
    s.draw();
    let pad = s.pad_bounds();
    let read = |probe: &Entity<Probe>, s: &mut Opened| probe.read_with(s.cx, |p, _| p.bounds.get());
    let (left, right, status) = (
        read(&left, &mut s),
        read(&right, &mut s),
        read(&bar, &mut s),
    );
    assert!(left.right() <= pad.left(), "{left:?} {pad:?}");
    assert!(right.left() >= pad.right(), "{right:?} {pad:?}");
    assert_eq!(status.size.height, ROW_HEIGHT);
    assert_eq!(status.top(), pad.bottom());
    assert_eq!(pad.bottom(), alone.bottom() - ROW_HEIGHT);
    assert!(s.shell.read_with(s.cx, |shell, _| {
        shell.dock(DockPosition::Left).is_some() && shell.dock(DockPosition::Bottom).is_none()
    }));

    // Hidden, the bar gives its row back; shown, it's drawn from its cache
    // unless the host asks for it afresh.
    s.shell
        .update(s.cx, |shell, cx| shell.set_status_bar_visible(false, cx));
    s.draw();
    assert_eq!(s.pad_bounds().bottom(), alone.bottom());
    s.shell
        .update(s.cx, |shell, cx| shell.set_status_bar_visible(true, cx));
    s.draw();
    let renders = bar.read_with(s.cx, |bar, _| bar.renders.clone());
    let before = renders.get();
    let pad = s.pad.clone();
    pad.update(s.cx, |_, cx| cx.notify());
    s.draw();
    assert_eq!(renders.get(), before, "from its cache");
    s.shell.update(s.cx, |shell, _| shell.redraw_status_bar());
    pad.update(s.cx, |_, cx| cx.notify());
    s.draw();
    // Afresh, then again for the cache the next frame draws from.
    assert!(renders.get() > before, "afresh");
    let before = renders.get();
    pad.update(s.cx, |_, cx| cx.notify());
    s.draw();
    assert_eq!(renders.get(), before, "from its cache again");
}

#[gpui::test]
fn the_chrome_follows_the_platform(cx: &mut TestAppContext) {
    let mut s = open(cx, linux());
    // F10, in the app's context, opens the menu bar the shell draws.
    let bar = s
        .shell
        .read_with(s.cx, |shell, _| shell.menu_bar().cloned())
        .expect("Linux has no native menus");
    s.cx.simulate_keystrokes("f10");
    s.draw();
    assert_eq!(
        bar.read_with(s.cx, |bar, _| bar.open_menu()),
        Some((0, Some(0))),
        "Pad, Clear"
    );
    let context = s
        .shell
        .read_with(s.cx, |shell, _| shell.key_context().clone());
    assert_eq!(context, APP);
    // The item's command reaches the pad beneath the shell.
    s.cx.simulate_keystrokes("enter");
    s.draw();
    assert_eq!(
        s.pad.read_with(s.cx, |pad, _| pad.ran.clone()),
        ["pad.clear"]
    );
    assert!(s.cx.debug_bounds("title-bar").is_none());
}

#[gpui::test]
fn macos_has_the_unified_title_bar(cx: &mut TestAppContext) {
    let mut s = open(cx, mac());
    assert!(
        s.shell
            .read_with(s.cx, |shell, _| shell.menu_bar().is_none())
    );
    let title = s.cx.debug_bounds("title-bar").expect("drawn");
    let pad = s.pad_bounds();
    assert_eq!(title.bottom(), pad.top(), "the pad under the title bar");
}

#[gpui::test]
fn a_modal_gives_focus_back_to_the_centre(cx: &mut TestAppContext) {
    let mut s = open(cx, linux());
    let modal = s.cx.new(Probe::new);
    let shell = s.shell.clone();
    let shown = modal.clone();
    s.cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.show_modal(ModalId("probe"), shown, window, cx)
        })
    });
    s.draw();
    assert_eq!(
        s.shell.read_with(s.cx, |shell, cx| shell.modal(cx)),
        Some(ModalId("probe"))
    );
    let focused =
        s.cx.update(|window, cx| modal.focus_handle(cx).is_focused(window));
    assert!(focused);
    s.cx.update(|window, cx| shell.update(cx, |shell, cx| shell.dismiss_modal(window, cx)));
    s.draw();
    assert_eq!(s.shell.read_with(s.cx, |shell, cx| shell.modal(cx)), None);
    let pad = s.pad.clone();
    assert!(s.cx.update(|window, cx| pad.focus_handle(cx).is_focused(window)));
    let center = s.shell.read_with(s.cx, |shell, cx| shell.center_focus(cx));
    assert_eq!(
        center,
        Some(pad.read_with(s.cx, |pad, _| pad.focus.clone()))
    );
}
