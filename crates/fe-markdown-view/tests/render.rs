//! A Markdown view in a test window: images, link clicks and theme colours.

use std::{
    cell::RefCell,
    path::Path,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
use fe_markdown::BlockKind;
use fe_markdown_view::{MarkdownStyle, MarkdownView, MarkdownViewEvent};
use fe_syntax::Highlight;
use gpui::{
    Bounds, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext, http_client, point, px,
};
use kiln_ui::{Theme, UiTheme};

fn dark_style(cx: &mut TestAppContext) -> MarkdownStyle {
    cx.update(|cx| {
        let editor = EditorTheme::from_palette(
            EditorPalette::fastedit_dark(),
            &EditorSettings::default(),
            cx,
        );
        MarkdownStyle::new(&editor, &UiTheme::default_dark())
    })
}

/// A view of `markdown`, the file at `doc`, in a test window, drawn once
/// whatever it loads has landed.
fn open<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
    doc: &Path,
    root: Option<&Path>,
) -> (Entity<MarkdownView>, &'a mut VisualTestContext) {
    let style = dark_style(cx);
    let document = Arc::new(fe_markdown::parse(markdown));
    let (doc, root) = (doc.to_owned(), root.map(Path::to_owned));
    let (view, cx) =
        cx.add_window_view(move |_, cx| MarkdownView::new(document, doc, root, style, cx));
    settle(cx);
    (view, cx)
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn bounds(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(selector.leak())
}

#[gpui::test]
fn images_load_locally_and_remote_ones_are_blocked(cx: &mut TestAppContext) {
    let fetches = Arc::new(AtomicUsize::new(0));
    let counted = fetches.clone();
    cx.update(|cx| {
        cx.set_http_client(http_client::FakeHttpClient::create(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            async {
                Ok(http_client::Response::builder()
                    .status(404)
                    .body(Default::default())
                    .unwrap())
            }
        }))
    });
    let dir = tempfile::tempdir().unwrap();
    let docs = dir.path().join("docs");
    std::fs::create_dir(&docs).unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="red"/></svg>"#;
    std::fs::write(docs.join("pic.svg"), svg).unwrap();
    // Exists, but outside the workspace and the document's folder.
    std::fs::write(dir.path().join("outside.svg"), svg).unwrap();
    // SVGs in the folder, drawing a file in it and one outside it.
    let linking = |target: &Path| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><image href="{}" width="40" height="30"/></svg>"#,
            target.display()
        )
    };
    std::fs::write(docs.join("links-in.svg"), linking(&docs.join("pic.svg"))).unwrap();
    let outside = dir.path().join("outside.svg");
    std::fs::write(docs.join("links-out.svg"), linking(&outside)).unwrap();
    // A filter drawing the outside file, on a file inside and on text: it's
    // drawn only once their size is known.
    let filtered = |content: String| {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="30"><filter id="f"><feImage xlink:href="{}"/></filter><g filter="url(#f)">{content}</g></svg>"#,
            outside.display()
        )
    };
    let image_in = format!(
        r#"<image href="{}" width="40" height="30"/>"#,
        docs.join("pic.svg").display()
    );
    std::fs::write(docs.join("filter-image.svg"), filtered(image_in)).unwrap();
    let text = r#"<text x="0" y="20" font-family="Helvetica">Hi</text>"#;
    std::fs::write(docs.join("filter-text.svg"), filtered(text.into())).unwrap();
    let markdown = "![local](pic.svg)\n\n\
                    ![a remote picture](https://example.com/pic.png)\n\n\
                    ![up](../outside.svg)\n\n\
                    ![in](links-in.svg)\n\n\
                    ![out](links-out.svg)\n\n\
                    ![filtered image](filter-image.svg)\n\n\
                    ![filtered text](filter-text.svg)\n";
    let (_view, cx) = open(cx, markdown, &docs.join("doc.md"), Some(&docs));
    assert_eq!(fetches.load(Ordering::SeqCst), 0, "nothing is fetched");

    let local = bounds(cx, "md-image-0".into()).expect("the local image is drawn");
    assert_eq!(
        (local.size.width, local.size.height),
        (px(40.), px(30.)),
        "drawn at the file's size, so it loaded"
    );
    for n in [1, 2] {
        assert_eq!(
            bounds(cx, format!("md-image-{n}")),
            None,
            "image {n} isn't drawn"
        );
        let placeholder = bounds(cx, format!("md-image-blocked-{n}"))
            .unwrap_or_else(|| panic!("image {n} has a placeholder"));
        assert!(placeholder.size.width > px(0.) && placeholder.size.height > px(0.));
    }
    assert_eq!(bounds(cx, "md-image-blocked-0".into()), None);
    // Each placeholder shows its own alt text: the longer one is wider.
    let remote_alt = bounds(cx, "md-image-blocked-1-alt".into()).expect("alt text");
    let outside_alt = bounds(cx, "md-image-blocked-2-alt".into()).expect("alt text");
    assert!(remote_alt.size.width > outside_alt.size.width * 4.);

    let links_in = bounds(cx, "md-image-3".into()).expect("an SVG drawing a file inside");
    assert_eq!(
        (links_in.size.width, links_in.size.height),
        (px(40.), px(30.))
    );
    assert_eq!(bounds(cx, "md-image-blocked-3".into()), None);
    assert_eq!(
        bounds(cx, "md-image-4".into()),
        None,
        "an SVG drawing a file outside isn't drawn"
    );
    assert!(bounds(cx, "md-image-blocked-4".into()).is_some());
    for n in [5, 6] {
        assert_eq!(
            bounds(cx, format!("md-image-{n}")),
            None,
            "filter {n} isn't drawn"
        );
        assert!(bounds(cx, format!("md-image-blocked-{n}")).is_some());
    }
}

#[gpui::test]
fn local_images_show_their_alt_text_until_drawn(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"/>"#;
    std::fs::write(dir.path().join("pic.svg"), svg).unwrap();
    std::fs::write(dir.path().join("broken.png"), "not an image").unwrap();
    let (view, cx) = open(cx, "", &dir.path().join("doc.md"), None);
    // Loads run only when the test runs them, so they're pending until
    // `settle`.
    let document = Arc::new(fe_markdown::parse(
        "![the picture](pic.svg)\n\n![a broken one](broken.png)\n",
    ));
    view.update(cx, |view, cx| view.set_document(document, cx));
    let draw = |cx: &mut VisualTestContext| cx.update(|window, cx| window.draw(cx).clear(cx));
    draw(cx);
    assert_eq!(
        bounds(cx, "md-image-0-loading".into()),
        None,
        "not at first"
    );
    std::thread::sleep(gpui::LOADING_DELAY + Duration::from_millis(50));
    draw(cx);
    for n in [0, 1] {
        assert!(
            bounds(cx, format!("md-image-{n}-loading")).is_some(),
            "image {n} loading"
        );
    }

    settle(cx);
    let picture = bounds(cx, "md-image-0".into()).expect("the picture");
    assert_eq!(
        (picture.size.width, picture.size.height),
        (px(40.), px(30.))
    );
    assert!(bounds(cx, "md-image-1-failed".into()).is_some());
    for n in [0, 1] {
        assert_eq!(bounds(cx, format!("md-image-{n}-loading")), None);
    }
    assert_eq!(bounds(cx, "md-image-0-failed".into()), None);
}

#[gpui::test]
fn images_flow_with_the_text_around_them(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"/>"#;
    std::fs::write(dir.path().join("pic.svg"), svg).unwrap();
    let markdown = "before ![a](pic.svg) <span\ntitle=\"x\">after</span>\n\n\
                    ![b](pic.svg)\n![c](pic.svg)\n\n\
                    ![d](pic.svg)\\\n![e](pic.svg)\n\n\
                    | | | text |\n|---|---|---|\n| ![f](pic.svg) | ![a remote one](https://example.com/g.png) | x |\n";
    let (_view, cx) = open(cx, markdown, &dir.path().join("doc.md"), None);
    let image = |cx: &mut VisualTestContext, n: usize| {
        bounds(cx, format!("md-image-{n}")).unwrap_or_else(|| panic!("image {n}"))
    };

    // On one line with its text, not between two.
    let mixed = bounds(cx, "md-block-0".into()).unwrap();
    let a = image(cx, 0);
    assert!(
        a.origin.x > mixed.origin.x + px(20.),
        "text before it on its line"
    );
    assert_eq!(mixed.size.height, a.size.height, "one line");
    // A soft break is a space, and a hard break a new line.
    let (b, c) = (image(cx, 1), image(cx, 2));
    assert_eq!(b.origin.y, c.origin.y);
    assert!(c.origin.x > b.right());
    let (d, e) = (image(cx, 3), image(cx, 4));
    assert!(e.origin.y >= d.bottom());
    assert_eq!(e.origin.x, d.origin.x);

    // A column of an image, or of a placeholder, is as wide as it is.
    let f = image(cx, 5);
    assert_eq!((f.size.width, f.size.height), (px(40.), px(30.)));
    let placeholder = bounds(cx, "md-image-blocked-6".into()).unwrap();
    // The SVG's column is its drawn width and the cells' padding: GPUI
    // rasterizes an SVG at twice its size.
    assert!(
        placeholder.origin.x - f.right() < px(30.),
        "{f:?} {placeholder:?}"
    );
    let alt = bounds(cx, "md-image-blocked-6-alt".into()).unwrap();
    assert!(alt.right() <= placeholder.right());
    let line = px(14. * 1.6);
    assert!(alt.size.height < line * 1.5, "the alt text isn't wrapped");
    assert!(placeholder.size.height < line * 3., "nor is the reason");
}

#[gpui::test]
fn link_clicks_emit_their_destination(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"/>"#;
    std::fs::write(dir.path().join("icon.svg"), svg).unwrap();
    let markdown = "[docs](./guide%20one.md#L3)\n\nplain text here\n\n- [ ] todo\n\n\
                    [before ![icon](icon.svg) after](two.md)\n\n\
                    [![only](icon.svg)](three.md)\n";
    let (view, cx) = open(cx, markdown, &dir.path().join("doc.md"), None);
    let events = Rc::new(RefCell::new(Vec::new()));
    let seen = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &MarkdownViewEvent, _| {
            seen.borrow_mut().push(event.clone())
        })
        .detach()
    });
    let click = |cx: &mut VisualTestContext, x: Pixels, y: Pixels| {
        cx.simulate_click(point(x, y), Modifiers::none());
        settle(cx);
    };

    let link = bounds(cx, "md-block-0".into()).expect("the link's paragraph");
    let line = link.origin.y + px(8.);
    // Past the end of the link's text, in its paragraph.
    click(cx, link.right() - px(4.), line);
    assert_eq!(*events.borrow(), []);
    click(cx, link.origin.x + px(4.), line);
    assert_eq!(
        *events.borrow(),
        [MarkdownViewEvent::LinkClicked {
            dest: "./guide%20one.md#L3".into()
        }]
    );
    events.borrow_mut().clear();

    let plain = bounds(cx, "md-block-1".into()).expect("the plain paragraph");
    click(cx, plain.origin.x + px(4.), plain.origin.y + px(8.));
    let checkbox = bounds(cx, "md-task-0".into()).expect("the task's checkbox");
    click(cx, checkbox.center().x, checkbox.center().y);
    assert_eq!(*events.borrow(), []);
    assert_eq!(
        bounds(cx, "md-task-0-check".into()),
        None,
        "still unchecked"
    );
    view.read_with(cx, |view, _| {
        let BlockKind::List { items, .. } = &view.document().blocks[2].kind else {
            panic!("a list");
        };
        assert_eq!(items[0].task, Some(false));
    });

    // A link's text before an image in it, the image, and its text after.
    let around = bounds(cx, "md-block-3".into()).expect("the link around an image");
    let icon = bounds(cx, "md-image-0".into()).expect("the icon");
    for (x, y) in [
        (around.origin.x + px(4.), around.bottom() - px(8.)),
        (icon.center().x, icon.center().y),
        (icon.right() + px(2.), around.bottom() - px(8.)),
    ] {
        click(cx, x, y);
    }
    let only = bounds(cx, "md-image-1".into()).expect("the linked image");
    click(cx, only.center().x, only.center().y);
    let link = |dest: &str| MarkdownViewEvent::LinkClicked { dest: dest.into() };
    assert_eq!(
        *events.borrow(),
        [
            link("two.md"),
            link("two.md"),
            link("two.md"),
            link("three.md")
        ]
    );
}

#[gpui::test]
fn the_hovered_link_follows_the_pointer(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"/>"#;
    std::fs::write(dir.path().join("icon.svg"), svg).unwrap();
    let markdown =
        "[docs](one.md)\n\nplain text here\n\n[before ![icon](icon.svg) after](two.md)\n";
    let (view, cx) = open(cx, markdown, &dir.path().join("doc.md"), None);
    let hovered = |cx: &mut VisualTestContext, x: Pixels, y: Pixels| {
        cx.simulate_mouse_move(point(x, y), None, Modifiers::none());
        settle(cx);
        view.read_with(cx, |view, _| view.hovered_link().map(str::to_owned))
    };
    let link = bounds(cx, "md-block-0".into()).unwrap();
    let plain = bounds(cx, "md-block-1".into()).unwrap();
    let on_link = (link.origin.x + px(4.), link.origin.y + px(8.));
    let on_plain = (plain.origin.x + px(4.), plain.origin.y + px(8.));
    assert_eq!(hovered(cx, on_link.0, on_link.1).as_deref(), Some("one.md"));
    assert_eq!(hovered(cx, on_plain.0, on_plain.1), None);
    // Back to the char it left from.
    assert_eq!(hovered(cx, on_link.0, on_link.1).as_deref(), Some("one.md"));
    assert_eq!(
        hovered(cx, link.right() - px(4.), on_link.1),
        None,
        "past the text"
    );

    // From the link's text before the image to its text after it.
    let around = bounds(cx, "md-block-2".into()).unwrap();
    let icon = bounds(cx, "md-image-0".into()).unwrap();
    let y = around.bottom() - px(8.);
    assert_eq!(
        hovered(cx, around.origin.x + px(4.), y).as_deref(),
        Some("two.md")
    );
    assert_eq!(
        hovered(cx, icon.right() + px(2.), y).as_deref(),
        Some("two.md")
    );
}

#[gpui::test]
fn style_follows_the_editor_theme(cx: &mut TestAppContext) {
    let keyword = Highlight::from_capture("keyword").expect("a highlight");
    let fence = fe_markdown::parse("```rust\npub struct Point;\n```\n");
    let BlockKind::Code {
        code, highlights, ..
    } = &fence.blocks[0].kind
    else {
        panic!("a code block");
    };
    let themes = [
        (EditorPalette::fastedit_dark(), UiTheme::default_dark()),
        (EditorPalette::fastedit_light(), Theme::light().ui_theme()),
    ];
    let mut backgrounds = Vec::new();
    for (palette, ui) in themes {
        let editor =
            cx.update(|cx| EditorTheme::from_palette(palette, &EditorSettings::default(), cx));
        let style = MarkdownStyle::new(&editor, &ui);
        assert_eq!(style.background, editor.background);
        assert_eq!(style.foreground, editor.foreground);
        assert_eq!(style.syntax, editor.syntax);
        assert_eq!(
            (style.muted, style.border, style.link, style.code_background),
            (ui.muted_text, ui.border, ui.accent, ui.panel_background)
        );
        assert_eq!(
            (style.code_font_family.clone(), style.code_font_size),
            (editor.font_family.clone(), editor.font_size)
        );

        // `pub` and `struct` are keywords, each its own run.
        let runs = style.code_runs(code, highlights);
        assert_eq!(runs.iter().map(|run| run.len).sum::<usize>(), code.len());
        let keyword_color = editor.syntax_color(keyword).expect("a keyword colour");
        assert_eq!((runs[0].len, runs[0].color), (3, keyword_color), "`pub`");
        let struct_at = runs.iter().position(|run| run.len == "struct".len());
        assert_eq!(
            struct_at.map(|ix| runs[ix].color),
            Some(keyword_color),
            "`struct`"
        );
        assert_ne!(keyword_color, style.foreground);
        backgrounds.push(style.background);
    }
    assert_ne!(backgrounds[0], backgrounds[1], "light differs from dark");
}
