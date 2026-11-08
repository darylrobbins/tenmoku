//! After `fe_workbench::init`, the text system resolves each bundled family
//! at the weights fe-fonts ships to the bundled face itself, not a system
//! fallback.
//!
//! `#[gpui::test]` lays text out with GPUI's `NoopTextSystem`, which
//! resolves every font to one id, so this test uses macOS's CoreText text
//! system instead and runs on macOS only (see `fe-gpui`'s
//! `tests/non_latin_text.rs`, which this follows).

#![cfg(target_os = "macos")]

use std::sync::{Arc, OnceLock};

use fe_fonts::{CODE, FACES, TITLE, UI};
use gpui::{
    Font, FontId, FontStyle, FontWeight, PlatformTextSystem, TestAppContext, TestDispatcher, font,
    px,
};

static TEXT_SYSTEM: OnceLock<Arc<dyn PlatformTextSystem>> = OnceLock::new();

/// GPUI builds its Mac platform, and with it the CoreText text system, only
/// on the main thread; libtest runs tests on threads of their own, so it is
/// built before `main`, from the binary's initialiser list, and kept.
extern "C" fn build_text_system() {
    let platform = gpui_platform::current_platform(true);
    TEXT_SYSTEM.get_or_init(|| platform.text_system());
    // Kept: its text system holds on to what the platform made.
    std::mem::forget(platform);
}

#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static BUILD_TEXT_SYSTEM: extern "C" fn() = build_text_system;

/// The characters whose glyphs identify a face. Spline Sans Mono's weights
/// share every advance, so a face is told by its glyphs' outline bounds too,
/// which grow with the stems.
const PROBE: &str = "Hamburgefonstiv 0123456789";

fn styled(family: &str, weight: u16, italic: bool) -> Font {
    Font {
        weight: FontWeight(f32::from(weight)),
        style: if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..font(family.to_string())
    }
}

/// A probe character's glyph id, advance and outline bounds (x min, y min,
/// x max, y max), in font units.
#[derive(Debug)]
struct Glyph {
    id: u32,
    advance: i32,
    bounds: Option<[i32; 4]>,
}

/// Whether two faces' probe glyphs are the same, allowing a unit of
/// rounding in the bounds (CoreText measures the outline, the `glyf` table
/// stores its box).
fn same(a: &[Glyph], b: &[Glyph]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.id == b.id
                && a.advance == b.advance
                && match (a.bounds, b.bounds) {
                    (Some(a), Some(b)) => a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1),
                    (a, b) => a == b,
                }
        })
}

/// The probe glyphs of a bundled file.
fn expected(bytes: &[u8]) -> Vec<Glyph> {
    let face = ttf_parser::Face::parse(bytes, 0).expect("a bundled font parses");
    PROBE
        .chars()
        .map(|ch| {
            let glyph = face.glyph_index(ch).expect("a probe character's glyph");
            Glyph {
                id: u32::from(glyph.0),
                advance: i32::from(face.glyph_hor_advance(glyph).expect("an advance")),
                bounds: face
                    .glyph_bounding_box(glyph)
                    .map(|r| [r.x_min, r.y_min, r.x_max, r.y_max].map(i32::from)),
            }
        })
        .collect()
}

/// The same, as the text system has `font_id`.
fn resolved(text_system: &dyn PlatformTextSystem, font_id: FontId) -> Vec<Glyph> {
    PROBE
        .chars()
        .map(|ch| {
            let glyph = text_system
                .glyph_for_char(font_id, ch)
                .expect("a probe character's glyph");
            let advance = text_system.advance(font_id, glyph).expect("an advance");
            let b = text_system
                .typographic_bounds(font_id, glyph)
                .expect("glyph bounds");
            Glyph {
                id: glyph.0,
                advance: advance.width.round() as i32,
                // A space has no outline: zero size, where ttf-parser has none.
                bounds: (b.size.width > 0. || b.size.height > 0.).then(|| {
                    [
                        b.origin.x,
                        b.origin.y,
                        b.origin.x + b.size.width,
                        b.origin.y + b.size.height,
                    ]
                    .map(|v| v.round() as i32)
                }),
            }
        })
        .collect()
}

/// Which bundled file `font_id` is, if any.
fn which(text_system: &dyn PlatformTextSystem, font_id: FontId) -> Option<&'static str> {
    let glyphs = resolved(text_system, font_id);
    FACES
        .iter()
        .find(|f| same(&glyphs, &expected(f.bytes)))
        .map(|f| f.file)
}

#[test]
fn bundled_faces_have_distinct_probe_glyphs() {
    // Else the resolution test below couldn't tell a wrong weight from the
    // right one.
    for (i, a) in FACES.iter().enumerate() {
        for b in &FACES[i + 1..] {
            assert!(
                !same(&expected(a.bytes), &expected(b.bytes)),
                "{} and {} have the same probe glyphs",
                a.file,
                b.file
            );
        }
    }
}

#[test]
fn bundled_families_resolve_at_each_weight() {
    let platform = TEXT_SYSTEM.get().expect("built before main").clone();
    let cx = TestAppContext::build_with_text_system(
        TestDispatcher::new(0),
        Some("bundled_families_resolve_at_each_weight"),
        platform.clone(),
    );
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });

    let wanted: [(&str, &[u16]); 3] = [
        (UI, &[400, 500, 600]),
        (CODE, &[400, 500, 600]),
        (TITLE, &[400]),
    ];
    let mut failures = Vec::new();
    for (family, weights) in wanted {
        for &weight in weights {
            let face = FACES
                .iter()
                .find(|f| f.family == family && f.weight == weight && !f.italic)
                .unwrap_or_else(|| panic!("fe-fonts ships no {family} {weight}"));
            // Not `TextSystem::resolve_font`, which falls back silently.
            match platform.font_id(&styled(family, weight, false)) {
                Ok(id) if same(&resolved(&*platform, id), &expected(face.bytes)) => {}
                Ok(id) => failures.push(format!(
                    "{family} {weight}: resolved to {:?}, not {}",
                    which(&*platform, id).unwrap_or("a face fe-fonts doesn't ship"),
                    face.file
                )),
                Err(e) => failures.push(format!("{family} {weight}: not resolved: {e:#}")),
            }
        }
    }
    // Spline Sans Mono's italic too.
    let italic = FACES.iter().find(|f| f.italic).expect("an italic");
    match platform.font_id(&styled(italic.family, italic.weight, true)) {
        Ok(id) if same(&resolved(&*platform, id), &expected(italic.bytes)) => {}
        Ok(id) => failures.push(format!(
            "{}: resolved to {:?}",
            italic.file,
            which(&*platform, id).unwrap_or("a face fe-fonts doesn't ship")
        )),
        Err(e) => failures.push(format!("{}: not resolved: {e:#}", italic.file)),
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));

    // Through the app's text system, the one `init` registered them with,
    // Instrument Sans's three weights set the same string at three widths.
    let widths: Vec<f32> = cx.update(|cx| {
        let text_system = cx.text_system();
        [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::SEMIBOLD]
            .into_iter()
            .map(|weight| {
                let id = text_system.resolve_font(&Font { weight, ..font(UI) });
                PROBE
                    .chars()
                    .map(|ch| f32::from(text_system.advance(id, px(100.), ch).unwrap().width))
                    .sum()
            })
            .collect()
    });
    assert!(
        widths[0] != widths[1] && widths[1] != widths[2] && widths[0] != widths[2],
        "Instrument Sans 400, 500 and 600 set {PROBE:?} at {widths:?}"
    );

    cx.run_until_parked();
    cx.update(|cx| {
        cx.background_executor().forbid_parking();
        cx.quit();
    });
    cx.run_until_parked();
}
