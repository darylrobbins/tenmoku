//! The document element in a test window: no scroll of its own, images as
//! their alt text, link clicks and code block headers.

use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
use fe_markdown::Document;
use fe_markdown_view::{CodeHeader, ElementOptions, Images, MarkdownStyle, document_element};
use gpui::{
    AnyElement, Bounds, Context, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Pixels, Render, ScrollDelta, ScrollWheelEvent, Styled as _, TestAppContext,
    TouchPhase, VisualTestContext, Window, div, point, px,
};
use kiln_ui::UiTheme;

/// A code header call: the block's index, info, language and code.
type Header = (usize, String, Option<&'static str>, String);

#[derive(Default)]
struct Log {
    clicks: Vec<String>,
    headers: Vec<Header>,
}

struct Host {
    document: Arc<Document>,
    style: MarkdownStyle,
    images: Images,
    log: Rc<RefCell<Log>>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let log = self.log.clone();
        let on_link = Rc::new(move |dest: &str, _: &mut Window, _: &mut gpui::App| {
            log.borrow_mut().clicks.push(dest.to_owned());
        });
        let log = self.log.clone();
        let code_header = Rc::new(
            move |block: CodeHeader<'_>, _: &mut Window, _: &mut gpui::App| -> AnyElement {
                let index = block.index;
                log.borrow_mut().headers.push((
                    index,
                    block.info.to_owned(),
                    block.language,
                    block.code.to_owned(),
                ));
                div()
                    .debug_selector(move || format!("header-{index}"))
                    .h(px(20.))
                    .child(block.info.to_owned())
                    .into_any_element()
            },
        );
        let options = ElementOptions {
            images: self.images.clone(),
            on_link: Some(on_link),
            code_header: Some(code_header),
            ..ElementOptions::default()
        };
        // A short box, which a scrolling element would fit itself to.
        div()
            .debug_selector(|| "box".into())
            .w(px(500.))
            .h(px(120.))
            .flex()
            .flex_col()
            .child(document_element(&self.document, &self.style, options))
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    markdown: &str,
    images: Images,
) -> (&'a mut VisualTestContext, Rc<RefCell<Log>>) {
    let style = cx.update(|cx| {
        let editor = EditorTheme::from_palette(
            EditorPalette::fastedit_dark(),
            &EditorSettings::default(),
            cx,
        );
        MarkdownStyle::new(&editor, &UiTheme::default_dark())
    });
    let log = Rc::new(RefCell::new(Log::default()));
    let host = Host {
        document: Arc::new(fe_markdown::parse(markdown)),
        style,
        images,
        log: log.clone(),
    };
    let (_, cx) = cx.add_window_view(|_, _| host);
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    (cx, log)
}

fn bounds(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(selector.leak())
}

const PICTURE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="red"/></svg>"#;

#[gpui::test]
fn document_element_draws_without_scroll_or_images(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pic.svg"), PICTURE).unwrap();
    let markdown = format!(
        "# Title\n\n\
         A [link](https://example.com/x)\n\n\
         ![the alt]({})\n\n\
         ```rust path=src/lib.rs\nfn one() {{}}\n```\n\n\
         - an item\n\n  ```\n  nested\n  ```\n\n\
         ```py\nprint(1)\n```\n",
        dir.path().join("pic.svg").display()
    );
    let (cx, log) = open(cx, &markdown, Images::AltText);

    // No scroll: the blocks run on past the short box at their full
    // height, and a wheel over them moves nothing.
    let boxed = bounds(cx, "box".into()).expect("the box");
    let first = bounds(cx, "md-block-0".into()).expect("the heading");
    let last = bounds(cx, "md-block-5".into()).expect("the last fence");
    assert!(
        last.bottom() > boxed.bottom() + px(100.),
        "{last:?} in {boxed:?}"
    );
    assert!(bounds(cx, "md-block-6".into()).is_none());
    cx.simulate_event(ScrollWheelEvent {
        position: first.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-200.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(bounds(cx, "md-block-0".into()), Some(first));

    // The image is its alt text: no image element, which is what reads a
    // file, though the file is there to read.
    assert!(bounds(cx, "md-image-alt-0".into()).is_some());
    assert_eq!(bounds(cx, "md-image-0".into()), None);
    assert_eq!(bounds(cx, "md-image-blocked-0".into()), None);

    // A click on the link calls on_link with its destination.
    let paragraph = bounds(cx, "md-block-1".into()).expect("the paragraph");
    let y = paragraph.origin.y + px(8.);
    let mut x = paragraph.origin.x;
    while x < paragraph.right() && log.borrow().clicks.is_empty() {
        cx.simulate_click(point(x, y), Modifiers::none());
        x += px(4.);
    }
    assert_eq!(log.borrow().clicks, ["https://example.com/x"]);

    // Each fence gets one header call a frame, the one in the list too,
    // numbered in document order, and the header is drawn above its code.
    log.borrow_mut().headers.clear();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let headers = std::mem::take(&mut log.borrow_mut().headers);
    assert_eq!(
        headers,
        [
            (
                0,
                "rust path=src/lib.rs".to_owned(),
                Some("rust"),
                "fn one() {}\n".to_owned()
            ),
            (1, String::new(), None, "nested\n".to_owned()),
            (2, "py".to_owned(), Some("python"), "print(1)\n".to_owned()),
        ]
    );
    for index in 0..3 {
        let header = bounds(cx, format!("header-{index}")).expect("a header");
        let code = bounds(cx, format!("md-code-{index}")).expect("its code");
        assert_eq!(header.bottom(), code.top(), "header {index}");
    }

    // The same document loading images draws the file.
    let doc_path: PathBuf = dir.path().join("README.md");
    let images = Images::Load {
        doc_path,
        root: Some(dir.path().to_owned()),
    };
    let (cx, _) = open(cx, &markdown, images);
    assert!(bounds(cx, "md-image-0".into()).is_some());
    assert_eq!(bounds(cx, "md-image-alt-0".into()), None);
}
