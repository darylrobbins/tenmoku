//! Markdown's markup in its styles (T1924): strong text in the bold face,
//! emphasis in the italic, struck text with a line through it, each in its
//! capture's colour, the `**`, `*` and `~~` around them in the punctuation
//! colour, and every column where the plain face puts it. A language
//! server's deprecated symbol, a `Strikethrough` decoration, is struck
//! through in the colour it already has.
//!
//! On macOS the text is shaped with CoreText and the bundled fonts are
//! registered as Tenmoku registers them, so each face is really there and
//! the glyphs are told apart by font. Elsewhere GPUI's `NoopTextSystem` has
//! one font for every face, and only the styles and strikes are checked.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::{Buffer, BufferSnapshot};
use fe_config::EditorPalette;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint},
    diagnostics::{Diagnostic, DiagnosticTag, Severity},
};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, init};
use fe_syntax::Highlight;
use gpui::{Entity, Hsla, TestAppContext, TestDispatcher, VisualTestContext, px, size};

/// CoreText, which GPUI only builds with its Mac platform, and that only
/// on the main thread: built before `main`, as `styled_spans` does.
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

/// A window showing `text` as the file `name` in Yohen, Tenmoku's default
/// dark theme, drawn until its injected languages have loaded.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    name: &'static str,
    text: &'static str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let palette = EditorPalette::fastedit_dark().clone();
    let theme = cx.update(|cx| EditorTheme::from_palette(&palette, &EditorSettings::default(), cx));
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(text), Some(name.into()));
        EditorView::new(model, theme, cx)
    });
    cx.simulate_resize(size(px(800.), px(300.)));
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// The colour the theme gives the capture `name`.
fn colour(view: &Entity<EditorView>, cx: &mut VisualTestContext, name: &str) -> Hsla {
    let highlight = Highlight::from_capture(name).expect("in the vocabulary");
    let (color, _) = view.read_with(cx, |view, _| view.theme().capture_style(highlight));
    color.unwrap_or_else(|| panic!("{name} is coloured"))
}

const MARKDOWN: &str = "Some **bold**, *slanted* and ~~gone~~ words.\n";

#[test]
fn a_markdown_row_draws_bold_italic_and_struck_spans() {
    with_app("a_markdown_row", |cx| {
        let (view, cx) = editor(cx, "notes.md", MARKDOWN);
        let at = |word: &str| MARKDOWN.find(word).unwrap();
        let spans = view.read_with(cx, |view, _| view.styled_spans(0..MARKDOWN.len()));
        let style = |offset: usize| {
            (spans.iter())
                .find(|(range, _)| range.contains(&offset))
                .map(|(_, style)| *style)
                .unwrap_or_else(|| panic!("{offset} is styled in {spans:?}"))
        };
        let (bold, slanted, gone) = (style(at("bold")), style(at("slanted")), style(at("gone")));
        assert!(bold.bold && !bold.italic && !bold.strikethrough, "{bold:?}");
        assert!(slanted.italic && !slanted.bold && !slanted.strikethrough);
        assert!(gone.strikethrough && !gone.bold && !gone.italic);
        assert_eq!(bold.color, colour(&view, cx, "markup.strong"));
        assert_eq!(slanted.color, colour(&view, cx, "markup.italic"));
        assert_eq!(gone.color, colour(&view, cx, "markup.strikethrough"));
        // The delimiters are punctuation, in no face or line of their own.
        let punctuation = colour(&view, cx, "punctuation.delimiter");
        for delimiter in [at("**"), at("*slanted"), at("~~")] {
            let style = style(delimiter);
            assert_eq!(style.color, punctuation, "{delimiter}");
            assert!(!style.bold && !style.italic && !style.strikethrough);
        }

        // Drawn: the strike is on the struck word alone, and every glyph is
        // where the plain face puts it.
        let drawn = |view: &Entity<EditorView>, cx: &mut VisualTestContext, offset: usize| {
            view.read_with(cx, |view, _| view.drawn_strike(offset))
                .unwrap_or_else(|| panic!("{offset} is drawn"))
        };
        for (offset, ch) in MARKDOWN.trim_end().char_indices() {
            let (_, struck) = drawn(&view, cx, offset);
            let in_gone = (at("gone")..at("gone") + 4).contains(&offset);
            assert_eq!(struck, in_gone, "{ch:?} at {offset}");
        }
        let fonts = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
            ["Some", "bold", "slanted", "gone"]
                .map(|word| view.read_with(cx, |view, _| view.drawn_font_id(at(word))))
        };
        let [plain, bold_font, italic_font, gone_font] = fonts(&view, cx);
        assert!(plain.is_some());
        assert_eq!(gone_font, plain, "a strike is not a face");
        #[cfg(target_os = "macos")]
        {
            assert!(bold_font != plain && italic_font != plain && bold_font != italic_font);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (bold_font, italic_font);

        let xs = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
            (MARKDOWN.trim_end().char_indices())
                .map(|(offset, _)| drawn(view, cx, offset).0)
                .collect::<Vec<_>>()
        };
        let styled = xs(&view, cx);
        let (plain_view, cx) = editor(cx, "notes.txt", MARKDOWN);
        let plain = xs(&plain_view, cx);
        assert_eq!(styled.len(), plain.len());
        // The same columns, to the shaper's rounding.
        for (offset, (styled, plain)) in styled.iter().zip(&plain).enumerate() {
            assert!(
                (styled - plain).abs() < 0.01,
                "{offset}: {styled} vs {plain}"
            );
        }
    });
}

/// Markup nested in markup takes the faces of both, in the colour of the
/// innermost, in Markdown and in a doc comment alike; the delimiters
/// between them take none.
#[test]
fn nested_markup_draws_in_every_face_around_it() {
    with_app("nested_markup", |cx| {
        const NESTED: &str = "# Title *em*\n\n~~**gone**~~ and **a *b* c** and > no\n";
        let (view, cx) = editor(cx, "nested.md", NESTED);
        let spans = view.read_with(cx, |view, _| view.styled_spans(0..NESTED.len()));
        let style = |offset: usize| {
            (spans.iter())
                .find(|(range, _)| range.contains(&offset))
                .map(|(_, style)| *style)
                .unwrap_or_else(|| panic!("{offset} is styled in {spans:?}"))
        };
        let at = |word: &str| NESTED.find(word).unwrap();
        let title = style(at("Title"));
        assert!(title.bold && !title.italic, "{title:?}");
        let em = style(at("em*"));
        assert!(em.bold && em.italic && !em.strikethrough, "{em:?}");
        assert_eq!(em.color, colour(&view, cx, "markup.italic"));
        let gone = style(at("gone"));
        assert!(gone.bold && gone.strikethrough && !gone.italic, "{gone:?}");
        assert_eq!(gone.color, colour(&view, cx, "markup.strong"));
        let b = style(at("b*"));
        assert!(b.bold && b.italic, "{b:?}");
        let a = style(at("a *"));
        assert!(a.bold && !a.italic, "{a:?}");
        // The delimiters, inside markup and out, are the punctuation's.
        let punctuation = colour(&view, cx, "punctuation.delimiter");
        for delimiter in [at("~~"), at("**gone"), at("*b"), at("*em")] {
            let style = style(delimiter);
            assert_eq!(style.color, punctuation, "{delimiter}");
            assert!(!style.bold && !style.italic && !style.strikethrough);
        }
        // Drawn struck through: the struck text, not its delimiters.
        for offset in [at("gone"), at("gone") + 3] {
            let drawn = view.read_with(cx, |view, _| view.drawn_strike(offset));
            assert_eq!(drawn.map(|d| d.1), Some(true), "{offset}");
        }
        let drawn = view.read_with(cx, |view, _| view.drawn_strike(at("**gone")));
        assert_eq!(drawn.map(|d| d.1), Some(false));
    });
}

#[test]
fn nested_doc_comment_markup_draws_in_every_face_around_it() {
    with_app("nested_doc_markup", |cx| {
        const DOC: &str = "/// **a *b* c** and ~~**gone**~~\nfn a() {}\n";
        let (view, cx) = editor(cx, "lib.rs", DOC);
        let spans = view.read_with(cx, |view, _| view.styled_spans(0..DOC.len()));
        let style = |offset: usize| {
            (spans.iter())
                .find(|(range, _)| range.contains(&offset))
                .map(|(_, style)| *style)
                .unwrap_or_else(|| panic!("{offset} is styled in {spans:?}"))
        };
        let at = |word: &str| DOC.find(word).unwrap();
        let comment = colour(&view, cx, "comment.documentation");
        // The prose's face, which Yohen's doc comments have italic: the
        // markup's adds to it.
        let prose = style(at(" and "));
        assert!(!prose.bold && !prose.strikethrough, "{prose:?}");
        let b = style(at("b*"));
        assert!(b.bold && b.italic && !b.strikethrough, "{b:?}");
        let gone = style(at("gone"));
        assert!(gone.bold && gone.strikethrough, "{gone:?}");
        assert_eq!(gone.italic, prose.italic);
        for word in ["a *", "b*", "gone"] {
            assert_eq!(style(at(word)).color, comment, "{word}");
        }
        for delimiter in [at("**a"), at("*b"), at("~~")] {
            assert_eq!(style(delimiter), prose, "{delimiter}");
        }
    });
}

/// An escaped character is text in the markup around it, in its faces and
/// its own colour; the delimiters still take none.
#[test]
fn escapes_in_markup_take_its_faces() {
    with_app("escapes_in_markup", |cx| {
        const ESCAPES: &str = "~~a \\! b~~ and **x \\* y** and # no\n\n# Head \\# here\n";
        let (view, cx) = editor(cx, "escapes.md", ESCAPES);
        let spans = view.read_with(cx, |view, _| view.styled_spans(0..ESCAPES.len()));
        let style = |offset: usize| {
            (spans.iter())
                .find(|(range, _)| range.contains(&offset))
                .map(|(_, style)| *style)
                .unwrap_or_else(|| panic!("{offset} is styled in {spans:?}"))
        };
        let at = |word: &str| ESCAPES.find(word).unwrap();
        let escape = colour(&view, cx, "escape");
        let struck = style(at("\\!"));
        assert!(struck.strikethrough && struck.color == escape, "{struck:?}");
        let bold = style(at("\\*"));
        assert!(bold.bold && bold.color == escape, "{bold:?}");
        let heading = style(at("\\#"));
        assert!(heading.bold && heading.color == escape, "{heading:?}");
        for delimiter in [at("~~"), at("**")] {
            let style = style(delimiter);
            assert!(
                !style.bold && !style.strikethrough,
                "{delimiter}: {style:?}"
            );
        }
    });
}

/// A `Foreground` decoration over markup recolours it and keeps its face:
/// a language server's doc-comment token over bold text, and a TODO's
/// colour in struck text, as the workbench lays them.
#[test]
fn foreground_decorations_keep_markup_faces() {
    with_app("foreground_keeps_markup", |cx| {
        const DOC: &str = "/// A **bold** word and ~~TODO: remove this~~.\nfn a() {}\n";
        let (view, cx) = editor(cx, "lib.rs", DOC);
        let at = |word: &str| DOC.find(word).unwrap();
        let font = |cx: &mut VisualTestContext, offset: usize| {
            view.read_with(cx, |view, _| view.drawn_font_id(offset))
        };
        let (bold, todo) = (at("bold"), at("TODO"));
        let before = (font(cx, bold), font(cx, at(" word")));
        // Only CoreText tells the faces apart; elsewhere the styles below
        // still show the face is kept.
        #[cfg(target_os = "macos")]
        assert_ne!(before.0, before.1, "bold is drawn bold");
        let style = |cx: &mut VisualTestContext, offset: usize| {
            let spans = view.read_with(cx, |view, _| view.decorated_spans(0..DOC.len()));
            (spans.iter())
                .find(|(range, _)| range.contains(&offset))
                .map(|(_, style)| *style)
                .unwrap_or_else(|| panic!("{offset} is styled in {spans:?}"))
        };
        assert!(style(cx, bold).bold);
        assert!(style(cx, todo).strikethrough);
        let decorations = vec![
            Decoration::new(
                0..DOC.find('\n').unwrap(),
                DecorationKind::Foreground("comment.documentation".into()),
                Tint::Muted,
            ),
            Decoration::new(
                todo..todo + 5,
                DecorationKind::Foreground("comment.todo".into()),
                Tint::Muted,
            ),
        ];
        view.update(cx, |view, cx| {
            view.model_mut()
                .add_decoration_provider(Rc::new(RefCell::new(Fixed(decorations))));
            cx.notify();
        });
        draw(cx);
        let todo_colour = colour(&view, cx, "comment.todo");
        assert_eq!(
            view.read_with(cx, |view, _| view.drawn_color(todo)),
            Some(todo_colour)
        );
        for offset in todo..todo + 5 {
            let drawn = view.read_with(cx, |view, _| view.drawn_strike(offset));
            assert_eq!(drawn.map(|d| d.1), Some(true), "{offset}");
        }
        // Styled: the token's colour, markup's face.
        let token = colour(&view, cx, "comment.documentation");
        let (bold_style, todo_style) = (style(cx, bold), style(cx, todo));
        assert!(
            bold_style.bold && bold_style.color == token,
            "{bold_style:?}"
        );
        assert!(todo_style.strikethrough, "{todo_style:?}");
        assert_eq!(todo_style.color, todo_colour);
        // Drawn so.
        assert_eq!((font(cx, bold), font(cx, at(" word"))), before);
        let drawn = view.read_with(cx, |view, _| view.drawn_strike(at(" word")));
        assert_eq!(drawn.map(|d| d.1), Some(false));
    });
}

/// Decorations that don't change, as a language server's would be.
struct Fixed(Vec<Decoration>);

impl DecorationProvider for Fixed {
    fn decorations(&self, _: &BufferSnapshot, _: Range<usize>, out: &mut Vec<Decoration>) {
        out.extend(self.0.iter().cloned());
    }
}

const RUST: &str = "fn old() {}\nfn main() {\n    old();\n}\n";

#[test]
fn a_deprecated_symbol_is_struck_through_in_its_colour() {
    with_app("a_deprecated_symbol", |cx| {
        let (view, cx) = editor(cx, "main.rs", RUST);
        let call = RUST.rfind("old").unwrap();
        let color = |cx: &mut VisualTestContext, offset: usize| {
            view.read_with(cx, |view, _| view.drawn_color(offset))
        };
        let before = (
            color(cx, call),
            view.read_with(cx, |view, _| view.drawn_font_id(call)),
        );
        assert_eq!(
            view.read_with(cx, |view, _| view.drawn_strike(call))
                .map(|d| d.1),
            Some(false)
        );
        // As the workbench turns a `deprecated` function token: its colour,
        // and a line through it.
        let decorations = vec![
            Decoration::new(
                call..call + 3,
                DecorationKind::Foreground("function".into()),
                Tint::Muted,
            ),
            Decoration::new(call..call + 3, DecorationKind::Strikethrough, Tint::Muted),
        ];
        view.update(cx, |view, cx| {
            view.model_mut()
                .add_decoration_provider(Rc::new(RefCell::new(Fixed(decorations))));
            cx.notify();
        });
        draw(cx);
        for offset in call..call + 3 {
            let drawn = view.read_with(cx, |view, _| view.drawn_strike(offset));
            assert_eq!(drawn.map(|d| d.1), Some(true), "{offset}");
        }
        assert_eq!(color(cx, call), Some(colour(&view, cx, "function")));
        assert_eq!(color(cx, call), before.0);
        assert_eq!(
            view.read_with(cx, |view, _| view.drawn_font_id(call)),
            before.1
        );
        // The rest of the row, and the definition, aren't struck.
        let around = [
            call + 3,
            RUST.find("old").unwrap(),
            RUST.find("main").unwrap(),
        ];
        for offset in around {
            let drawn = view.read_with(cx, |view, _| view.drawn_strike(offset));
            assert_eq!(drawn.map(|d| d.1), Some(false), "{offset}");
        }
    });
}

/// A symbol a language server marks deprecated twice, by its semantic
/// token and by a diagnostic, over ranges that overlap, is struck through
/// once: one line from the first's start to the last's end.
#[test]
fn deprecated_twice_is_struck_once() {
    with_app("deprecated_twice", |cx| {
        let (view, cx) = editor(cx, "main.rs", RUST);
        let call = RUST.rfind("old").unwrap();
        let token = call..call + 3;
        let diagnosed = call..call + 5;
        let provider = view.update(cx, |view, cx| {
            let version = view.model().snapshot().version();
            let mut diagnostic = Diagnostic::new(Severity::Warning, "deprecated");
            diagnostic.tags.push(DiagnosticTag::Deprecated);
            let id = (1, 1);
            assert!(view.model_mut().apply_diagnostics(
                1u32,
                id,
                version,
                vec![(diagnosed.clone(), diagnostic)],
                Some(id),
            ));
            let decorations = vec![Decoration::new(
                token.clone(),
                DecorationKind::Strikethrough,
                Tint::Muted,
            )];
            let provider = view
                .model_mut()
                .add_decoration_provider(Rc::new(RefCell::new(Fixed(decorations))));
            cx.notify();
            provider
        });
        draw(cx);
        for offset in diagnosed.clone() {
            let drawn = view.read_with(cx, |view, _| view.drawn_strike(offset));
            assert_eq!(drawn.map(|d| d.1), Some(true), "{offset}");
        }
        let strikes = view.read_with(cx, |view, _| view.drawn_strikes());
        let [strike] = strikes[..] else {
            panic!("one strike, got {strikes:?}");
        };
        let x = |offset: usize| view.read_with(cx, |view, _| view.drawn_strike(offset).unwrap().0);
        let (start, end) = (x(diagnosed.start), x(diagnosed.end));
        assert!(
            (f32::from(strike.size.width) - (end - start)).abs() < 0.5,
            "{strike:?} for {start}..{end}"
        );
        // A diagnostic alone strikes the same way: the text's own line.
        view.update(cx, |view, cx| {
            assert!(view.model_mut().remove_decoration_provider(provider));
            cx.notify();
        });
        draw(cx);
        assert_eq!(view.read_with(cx, |view, _| view.drawn_strikes()).len(), 1);
    });
}

/// Paired brackets in their depth's colour keep the face and line the
/// theme gives their capture, in the text area as in sticky-scroll rows,
/// which share its spans.
#[test]
fn bracket_colours_keep_the_bracket_face() {
    with_app("bracket_colours_keep_the_face", |cx| {
        let mut palette = EditorPalette::fastedit_dark().clone();
        let bold_struck = fe_config::FontStyle::BOLD.union(fe_config::FontStyle::STRIKETHROUGH);
        palette.styles = vec![("punctuation.bracket".into(), bold_struck)];
        let mut theme =
            cx.update(|cx| EditorTheme::from_palette(&palette, &EditorSettings::default(), cx));
        theme.bracket_pair_colors = true;
        let text = "fn main() {}\n";
        let depth_0 = theme.bracket_color(0);
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let model = EditorModel::new(Buffer::new(text), Some("main.rs".into()));
            EditorView::new(model, theme, cx)
        });
        draw(cx);
        let spans = view.read_with(cx, |view, _| view.styled_spans(0..text.len()));
        for bracket in [text.find('(').unwrap(), text.find('{').unwrap()] {
            let (_, style) = (spans.iter())
                .find(|(range, _)| range.contains(&bracket))
                .unwrap_or_else(|| panic!("{bracket} is styled in {spans:?}"));
            assert_eq!(style.color, depth_0, "{bracket}");
            assert!(style.bold && style.strikethrough, "{bracket}: {style:?}");
        }
    });
}
