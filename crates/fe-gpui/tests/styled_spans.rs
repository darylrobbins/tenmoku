//! Italic and bold syntax spans (T1600): a row is shaped in one font run
//! while no span changes the face, else in one run per change of face, the
//! colours left to the decoration runs. Every face of the bundled code
//! family has the same advance, so the columns line up whatever the face.
//!
//! On macOS the text is shaped with CoreText and the bundled fonts are
//! registered as Tenmoku registers them, so each face is really there and
//! the glyphs are told apart by font. Elsewhere GPUI's `NoopTextSystem` has
//! one font for every face, and only the runs are checked.

use fe_buffer::Buffer;
use fe_config::{EditorPalette, FontStyle};
use fe_display::layout::StyleRun;
use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, Faces, SpanStyle, init, text_runs};
use gpui::{Font, FontWeight, Hsla, TestAppContext, TestDispatcher, TextRun, hsla, px, size};

/// CoreText, which GPUI only builds with its Mac platform, and that only
/// on the main thread: built before `main`, as `default_fonts` does.
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

const SIZE: f32 = 13.;

fn code() -> Font {
    gpui::font(fe_fonts::CODE)
}

fn faces(cx: &mut TestAppContext) -> Faces {
    cx.update(|cx| Faces::new(cx.text_system(), &code(), px(SIZE)))
}

const RED: Hsla = Hsla {
    h: 0.,
    s: 1.,
    l: 0.5,
    a: 1.,
};

fn run(len: usize, color: Hsla, italic: bool, bold: bool) -> StyleRun<SpanStyle> {
    StyleRun {
        len,
        style: SpanStyle {
            color,
            italic,
            bold,
        },
    }
}

/// Each text run's length, weight and whether it's italic.
fn summary(runs: &[TextRun]) -> Vec<(usize, FontWeight, bool)> {
    (runs.iter())
        .map(|run| {
            let italic = run.font.style == gpui::FontStyle::Italic;
            (run.len, run.font.weight, italic)
        })
        .collect()
}

#[test]
fn an_unstyled_row_shapes_as_one_run() {
    with_app("an_unstyled_row", |cx| {
        let faces = faces(cx);
        let text = "let answer = 42; // no faces";
        let blue = hsla(0.6, 1., 0.5, 1.);
        // Colours alone, however many, and an empty styled run, change no
        // face.
        let runs = [
            run(3, RED, false, false),
            run(10, blue, false, false),
            run(0, RED, true, true),
            run(text.len() - 13, RED, false, false),
        ];
        let mut out = Vec::new();
        text_runs(text, &runs, &faces, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len, text.len());
        assert_eq!(out[0].font, code());
        // No runs at all is the plain face too.
        text_runs("", &[], &faces, &mut out);
        assert_eq!(summary(&out), [(0, FontWeight::NORMAL, false)]);
    });
}

#[test]
fn a_styled_row_has_one_run_per_style_change() {
    with_app("a_styled_row", |cx| {
        let faces = faces(cx);
        let blue = hsla(0.6, 1., 0.5, 1.);
        // `fn f() {} // note` with `fn` red, `f` bold, a blue gap, and the
        // comment italic in two colours.
        let text = "fn f() {} // note";
        let runs = [
            run(2, RED, false, false),
            run(1, RED, false, false),
            run(1, blue, false, true),
            run(5, blue, false, false),
            run(3, blue, true, false),
            run(5, RED, true, false),
        ];
        let mut out = Vec::new();
        text_runs(text, &runs, &faces, &mut out);
        let semibold = FontWeight::SEMIBOLD;
        assert_eq!(
            summary(&out),
            [
                (3, FontWeight::NORMAL, false),
                (1, semibold, false),
                (5, FontWeight::NORMAL, false),
                (8, FontWeight::NORMAL, true),
            ]
        );
        // Spaces draw nothing and are as wide in every face: an indented
        // comment is one run, and spaces between faces take the one before.
        let comment = "    // note";
        let runs = [run(4, RED, false, false), run(7, RED, true, false)];
        text_runs(comment, &runs, &faces, &mut out);
        assert_eq!(summary(&out), [(11, FontWeight::NORMAL, true)]);
        let line = "x;  // c";
        let runs = [
            run(2, RED, false, false),
            run(2, blue, false, false),
            run(4, RED, true, false),
        ];
        text_runs(line, &runs, &faces, &mut out);
        let want = [
            (4, FontWeight::NORMAL, false),
            (4, FontWeight::NORMAL, true),
        ];
        assert_eq!(summary(&out), want);
        let blank = [run(3, RED, true, false)];
        text_runs("   ", &blank, &faces, &mut out);
        assert_eq!(summary(&out), [(3, FontWeight::NORMAL, false)]);
        // Bold italic is a face of its own.
        let runs = [
            run(2, RED, true, true),
            run(text.len() - 2, RED, true, false),
        ];
        text_runs(text, &runs, &faces, &mut out);
        assert_eq!(
            summary(&out),
            [
                (2, semibold, true),
                (text.len() - 2, FontWeight::NORMAL, true)
            ]
        );

        // Shaped, each glyph is in its run's face's font: one font run per
        // change of face.
        #[cfg(target_os = "macos")]
        cx.update(|cx| {
            let text_system = cx.text_system();
            let id = |italic, bold| {
                let style = SpanStyle {
                    color: RED,
                    italic,
                    bold,
                };
                text_system.resolve_font(faces.font(&style))
            };
            let (plain, bold, italic) = (id(false, false), id(false, true), id(true, false));
            assert!(plain != bold && plain != italic && bold != italic);
            let runs = [
                run(3, RED, false, false),
                run(1, RED, false, true),
                run(5, RED, false, false),
                run(8, RED, true, false),
            ];
            text_runs(text, &runs, &faces, &mut out);
            let shaper = gpui::WindowTextSystem::new(text_system.clone());
            let line = shaper.layout_line(text, px(SIZE), &out, None);
            let fonts: Vec<_> = (line.runs.iter()).map(|run| run.font_id).collect();
            assert_eq!(fonts, [plain, bold, plain, italic]);
        });
    });
}

#[test]
fn every_face_has_the_same_advance() {
    with_app("every_face_has_the_same_advance", |cx| {
        let faces = faces(cx);
        cx.update(|cx| {
            let text_system = cx.text_system();
            let styles = [(false, false), (true, false), (false, true), (true, true)];
            let fonts: Vec<Font> = (styles.iter())
                .map(|&(italic, bold)| {
                    let style = SpanStyle {
                        color: RED,
                        italic,
                        bold,
                    };
                    faces.font(&style).clone()
                })
                .collect();
            // Each face is the one asked for, none fallen back to the plain
            // one for an advance of its own.
            let asked: Vec<(FontWeight, bool)> = (fonts.iter())
                .map(|font| (font.weight, font.style == gpui::FontStyle::Italic))
                .collect();
            let semibold = FontWeight::SEMIBOLD;
            assert_eq!(
                asked,
                [
                    (FontWeight::NORMAL, false),
                    (FontWeight::NORMAL, true),
                    (semibold, false),
                    (semibold, true),
                ]
            );
            let ids: Vec<_> = fonts
                .iter()
                .map(|font| text_system.resolve_font(font))
                .collect();
            // The printable ASCII and the Latin-1 and Latin Extended-A
            // letters the faces have.
            let chars = (' '..='~').chain('À'..='ſ').filter(|&ch| {
                (ids.iter()).all(|&id| text_system.advance(id, px(SIZE), ch).is_ok())
            });
            #[cfg(target_os = "macos")]
            for (font, id) in fonts.iter().zip(&ids).skip(1) {
                let family = text_system.get_font_for_id(*id).expect("resolved").family;
                assert_eq!(family, fe_fonts::CODE, "{font:?} falls back");
                assert_ne!(*id, ids[0], "{font:?} is the plain face");
            }
            for ch in chars {
                let advance = |id| text_system.advance(id, px(SIZE), ch).expect("a glyph");
                let plain = advance(ids[0]);
                for (font, id) in fonts.iter().zip(&ids).skip(1) {
                    assert_eq!(advance(*id).width, plain.width, "{ch:?} in {font:?}");
                }
            }
        });
    });
}

#[test]
fn a_boundary_inside_a_grapheme_cluster_does_not_split_it() {
    with_app("a_boundary_inside_a_cluster", |cx| {
        let faces = faces(cx);
        // `e` and a combining acute are one cluster, and so is a family
        // emoji joined by ZWJs; each run ends inside one.
        let text = "ae\u{301}b👨\u{200d}👩\u{200d}👧c";
        let acute = text.find('\u{301}').unwrap();
        let joiner = text.find('\u{200d}').unwrap();
        let b = text.find('b').unwrap();
        let c = text.find('c').unwrap();
        let runs = [
            run(acute, RED, true, false),
            run(b - acute, RED, false, false),
            run(joiner - b, RED, false, true),
            run(text.len() - joiner, RED, false, false),
        ];
        let mut out = Vec::new();
        text_runs(text, &runs, &faces, &mut out);
        // The cluster each run ends in is drawn in that run's face, whole.
        let semibold = FontWeight::SEMIBOLD;
        assert_eq!(
            summary(&out),
            [
                (b, FontWeight::NORMAL, true),
                (c - b, semibold, false),
                (text.len() - c, FontWeight::NORMAL, false),
            ]
        );
        // A run of the acute alone, inside the cluster, takes no run of its
        // own, and the face around it carries on.
        let mark = '\u{301}'.len_utf8();
        let runs = [
            run(acute, RED, false, false),
            run(mark, RED, true, false),
            run(text.len() - acute - mark, RED, false, false),
        ];
        text_runs(text, &runs, &faces, &mut out);
        assert_eq!(summary(&out), [(text.len(), FontWeight::NORMAL, false)]);
        // Runs that don't add up to the text still cover it.
        text_runs(text, &[run(1, RED, true, false)], &faces, &mut out);
        assert_eq!(out.iter().map(|run| run.len).sum::<usize>(), text.len());
    });
}

/// The editor draws a comment in the italic face and the code around it in
/// the plain one, with the columns where the plain face puts them.
#[test]
fn the_editor_draws_comments_in_the_italic_face() {
    with_app("the_editor_draws_comments_in_italic", |cx| {
        let mut palette = EditorPalette::fastedit_dark().clone();
        let italic = FontStyle {
            italic: true,
            bold: false,
        };
        palette.styles = vec![("comment".into(), italic)];
        let text = "let x = 1; // one\nlet y = 2;\n";
        let theme =
            cx.update(|cx| EditorTheme::from_palette(&palette, &EditorSettings::default(), cx));
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let model = EditorModel::new(Buffer::new(text), Some("main.rs".into()));
            EditorView::new(model, theme, cx)
        });
        cx.simulate_resize(size(px(600.), px(300.)));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(view.read_with(cx, |view, _| view.syntax().is_some()));
        let drawn = |offset: usize| view.read_with(cx, |view, _| view.drawn_font_id(offset));
        let (code_at, comment_at) = (text.find('x').unwrap(), text.find("one").unwrap());
        let (plain, comment) = (drawn(code_at), drawn(comment_at));
        assert!(plain.is_some() && comment.is_some());
        let below = drawn(text.find('y').unwrap());
        assert_eq!(below, plain);
        // The editor's font, with its features, and its italic.
        #[cfg(target_os = "macos")]
        {
            let font = view.read_with(cx, |view, _| view.theme().font());
            assert_eq!(font.family, fe_fonts::CODE);
            let italic_font = Font {
                style: gpui::FontStyle::Italic,
                ..font.clone()
            };
            let mut id = |font: &Font| cx.update(|_, cx| cx.text_system().resolve_font(font));
            assert_eq!(plain, Some(id(&font)));
            assert_eq!(comment, Some(id(&italic_font)));
        }
    });
}
