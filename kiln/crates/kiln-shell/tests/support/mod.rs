// SPDX-License-Identifier: LicenseRef-Proprietary

//! A small app for the shell's tests: a `Pad` view in a [`Root`], with
//! commands, a keymap and menus, and what the chrome asked of the window.

#![allow(
    dead_code,
    reason = "each test file uses its own share; tests/it.rs holds the one copy"
)]

use std::{cell::RefCell, path::Path, rc::Rc};

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    Modifiers, Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use kiln_command::Invocation;
use kiln_core::{Keymap, KeymapLayer, MenuBar, Os};
use kiln_platform::{
    Capabilities,
    commands::{self, observe_invocations},
    keys, menus,
};
use kiln_shell::{
    Root,
    window_frame::{WindowRequest, intercept_window_requests},
};

pub(crate) mod note;

pub(crate) mod pad {
    kiln_platform::commands!(
        pad,
        [
            /// Clear
            Clear,
            /// Select All
            SelectAll,
        ]
    );
}

/// The app's root key context.
pub(crate) const APP: &str = "pad_app";

pub(crate) const MENUS: &str = r#"
[[menus]]
name = "Pad"

[[menus.items]]
title = "Clear"
command = "pad.clear"

[[menus.items]]
separator = true

[[menus.items]]
title = "Select All"
command = "pad.select_all"

[[menus]]
name = "Edit"

[[menus.items]]
title = "Select All"
command = "pad.select_all"

[[menus]]
name = "Window"

[[menus.items]]
title = "Close Window"
command = "window.close"
"#;

pub(crate) const KEYS: &str = r#"
[[bindings]]
keys = "f10"
command = "menu_bar.open"

[[bindings]]
keys = "secondary-a"
command = "pad.select_all"
"#;

/// The app's view: what its commands ran, in order.
pub(crate) struct Pad {
    pub(crate) focus: FocusHandle,
    pub(crate) ran: Vec<&'static str>,
}

impl Pad {
    fn clear(&mut self, _: &pad::Clear, _: &mut Window, _: &mut Context<Self>) {
        self.ran.push("pad.clear");
    }

    fn select_all(&mut self, _: &pad::SelectAll, _: &mut Window, _: &mut Context<Self>) {
        self.ran.push("pad.select_all");
    }
}

impl Focusable for Pad {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Pad {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "pad".into())
            .key_context("pad")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::clear))
            .on_action(cx.listener(Self::select_all))
            .size_full()
    }
}

/// Linux's chrome: menus in the window, no unified title bar.
pub(crate) fn linux() -> Capabilities {
    Capabilities::for_os(Os::Linux)
}

/// macOS's chrome: native menus and the unified title bar.
pub(crate) fn mac() -> Capabilities {
    Capabilities::for_os(Os::MacOs)
}

/// The app's commands, keymap and menus, then a window of a `Pad` in a
/// [`Root`] drawn as on a platform with `capabilities`, the pad focused.
pub(crate) fn open(
    cx: &mut TestAppContext,
    capabilities: Capabilities,
) -> (Entity<Root>, Entity<Pad>, &mut VisualTestContext) {
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
    let (root, cx) = cx.add_window_view(|window, cx| {
        let pad = cx.new(|cx| Pad {
            focus: cx.focus_handle(),
            ran: Vec::new(),
        });
        Root::new(pad, window, cx)
            .capabilities(capabilities)
            .title("Notes")
            .key_context(APP)
    });
    let pad = root.read_with(cx, |root, _| {
        root.content().clone().downcast::<Pad>().unwrap()
    });
    cx.update(|window, cx| window.focus(&pad.focus_handle(cx), cx));
    cx.run_until_parked();
    (root, pad, cx)
}

/// Every invocation observers hear of from now on.
pub(crate) fn record(cx: &mut VisualTestContext) -> Rc<RefCell<Vec<Invocation>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        })
    });
    seen
}

/// Every request the chrome makes of the window from now on, instead of
/// the window.
pub(crate) fn requests(cx: &mut VisualTestContext) -> Rc<RefCell<Vec<WindowRequest>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        intercept_window_requests(cx, move |request| sink.borrow_mut().push(request))
    });
    seen
}

pub(crate) fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
}

pub(crate) fn pad_focused(pad: &Entity<Pad>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| pad.focus_handle(cx).is_focused(window))
}

/// A view that draws nothing.
pub(crate) struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
