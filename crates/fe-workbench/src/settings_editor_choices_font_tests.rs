//! The Settings tab's model and font dropdowns: `chat.model` lists the
//! Claude models the app knows and keeps an id it doesn't, Other…
//! swapping the row to an input for one; `editor.font_family` and
//! `terminal.font_family` list the text system's fonts, resolved when the
//! dropdown opens.
//!
//! The helpers, and why the tests that write run in a process of their
//! own, are [`choices_tests`](super::choices_tests)'.

use std::{
    borrow::Cow,
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use fe_config::chat_models::KNOWN;
use gpui::{
    Bounds, DevicePixels, NoopTextSystem, PlatformTextSystem, Size, TestAppContext, TestDispatcher,
    VisualTestContext,
};

use super::{
    Scope, SettingsEditor,
    choices_tests::{
        bounds, click, draw, drawn, in_own_process_of, open_list, open_settings, option, press,
        read, search_for, workbench,
    },
    controls::{self, DEFAULT_CHOICE},
};

/// The fonts the text system has, out of order and twice over.
const FONTS: &[&str] = &["Menlo", "Fira Code", "Menlo", "Iosevka", "Fira Code"];

/// [`FONTS`] as the dropdown lists them: sorted, each once.
const LISTED: &[&str] = &["Fira Code", "Iosevka", "Menlo"];

/// The test platform's text system, with [`FONTS`] installed, counting
/// how often they're listed.
struct Fonts(NoopTextSystem, Arc<AtomicUsize>);

impl PlatformTextSystem for Fonts {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> gpui::Result<()> {
        self.0.add_fonts(fonts)
    }
    fn all_font_names(&self) -> Vec<String> {
        self.1.fetch_add(1, Ordering::SeqCst);
        FONTS.iter().map(|font| (*font).to_owned()).collect()
    }
    fn font_id(&self, descriptor: &gpui::Font) -> gpui::Result<gpui::FontId> {
        self.0.font_id(descriptor)
    }
    fn font_metrics(&self, font_id: gpui::FontId) -> gpui::FontMetrics {
        self.0.font_metrics(font_id)
    }
    fn typographic_bounds(
        &self,
        font_id: gpui::FontId,
        glyph_id: gpui::GlyphId,
    ) -> gpui::Result<Bounds<f32>> {
        self.0.typographic_bounds(font_id, glyph_id)
    }
    fn advance(&self, font_id: gpui::FontId, glyph_id: gpui::GlyphId) -> gpui::Result<Size<f32>> {
        self.0.advance(font_id, glyph_id)
    }
    fn glyph_for_char(&self, font_id: gpui::FontId, ch: char) -> Option<gpui::GlyphId> {
        self.0.glyph_for_char(font_id, ch)
    }
    fn glyph_raster_bounds(
        &self,
        params: &gpui::RenderGlyphParams,
    ) -> gpui::Result<Bounds<DevicePixels>> {
        self.0.glyph_raster_bounds(params)
    }
    fn rasterize_glyph(
        &self,
        params: &gpui::RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> gpui::Result<(Size<DevicePixels>, Vec<u8>)> {
        self.0.rasterize_glyph(params, bounds)
    }
    fn layout_line(
        &self,
        text: &str,
        font_size: gpui::Pixels,
        runs: &[gpui::FontRun],
    ) -> gpui::LineLayout {
        self.0.layout_line(text, font_size, runs)
    }
    fn recommended_rendering_mode(
        &self,
        font_id: gpui::FontId,
        font_size: gpui::Pixels,
    ) -> gpui::TextRenderingMode {
        self.0.recommended_rendering_mode(font_id, font_size)
    }
}

/// `key`'s input, if the row shows one: what it holds, and why it wasn't
/// written, if it wasn't.
fn field(
    editor: &gpui::Entity<SettingsEditor>,
    key: &str,
    cx: &mut VisualTestContext,
) -> Option<(String, Option<String>)> {
    if !drawn(&format!("setting-input-{key}"), cx) {
        return None;
    }
    editor.read_with(cx, |editor, cx| editor.field(key, cx))
}

/// What `key`'s dropdown shows, and its hint, as it's drawn closed: from
/// no list, as neither a model nor a font not listed has a hint.
fn label(
    editor: &gpui::Entity<SettingsEditor>,
    key: &str,
    cx: &mut VisualTestContext,
) -> (String, Option<&'static str>) {
    editor.read_with(cx, |editor, cx| {
        let entry = editor.entry(key).unwrap();
        let (value, _) = editor.value(entry, cx).unwrap();
        controls::select_label(entry, &[], &value)
    })
}

/// Clears the focused input and types `text` into it.
fn retype(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input(text);
    draw(cx);
}

#[gpui::test]
fn model_dropdown_keeps_a_custom_id(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process_of(module_path!(), "model_dropdown_keeps_a_custom_id") else {
        return;
    };
    fs::write(dirs.user(), "[chat]\nmodel = \"my-proxy-model\"\n").unwrap();
    let (workbench, cx, _watcher) = workbench(cx, &dirs);
    let key = "chat.model";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    let select = format!("setting-select-{key}");
    let other = format!("setting-option-other-{key}");

    // The id as the file has it, no hint: a model newer than the app.
    assert_eq!(label(&editor, key, cx), ("my-proxy-model".to_owned(), None));
    assert!(!drawn(&format!("setting-hint-{key}"), cx));

    // The models known, default first, then Other…, highlighted for an id
    // the list hasn't.
    click(&select, cx);
    let (_, options, highlighted) = open_list(&editor, cx).unwrap();
    let known: Vec<Option<String>> = KNOWN.iter().map(|m| Some((*m).to_owned())).collect();
    assert_eq!(options, known);
    assert_eq!(highlighted, KNOWN.len(), "Other… is highlighted");
    let last = bounds(&option(key, KNOWN[KNOWN.len() - 1]), cx).top();
    assert!(bounds(&other, cx).top() > last, "Other… ends the list");
    press("escape", cx);

    // Other… swaps the row to an input, focused, holding the id.
    click(&select, cx);
    click(&other, cx);
    assert!(!drawn(&select, cx) && !drawn(&other, cx));
    assert_eq!(
        field(&editor, key, cx),
        Some(("my-proxy-model".into(), None))
    );
    retype("my-newer-model", cx);
    press("enter", cx);
    assert_eq!(read(&dirs.user()), "[chat]\nmodel = \"my-newer-model\"\n");

    // Leaving it brings the dropdown back, showing the id typed.
    click(&format!("settings-description-{key}"), cx);
    assert!(drawn(&select, cx), "the dropdown is back");
    assert_eq!(field(&editor, key, cx), None);
    assert_eq!(label(&editor, key, cx), ("my-newer-model".to_owned(), None));

    // From the keyboard: Enter opens it at Other…, Enter takes it. An
    // empty id isn't one: why shows, and the input stays.
    press("enter", cx);
    press("enter", cx);
    assert!(field(&editor, key, cx).is_some(), "Enter took Other…");
    retype("", cx);
    click(&format!("settings-description-{key}"), cx);
    let (text, message) = field(&editor, key, cx).expect("the input stays");
    assert_eq!(text, "");
    assert!(message.is_some(), "why shows");
    assert_eq!(read(&dirs.user()), "[chat]\nmodel = \"my-newer-model\"\n");

    // A known model, picked from the dropdown once it's back.
    press("enter", cx);
    retype("my-newer-model", cx);
    press("escape", cx);
    assert!(drawn(&select, cx), "the dropdown is back");
    click(&select, cx);
    click(&option(key, KNOWN[1]), cx);
    let picked = format!("[chat]\nmodel = \"{}\"\n", KNOWN[1]);
    assert_eq!(read(&dirs.user()), picked);
    assert_eq!(label(&editor, key, cx), (KNOWN[1].to_owned(), None));
}

/// Built by hand rather than by `#[gpui::test]`, to give the app a text
/// system with [`FONTS`].
#[test]
fn font_dropdown_lists_installed_fonts() {
    let Some(dirs) = in_own_process_of(module_path!(), "font_dropdown_lists_installed_fonts")
    else {
        return;
    };
    let listings = Arc::new(AtomicUsize::new(0));
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some("font_dropdown_lists_installed_fonts"),
        Arc::new(Fonts(NoopTextSystem::new(), listings.clone())),
    );
    fs::write(
        dirs.user(),
        "[editor]\n# Mine.\nfont_family = \"Iosevka\"\n",
    )
    .unwrap();
    let (workbench, cx, _watcher) = workbench(&mut cx, &dirs);
    let key = "editor.font_family";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    let listed = || listings.load(Ordering::SeqCst);

    // Closed, it shows the font and lists none, however often it's drawn.
    let before = listed();
    assert_eq!(label(&editor, key, cx), ("Iosevka".to_owned(), None));
    draw(cx);
    draw(cx);
    assert_eq!(listed(), before, "a closed dropdown lists no fonts");

    // Open: Default, then the fonts sorted, each once, its font
    // highlighted; listed once, not again as it's drawn.
    click(&format!("setting-select-{key}"), cx);
    assert_eq!(listed(), before + 1, "listed on opening");
    let (_, options, highlighted) = open_list(&editor, cx).unwrap();
    let expected: Vec<Option<String>> = [None]
        .into_iter()
        .chain(LISTED.iter().map(|font| Some((*font).to_owned())))
        .collect();
    assert_eq!(options, expected);
    assert_eq!(options[highlighted].as_deref(), Some("Iosevka"));
    let mut above = bounds(&format!("setting-option-default-{key}"), cx).top();
    for font in LISTED {
        let top = bounds(&option(key, font), cx).top();
        assert!(top > above, "{font} is listed in order");
        above = top;
    }
    assert!(
        !drawn(&format!("setting-option-other-{key}"), cx),
        "no Other…"
    );
    draw(cx);
    draw(cx);
    assert_eq!(listed(), before + 1, "not listed again while open");

    // Default resets the key, the comment kept.
    click(&format!("setting-option-default-{key}"), cx);
    assert_eq!(read(&dirs.user()), "[editor]\n# Mine.\n");
    assert_eq!(label(&editor, key, cx), (DEFAULT_CHOICE.to_owned(), None));

    // The terminal's lists them too, afresh, and Enter picks one.
    let key = "terminal.font_family";
    search_for(&editor, key, cx);
    click(&format!("settings-description-{key}"), cx);
    press("enter", cx);
    assert_eq!(listed(), before + 2, "listed again on opening again");
    assert_eq!(open_list(&editor, cx).unwrap().1, expected);
    press("down down enter", cx);
    assert_eq!(
        read(&dirs.user()),
        "[editor]\n# Mine.\n\n[terminal]\nfont_family = \"Iosevka\"\n"
    );
    assert_eq!(label(&editor, key, cx), ("Iosevka".to_owned(), None));

    cx.run_until_parked();
    cx.update(|_, cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
}
