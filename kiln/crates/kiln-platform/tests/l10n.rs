// SPDX-License-Identifier: Apache-2.0

//! kiln-platform's catalogue: the literal lint (kiln-l10n), no hard-coded
//! user-facing text in this crate's non-test source outside
//! `l10n-allow.toml`, and every key it uses exists. Then the app's language
//! (`locale`): the system's preferred languages, the [`ActiveLocale`]
//! global [`set_locale`] keeps, and the warm-up; then, on Linux, what the
//! string catalogue's bidi isolates draw (i18n design § 5).
//!
//! The string catalogue is the process's, so every test that switches its
//! locale does so in `set_locale_follows_the_requested_languages` alone.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gpui::{App, Context, IntoElement, ParentElement as _, Render, TestAppContext, Window, div};
use kiln_l10n::{LanguageIdentifier, ResourceFile, Resources, tr};
use kiln_platform::locale::{
    ActiveLocale, active, negotiate, parse_tags, preferred, requested, set_locale, warm,
};

/// A probe message in en-US and German, registered as a crate's
/// `resources!` registers its files.
static PROBE: Resources = Resources {
    crate_name: "kiln-platform-locale-probe",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[
        ResourceFile {
            locale: "en-US",
            path: "probe.ftl",
            source: "platform-probe = Probe\n",
        },
        ResourceFile {
            locale: "de",
            path: "probe.ftl",
            source: "platform-probe = Sonde\n",
        },
    ],
    dynamic_prefixes: &[],
};

kiln_l10n::__private::inventory::submit! {
    kiln_l10n::Registered(&PROBE)
}

#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), &kiln_platform::l10n::RESOURCES).check();
}

fn ids(tags: &[&str]) -> Vec<LanguageIdentifier> {
    tags.iter().map(|tag| tag.parse().unwrap()).collect()
}

/// Draws the probe message, recording each text it drew.
struct Probe(Rc<RefCell<Vec<String>>>);

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let text = tr!("platform-probe");
        self.0.borrow_mut().push(text.to_string());
        div().child(text)
    }
}

#[test]
fn preferred_languages_parse_as_language_tags() {
    assert_eq!(
        parse_tags(["de-AT", "C", "POSIX", "not a tag", "fr", "de-AT", " en-GB "]),
        ids(&["de-AT", "fr", "en-GB"])
    );
    // Whatever the system has, only tags.
    let _: Vec<LanguageIdentifier> = preferred();

    let system = ids(&["de-AT", "fr"]);
    let xx: LanguageIdentifier = "xx".parse().unwrap();
    assert_eq!(requested(Some(&xx), &system), ids(&["xx", "de-AT", "fr"]));
    assert_eq!(requested(None, &system), system);
    let fr: LanguageIdentifier = "fr".parse().unwrap();
    assert_eq!(requested(Some(&fr), &system), ids(&["fr", "de-AT"]));
    // Against what the binary ships: en-US and the probe's German.
    assert_eq!(
        negotiate(&ids(&["xx", "de-AT", "fr"])),
        ids(&["de", "en-US"])
    );
    assert_eq!(negotiate(&ids(&["fr"])), ids(&["en-US"]));
}

#[gpui::test]
fn set_locale_follows_the_requested_languages(cx: &mut TestAppContext) {
    // The warm-up builds the catalogue for the system's languages off this
    // thread; setting those languages then switches nothing.
    warm();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while kiln_l10n::generation() == 0 {
        assert!(std::time::Instant::now() < deadline, "the warm-up ran");
        std::thread::sleep(Duration::from_millis(5));
    }
    let warmed = kiln_l10n::generation();
    let system = preferred();
    let chain = cx.update(|cx| set_locale(cx, &system));
    assert_eq!(kiln_l10n::generation(), warmed, "no second build");
    assert_eq!(chain, negotiate(&system));
    cx.update(|cx| assert_eq!(active(cx), chain));

    // Start from English, with a window drawing the probe.
    cx.update(|cx| set_locale(cx, &ids(&["fr"])));
    let drawn = Rc::new(RefCell::new(Vec::new()));
    let probe = drawn.clone();
    cx.add_window(move |_, _| Probe(probe));
    cx.run_until_parked();
    assert_eq!(drawn.borrow().last().map(String::as_str), Some("Probe"));
    let changes = Rc::new(RefCell::new(0));
    let counted = changes.clone();
    let _observer = cx.update(|cx| {
        cx.observe_global::<ActiveLocale>(move |_: &mut App| *counted.borrow_mut() += 1)
    });

    // A shipped language: the catalogue switches, the global follows and
    // the window redraws in it.
    let before = kiln_l10n::generation();
    let chain = cx.update(|cx| set_locale(cx, &ids(&["xx", "de-AT", "fr"])));
    assert_eq!(chain, ids(&["de", "en-US"]));
    assert!(kiln_l10n::generation() > before);
    assert_eq!(tr!("platform-probe"), "Sonde");
    cx.update(|cx| {
        let active = cx.global::<ActiveLocale>();
        assert_eq!(active.chain(), chain);
        assert_eq!(active.language().to_string(), "de");
        assert_eq!(active.generation(), kiln_l10n::generation());
    });
    cx.run_until_parked();
    assert_eq!(*changes.borrow(), 1);
    assert_eq!(drawn.borrow().last().map(String::as_str), Some("Sonde"));

    // Languages negotiating the same chain change nothing.
    let generation = kiln_l10n::generation();
    let redraws = drawn.borrow().len();
    cx.update(|cx| set_locale(cx, &ids(&["de"])));
    cx.run_until_parked();
    assert_eq!(kiln_l10n::generation(), generation);
    assert_eq!(*changes.borrow(), 1);
    assert_eq!(drawn.borrow().len(), redraws, "no redraw");

    // A switch made around this module is noticed, not taken as current.
    kiln_l10n::set_locales(&ids(&["fr"]));
    assert_eq!(tr!("platform-probe"), "Probe");
    cx.update(|cx| set_locale(cx, &ids(&["de"])));
    assert_eq!(tr!("platform-probe"), "Sonde");

    // Back to English.
    let chain = cx.update(|cx| set_locale(cx, &ids(&["fr"])));
    assert_eq!(chain, ids(&["en-US"]));
    cx.run_until_parked();
    assert_eq!(drawn.borrow().last().map(String::as_str), Some("Probe"));
    assert_eq!(*changes.borrow(), 3);
}

/// On Linux, GPUI lays text out with cosmic-text. The catalogue isolates an
/// argument with FSI (U+2068) and PDI (U+2069) where the locale is
/// right-to-left or the argument holds right-to-left text (§ 5), so the
/// marks must draw nothing: no width, and no ink, while the text between
/// them is still reordered.
#[cfg(target_os = "linux")]
#[test]
fn bidi_isolates_draw_nothing() {
    use gpui::{FontRun, Point, RenderGlyphParams, font, px};
    use kiln_l10n::{FSI, PDI};

    let platform = gpui_platform::current_platform(true);
    let text_system = platform.text_system();
    let font_id = [
        "DejaVu Sans",
        "Noto Sans",
        "Liberation Sans",
        "IBM Plex Sans",
    ]
    .into_iter()
    .find_map(|family| text_system.font_id(&font(family)).ok())
    .or_else(|| {
        let family = text_system.all_font_names().into_iter().next()?;
        text_system.font_id(&font(family)).ok()
    })
    .expect("a font is installed");
    let size = px(16.);
    let layout = |text: &str| {
        text_system.layout_line(
            text,
            size,
            &[FontRun {
                len: text.len(),
                font_id,
            }],
        )
    };

    // Whether `glyph` of the font `font_id` paints any pixel.
    let ink = |font_id, glyph: &gpui::ShapedGlyph| {
        let bounds = text_system
            .glyph_raster_bounds(&RenderGlyphParams {
                font_id,
                glyph_id: glyph.id,
                font_size: size,
                subpixel_variant: Point::default(),
                scale_factor: 2.,
                is_emoji: glyph.is_emoji,
                subpixel_rendering: false,
                dilation: 0,
            })
            .expect("bounds");
        bounds.size.width.0 > 0 && bounds.size.height.0 > 0
    };

    let plain = layout("abc");
    let ids: Vec<_> = plain
        .runs
        .iter()
        .flat_map(|run| &run.glyphs)
        .map(|g| g.id)
        .collect();
    assert!(
        ids[0] != ids[1] && plain.width > px(0.),
        "a real text system, not GPUI's no-op one: {ids:?}"
    );
    assert!(
        ink(plain.runs[0].font_id, &plain.runs[0].glyphs[0]),
        "a letter has ink"
    );

    for (bare, isolated) in [
        ("abc".to_owned(), format!("a{FSI}b{PDI}c")),
        ("aשלc".to_owned(), format!("a{FSI}של{PDI}c")),
        ("x 12 y".to_owned(), format!("x {FSI}12{PDI} y")),
    ] {
        let bare_layout = layout(&bare);
        let line = layout(&isolated);
        assert_eq!(line.width, bare_layout.width, "{isolated:?} is as wide");

        let marks: Vec<usize> = isolated
            .char_indices()
            .filter(|(_, ch)| [FSI, PDI].contains(ch))
            .map(|(at, _)| at)
            .collect();
        let mut seen = Vec::new();
        let mut visible = Vec::new();
        for run in &line.runs {
            for glyph in &run.glyphs {
                if !marks.contains(&glyph.index) {
                    visible.push((glyph.id, glyph.position.x));
                    continue;
                }
                seen.push(glyph.index);
                assert!(
                    !ink(run.font_id, glyph),
                    "the mark at {} in {isolated:?} has ink",
                    glyph.index
                );
            }
        }
        // The other glyphs are the bare text's, in the same places: the
        // marks take no room, and the bidi algorithm still reorders.
        let bare_glyphs: Vec<_> = (bare_layout.runs.iter())
            .flat_map(|run| &run.glyphs)
            .map(|glyph| (glyph.id, glyph.position.x))
            .collect();
        assert_eq!(visible, bare_glyphs, "{isolated:?}");
        assert!(seen.iter().all(|at| marks.contains(at)));
    }
    drop(platform);
}
