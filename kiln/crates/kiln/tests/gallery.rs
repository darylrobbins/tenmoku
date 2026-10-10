// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery example's own logic, headless: its keymap and menus load
//! cleanly on every platform, and its typed commands, from an invocation
//! or a key, page through it and switch the theme.

#[path = "../examples/gallery/app.rs"]
mod app;
#[path = "../examples/gallery/commands.rs"]
mod commands;
#[path = "../examples/gallery/pages/mod.rs"]
mod pages;
#[path = "../examples/gallery/setup.rs"]
mod setup;

use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::{
    command::Invocation,
    core::Os,
    dispatch_invocation,
    platform::commands::catalog,
    prelude::*,
    ui::{Appearance, ThemeMode},
};

use app::{Gallery, set_mode};
use pages::Page;

/// The gallery's window, set up as `main` does, its root focused.
fn gallery(cx: &mut TestAppContext) -> (Entity<Gallery>, &mut VisualTestContext) {
    cx.update(setup::init);
    let (gallery, cx) = cx.add_window_view(Gallery::new);
    cx.update(|window, cx| {
        let focus = gallery.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (gallery, cx)
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(command).unwrap();
    cx.update(|window, cx| dispatch_invocation(invocation, window, cx).unwrap());
    cx.run_until_parked();
}

fn page(gallery: &Entity<Gallery>, cx: &mut VisualTestContext) -> Page {
    cx.read(|cx| gallery.read(cx).page())
}

/// `keys` with this platform's `secondary`, in GPUI's spelling.
fn sec(keys: &str) -> String {
    let secondary = if Os::current() == Os::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    keys.replace("secondary", secondary)
}

#[gpui::test]
fn keymap_and_menus_load_cleanly_on_every_platform(cx: &mut TestAppContext) {
    cx.update(setup::init);
    cx.update(|cx| {
        let catalog = catalog(cx);
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            let (keymap, diagnostics) = setup::keymap(os, Some(&*catalog));
            assert_eq!(diagnostics, [], "keymap on {os:?}");
            assert_eq!(keymap.bindings().len(), 7, "keymap on {os:?}");
            let (menus, diagnostics) = setup::menu_bar(os, Some(&*catalog));
            assert_eq!(diagnostics, [], "menus on {os:?}");
            assert_eq!(menus.menus.len(), 3, "menus on {os:?}");
        }
    });
}

#[gpui::test]
fn page_commands_move_through_the_pages_and_wrap(cx: &mut TestAppContext) {
    let (gallery, cx) = gallery(cx);
    assert_eq!(page(&gallery, cx), Page::Controls);
    run("gallery.next_page", cx);
    run("gallery.next_page", cx);
    assert_eq!(page(&gallery, cx), Page::Lists);
    for _ in 0..3 {
        run("gallery.previous_page", cx);
    }
    let last = Page::ALL[Page::ALL.len() - 1];
    assert_eq!(page(&gallery, cx), last);
    run("gallery.next_page", cx);
    assert_eq!(page(&gallery, cx), Page::Controls);
}

#[gpui::test]
fn keys_from_the_keymap_run_the_page_commands(cx: &mut TestAppContext) {
    let (gallery, cx) = gallery(cx);
    cx.simulate_keystrokes(&sec("secondary-]"));
    assert_eq!(page(&gallery, cx), Page::Text);
    cx.simulate_keystrokes(&sec("secondary-[ secondary-["));
    assert_eq!(page(&gallery, cx), Page::ALL[Page::ALL.len() - 1]);
}

#[gpui::test]
fn toggle_theme_flips_between_dark_and_light(cx: &mut TestAppContext) {
    let (_, cx) = gallery(cx);
    cx.update(|_, cx| set_mode(ThemeMode::Dark, cx));
    run("gallery.toggle_theme", cx);
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Light);
    run("gallery.toggle_theme", cx);
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Dark);
}
