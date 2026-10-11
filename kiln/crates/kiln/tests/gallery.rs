// SPDX-License-Identifier: LicenseRef-Proprietary

//! The gallery example's own logic, headless: its keymap and menus load
//! cleanly on every platform, and its typed commands, from an invocation,
//! a key or a button, page through it and switch the theme.

#[path = "../examples/gallery/app.rs"]
mod app;
#[path = "../examples/gallery/commands.rs"]
mod commands;
#[path = "../examples/gallery/pages/mod.rs"]
mod pages;
#[path = "../examples/gallery/settings.rs"]
mod settings;
#[path = "../examples/gallery/setup.rs"]
mod setup;

use gpui::{AppContext as _, Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln::{
    command::Invocation,
    core::Os,
    platform::{Capabilities, commands::catalog},
    prelude::*,
    ui::{Appearance, ThemeMode},
};

use app::Gallery;
use pages::Page;

fn init(cx: &mut gpui::App) {
    let diagnostics = kiln::init(cx, setup::config());
    assert_eq!(diagnostics, []);
}

/// The gallery's window, set up as `main` does, its root focused.
fn gallery(cx: &mut TestAppContext) -> (Entity<Gallery>, &mut VisualTestContext) {
    cx.update(init);
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
    cx.update(|window, cx| window.run_command(invocation, cx).unwrap());
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
    cx.update(init);
    cx.update(|cx| {
        let catalog = catalog(cx);
        let config = setup::config();
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            let (keymap, diagnostics) = config.keymap_for(os, Some(&*catalog));
            assert_eq!(diagnostics, [], "keymap on {os:?}");
            // F10 opens the menus drawn in the window, which macOS hasn't.
            let bindings = if os == Os::MacOs { 7 } else { 8 };
            assert_eq!(keymap.bindings().len(), bindings, "keymap on {os:?}");
            let (menus, diagnostics) = config.menu_bar_for(os, Some(&*catalog));
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

/// Wrapped in a `Root` drawn as on Linux, F10 opens the gallery's menus in
/// the window, and an item chosen with the keys runs in the gallery.
#[gpui::test]
fn f10_opens_the_menus_in_the_window(cx: &mut TestAppContext) {
    cx.update(init);
    // `init` loads the keymap and menus for the host, and the gallery binds
    // F10 everywhere but macOS: load them as on Linux, as the window draws.
    cx.update(|cx| {
        let config = setup::config();
        let catalog = catalog(cx);
        let (keymap, diagnostics) = config.keymap_for(Os::Linux, Some(&*catalog));
        assert_eq!(diagnostics, []);
        let (menus, diagnostics) = config.menu_bar_for(Os::Linux, Some(&*catalog));
        assert_eq!(diagnostics, []);
        kiln::platform::keys::set_keymap(cx, keymap);
        kiln::platform::menus::install(cx, menus);
    });
    let (root, cx) = cx.add_window_view(|window, cx| {
        let gallery = cx.new(|cx| Gallery::new(window, cx));
        kiln::Root::new(gallery, window, cx)
            .capabilities(Capabilities::for_os(Os::Linux))
            .title(setup::APP_NAME)
    });
    let gallery = root.read_with(cx, |root, _| {
        root.content().clone().downcast::<Gallery>().unwrap()
    });
    cx.update(|window, cx| window.focus(&gallery.focus_handle(cx), cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("menu:Go").is_some());
    cx.simulate_keystrokes("f10 right");
    let open = root.read_with(cx, |root, cx| root.menu_bar().unwrap().read(cx).open_menu());
    assert_eq!(open, Some((1, Some(0))), "Go, Next Page");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(page(&gallery, cx), Page::Text);
}

#[gpui::test]
fn toggle_theme_flips_between_dark_and_light(cx: &mut TestAppContext) {
    let (_, cx) = gallery(cx);
    cx.update(|_, cx| cx.set_theme_mode(ThemeMode::Dark));
    run("gallery.toggle_theme", cx);
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Light);
    run("gallery.toggle_theme", cx);
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Dark);
}

#[gpui::test]
fn the_header_buttons_run_the_page_commands(cx: &mut TestAppContext) {
    let (gallery, cx) = gallery(cx);
    let click = |selector: &'static str, cx: &mut VisualTestContext| {
        let bounds = cx.debug_bounds(selector).expect(selector);
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.run_until_parked();
    };
    click("next-page", cx);
    assert_eq!(page(&gallery, cx), Page::Text);
    click("previous-page", cx);
    click("previous-page", cx);
    assert_eq!(page(&gallery, cx), Page::ALL[Page::ALL.len() - 1]);
}

#[gpui::test]
fn the_status_bar_labels_shortcuts_from_the_keymap(cx: &mut TestAppContext) {
    let (_, cx) = gallery(cx);
    let label = cx.update(|_, cx| cx.binding_label(&commands::NextPage::id()));
    let expected = if Os::current() == Os::MacOs {
        "⌘]"
    } else {
        "Ctrl+]"
    };
    assert_eq!(label.as_deref(), Some(expected));
    let quit = cx.update(|_, cx| cx.binding_label(&kiln::app::Quit::id()));
    assert!(quit.is_some(), "app.quit is bound");
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn keys_still_work_once_the_focused_page_is_hidden(cx: &mut TestAppContext) {
    let (gallery, cx) = gallery(cx);
    run("gallery.next_page", cx);
    let text = cx
        .read(|cx| gallery.read(cx).page_view(Page::Text))
        .downcast::<pages::TextPage>()
        .unwrap();
    cx.update(|window, cx| {
        let field = text.read(cx).field().focus_handle(cx);
        window.focus(&field, cx);
    });
    draw(cx);

    // The page goes, and its field with it: focus moves to the gallery,
    // so the next key still reaches it.
    cx.simulate_keystrokes(&sec("secondary-]"));
    draw(cx);
    assert_eq!(page(&gallery, cx), Page::Lists);
    cx.update(|window, cx| {
        assert!(gallery.focus_handle(cx).is_focused(window));
    });
    cx.simulate_keystrokes(&sec("secondary-]"));
    assert_eq!(page(&gallery, cx), Page::Layout);
}
