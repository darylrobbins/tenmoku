//! The editor's default font and row height (T1515): with no
//! `editor.font_family`, the editor is set in Spline Sans Mono, the bundled
//! code family, and a 13 px row is 20 px, the design's Code role
//! (`docs/design/design-system.md#type-spacing-and-sizes`).
//!
//! On macOS the text is shaped with CoreText and the bundled fonts are
//! registered as Tenmoku registers them, so a family resolves only if it's
//! really there. Elsewhere GPUI's `NoopTextSystem` takes any family as
//! installed.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, init};
use gpui::{Bounds, EntityInputHandler as _, TestAppContext, TestDispatcher, px, size};

/// CoreText, which GPUI only builds with its Mac platform, and that only
/// on the main thread: built before `main`, as `non_latin_text` does.
#[cfg(target_os = "macos")]
mod shaped {
    use std::sync::{Arc, OnceLock};

    use gpui::PlatformTextSystem;

    pub(super) static TEXT_SYSTEM: OnceLock<Arc<dyn PlatformTextSystem>> = OnceLock::new();

    extern "C" fn build_text_system() {
        let platform = gpui_platform::current_platform(true);
        TEXT_SYSTEM.get_or_init(|| platform.text_system());
        // Kept: its text system holds on to what the platform made.
        std::mem::forget(platform);
    }

    #[used]
    #[unsafe(link_section = "__DATA,__mod_init_func")]
    static BUILD_TEXT_SYSTEM: extern "C" fn() = build_text_system;
}

/// Runs `test` in an app with the bundled fonts registered, and shuts it
/// down.
fn with_app(name: &'static str, test: impl FnOnce(&mut TestAppContext)) {
    #[cfg(target_os = "macos")]
    let mut cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some(name),
        shaped::TEXT_SYSTEM
            .get()
            .expect("built before main")
            .clone(),
    );
    #[cfg(not(target_os = "macos"))]
    let mut cx = TestAppContext::build(TestDispatcher::new(0), Some(name));
    cx.update(|cx| {
        cx.text_system()
            .add_fonts(fe_fonts::all())
            .expect("the bundled fonts register");
        init(cx);
    });
    test(&mut cx);
    cx.run_until_parked();
    cx.update(|cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
}

#[test]
fn default_code_font_is_spline_sans_mono_with_20px_rows() {
    with_app("default_code_font", |cx| {
        let settings = EditorSettings::default();
        assert_eq!(settings.font_family, None);
        assert_eq!(settings.font_size, 13.);
        let theme = cx.update(|cx| EditorTheme::from_settings(&settings, cx));
        assert_eq!(theme.font_family, fe_fonts::CODE);
        assert_eq!(theme.font_size, px(13.));
        // The family resolves to itself, not to a fallback under another name.
        let resolved = cx.update(|cx| {
            let text_system = cx.text_system();
            let id = text_system.resolve_font(&theme.font());
            text_system.get_font_for_id(id).expect("resolved").family
        });
        assert_eq!(resolved, fe_fonts::CODE);

        // Drawn, row 1 starts 20 px below row 0, and a row is 20 px tall.
        let text = "fn main() {\n    let x = 1;\n}\n";
        let (view, cx) = cx.add_window_view(move |_, cx| {
            EditorView::new(EditorModel::new(Buffer::new(text), None), theme, cx)
        });
        cx.simulate_resize(size(px(600.), px(300.)));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let mut row_bounds = |offset: usize| {
            cx.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.bounds_for_range(offset..offset, Bounds::default(), window, cx)
                })
            })
            .expect("the row is drawn")
        };
        let (first, second) = (row_bounds(0), row_bounds(text.find("let").unwrap()));
        assert_eq!(second.top() - first.top(), px(20.));
        assert_eq!(first.size.height, px(20.));
    });
}

/// Spline Sans Mono has no CJK or emoji, so CoreText takes them from
/// fonts that do: the editor's non-Latin text still shows
/// (`non_latin_text`).
#[cfg(target_os = "macos")]
#[test]
fn cjk_and_emoji_fall_back_from_spline_sans_mono() {
    with_app("cjk_and_emoji_fall_back", |cx| {
        cx.update(|cx| {
            let code = cx.text_system().resolve_font(&gpui::font(fe_fonts::CODE));
            let shaper = gpui::WindowTextSystem::new(cx.text_system().clone());
            for text in ["中文", "😀"] {
                let run = gpui::TextRun {
                    len: text.len(),
                    font: gpui::font(fe_fonts::CODE),
                    color: gpui::black(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = shaper.layout_line(text, px(13.), &[run], None);
                let fonts: Vec<_> = (line.runs.iter()).map(|run| run.font_id).collect();
                assert!(!fonts.is_empty(), "{text}: no glyphs");
                assert!(
                    fonts.iter().all(|id| *id != code),
                    "{text} is set in Spline Sans Mono, which has none of it"
                );
            }
        });
    });
}
