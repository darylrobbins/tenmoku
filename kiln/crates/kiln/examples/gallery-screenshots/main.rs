// SPDX-License-Identifier: LicenseRef-Proprietary

//! Golden screenshots of the gallery: its Controls, Lists and Theme pages,
//! dark and light, each in a window wrapped in `kiln::Root` as the gallery
//! runs, drawn off screen with the platform's real renderer
//! (`kiln::platform::screenshots`).
//!
//! ```text
//! cargo run -p kiln --example gallery-screenshots                  # compare with golden/
//! UPDATE_SCREENSHOTS=1 cargo run -p kiln --example gallery-screenshots
//! cargo run -p kiln --example gallery-screenshots -- render out.png <page> [--size WxH]
//! ```
//!
//! `render` draws any page (`controls`, `text`, `lists`, …) in the theme
//! `KILN_GALLERY_THEME` names, dark unless it's `light`.
//!
//! The goldens are drawn by Metal on macOS, and only macOS compares with
//! them: elsewhere the comparison says so and exits cleanly. `render`
//! works wherever GPUI has a headless renderer, as on Linux with a Vulkan
//! device, in that platform's fonts.

#[path = "../gallery/app.rs"]
mod app;
#[path = "../gallery/commands.rs"]
mod commands;
#[path = "../gallery/pages/mod.rs"]
mod pages;
#[path = "../gallery/settings.rs"]
mod settings;
#[path = "../gallery/setup.rs"]
mod setup;

use std::{process::ExitCode, sync::Arc};

use gpui::{AppContext as _, Focusable as _, Pixels, Size, px, size};
use kiln::{
    core::Os,
    platform::screenshots::{
        Goldens, Offscreen, OffscreenApp, Result, RgbaImage, Settle, exit_code, offscreen_app,
        render_args,
    },
    prelude::*,
    ui::ThemeMode,
};

use crate::{
    app::Gallery,
    pages::{ControlsPage, ListsPage, Page},
    settings::GallerySettings,
};

/// The window size of every scene, in logical pixels.
const SCENE_SIZE: Size<Pixels> = size(px(1000.), px(640.));

/// The pages with goldens, each in both themes.
const PAGES: [Page; 3] = [Page::Controls, Page::Lists, Page::Theme];

const THEMES: [(ThemeMode, &str); 2] = [(ThemeMode::Dark, "dark"), (ThemeMode::Light, "light")];

/// `page`'s name in scene names and `render`'s argument: `controls`.
fn page_name(page: Page) -> String {
    format!("{page:?}").to_lowercase()
}

/// Opens the gallery on `page` in `mode`, inside its `Root`, and captures it
/// once it settles.
fn capture(
    cx: &mut OffscreenApp,
    page: Page,
    mode: ThemeMode,
    size: Size<Pixels>,
) -> Result<RgbaImage> {
    cx.update_app(|cx| cx.set_theme_mode(mode));
    let window = cx.open_offscreen_window(size, |window, cx| {
        let gallery = cx.new(|cx| Gallery::new(window, cx));
        gallery.update(cx, |gallery, cx| gallery.show(page, window, cx));
        window.focus(&gallery.focus_handle(cx), cx);
        let title = cx.settings::<GallerySettings>().title.clone();
        cx.new(|cx| {
            kiln::Root::new(gallery, window, cx)
                .title(title)
                .key_context(app::KEY_CONTEXT)
        })
    })?;
    let root = window.entity(cx)?;
    let gallery = root.read_with(cx, |root, _| {
        root.content()
            .clone()
            .downcast::<Gallery>()
            .expect("the gallery")
    });
    let page_view = gallery.read_with(cx, |gallery, _| gallery.page_view(page));
    let mut settle = Settle::new();
    settle.observe(&root, cx);
    settle.observe(&gallery, cx);
    // The page's own view, where it has state that can still change.
    if let Ok(view) = page_view.clone().downcast::<ControlsPage>() {
        settle.observe(&view, cx);
    }
    if let Ok(view) = page_view.downcast::<ListsPage>() {
        settle.observe(&view, cx);
    }
    Ok(settle.capture(cx, window.into())?.0)
}

/// Renders every scene and compares it with its golden, or rewrites the
/// goldens. Returns whether all of them matched.
fn check_scenes(cx: &mut OffscreenApp) -> Result<bool> {
    let goldens = Goldens::new(
        env!("CARGO_MANIFEST_DIR"),
        "examples/gallery-screenshots/golden",
    );
    let mut passed = true;
    for page in PAGES {
        for (mode, theme) in THEMES {
            let image = capture(cx, page, mode, SCENE_SIZE)?;
            let name = format!("gallery_{}_{theme}", page_name(page));
            passed &= goldens.check(&name, &image)?;
        }
    }
    Ok(passed)
}

/// `render <out.png> <page> [--size WxH]`
fn render(cx: &mut OffscreenApp, args: &[String]) -> Result<()> {
    let usage = "usage: gallery-screenshots render <out.png> <page> [--size WxH]";
    let (out, name, size) = render_args(args, SCENE_SIZE, usage)?;
    let page = Page::ALL
        .into_iter()
        .find(|page| page_name(*page) == name)
        .ok_or_else(|| {
            let names: Vec<_> = Page::ALL.into_iter().map(page_name).collect();
            format!("no page {name:?}; the pages are {}", names.join(", "))
        })?;
    let mode = match std::env::var("KILN_GALLERY_THEME").as_deref() {
        Ok("light") => ThemeMode::Light,
        _ => ThemeMode::Dark,
    };
    capture(cx, page, mode, size)?.save(out)?;
    println!("wrote {out}");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() && Os::current() != Os::MacOs {
        println!(
            "skipped: the gallery's goldens are drawn by Metal on macOS, so only macOS compares \
             with them (`render` works here)"
        );
        return ExitCode::SUCCESS;
    }
    let mut cx = offscreen_app(Arc::new(kiln::ui::Assets));
    let diagnostics = cx.update_app(|cx| kiln::init(cx, setup::config()));
    if !diagnostics.is_empty() {
        eprintln!("error: the gallery's config has problems: {diagnostics:?}");
        return ExitCode::FAILURE;
    }
    exit_code(match args.first().map(String::as_str) {
        Some("render") => render(&mut cx, &args[1..]).map(|()| true),
        None => check_scenes(&mut cx),
        Some(_) => Err("usage: gallery-screenshots [render <out.png> <page> [--size WxH]]".into()),
    })
}
