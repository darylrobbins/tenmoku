//! Hover and completion documentation as Markdown, as VS Code draws it:
//! each editor's [`MarkdownRenderer`] parses it with fe-markdown and draws
//! the compact form, code highlighted in the editor's theme. A link opens
//! as the preview tab opens its links, except that an anchor has no
//! heading to scroll to and is ignored.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_gpui::{EditorTheme, MarkdownRenderer};
use fe_markdown::{Block, BlockKind, Document, Inline, LinkTarget};
use fe_markdown_view::{LinkHandler, MarkdownStyle, markdown_element};
use fe_ui::ui_theme;
use gpui::{AnyElement, App, Context, EntityId, Pixels, WeakEntity, Window};

use kiln_l10n::verbatim;

use crate::{Workbench, pane::Reveal};

/// Every editor's renderer.
#[derive(Default)]
pub(super) struct Renderers {
    by_editor: HashMap<EntityId, Rc<HoverMarkdown>>,
}

/// Draws one editor's documentation.
struct HoverMarkdown {
    workbench: WeakEntity<Workbench>,
    /// The editor's file, and the window's root: what relative links
    /// resolve against.
    location: RefCell<(PathBuf, Option<PathBuf>)>,
    /// The last parse, by its text: a redraw doesn't parse again.
    parsed: RefCell<Option<(String, Document)>>,
}

impl MarkdownRenderer for HoverMarkdown {
    fn render(
        &self,
        markdown: &str,
        theme: &EditorTheme,
        max_width: Pixels,
        _: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let document = {
            let mut parsed = self.parsed.borrow_mut();
            match &*parsed {
                Some((text, document)) if text == markdown => document.clone(),
                _ => {
                    let document = fe_markdown::parse(markdown);
                    *parsed = Some((markdown.to_owned(), document.clone()));
                    document
                }
            }
        };
        let style = MarkdownStyle::new(theme, &ui_theme(cx)).compact();
        let (doc, root) = self.location.borrow().clone();
        let on_link = link_handler(self.workbench.clone(), Some(doc), root);
        markdown_element(&document, &style, max_width, Some(on_link))
    }
}

/// `markdown` drawn as the hover draws it in `theme`, wrapped at `max_width`,
/// for a host with no editor: a link opens as the preview tab opens it, the
/// relative ones against `root` (ignored when there is none). With
/// `keep_breaks`, a lone newline in a paragraph is a line break, as a
/// commit message's lines are.
pub(crate) fn markdown_body(
    markdown: &str,
    keep_breaks: bool,
    theme: &EditorTheme,
    max_width: Pixels,
    workbench: WeakEntity<Workbench>,
    root: Option<PathBuf>,
    cx: &App,
) -> AnyElement {
    let mut document = fe_markdown::parse(markdown);
    if keep_breaks {
        document.blocks.iter_mut().for_each(hard_breaks);
    }
    let style = MarkdownStyle::new(theme, &ui_theme(cx)).compact();
    // With no root, nothing relative resolves.
    let doc = root.as_ref().map(|root| root.join(verbatim!("Untitled")));
    let on_link = link_handler(workbench, doc, root);
    markdown_element(&document, &style, max_width, Some(on_link))
}

/// Turns every soft break under `block` into a hard one.
fn hard_breaks(block: &mut Block) {
    fn inlines(content: &mut [Inline]) {
        for inline in content {
            match inline {
                Inline::SoftBreak => *inline = Inline::HardBreak,
                Inline::Emphasis(content)
                | Inline::Strong(content)
                | Inline::Strikethrough(content)
                | Inline::Link { content, .. } => inlines(content),
                _ => {}
            }
        }
    }
    match &mut block.kind {
        BlockKind::Paragraph(content) | BlockKind::Heading { content, .. } => inlines(content),
        BlockKind::Quote(blocks) => blocks.iter_mut().for_each(hard_breaks),
        BlockKind::List { items, .. } => {
            for item in items {
                item.blocks.iter_mut().for_each(hard_breaks);
            }
        }
        _ => {}
    }
}

/// What a click on a link does: a web link opens in the browser, a file's
/// in the workbench's pane, and an anchor or other scheme is ignored.
/// `doc` and `root` are what relative links resolve against.
fn link_handler(
    workbench: WeakEntity<Workbench>,
    doc: Option<PathBuf>,
    root: Option<PathBuf>,
) -> LinkHandler {
    Rc::new(move |dest, window, cx| {
        let target = fe_markdown::link_target(
            dest,
            doc.as_deref().unwrap_or(Path::new("")),
            root.as_deref(),
        );
        match target {
            LinkTarget::File { .. } if doc.is_none() => log::info!("no folder to open {dest} in"),
            LinkTarget::Web(url) => cx.open_url(&url),
            LinkTarget::File { path, line } => {
                // Once this update is over: opening reads the panes'
                // items, the editor among them.
                let workbench = workbench.clone();
                window.defer(cx, move |window, cx| {
                    workbench
                        .update(cx, |workbench, cx| {
                            let reveal = line.map(|row| Reveal {
                                row,
                                columns: 0..0,
                                within_row: true,
                            });
                            let pane = workbench.pane().clone();
                            pane.update(cx, |pane, cx| pane.open_at(path, reveal, window, cx));
                        })
                        .ok();
                });
            }
            LinkTarget::Anchor(_) | LinkTarget::Unsupported => {
                log::info!("hover documentation doesn't open {dest}")
            }
        }
    })
}

impl Workbench {
    /// Gives every open editor a renderer for its documentation, following
    /// the file it shows and the root.
    pub(super) fn sync_markdown_renderers(&mut self, cx: &mut Context<Self>) {
        let editors = self.open_editors(cx);
        let root = self.lsp.root.clone();
        let workbench = cx.entity().downgrade();
        let renderers = &mut self.lsp.markdown.by_editor;
        renderers.retain(|id, _| editors.iter().any(|editor| editor.entity_id() == *id));
        for editor in &editors {
            let path = (editor.read(cx).model().path())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| root.clone().unwrap_or_default().join(verbatim!("Untitled")));
            let renderer = renderers
                .entry(editor.entity_id())
                .or_insert_with(|| {
                    Rc::new(HoverMarkdown {
                        workbench: workbench.clone(),
                        location: Default::default(),
                        parsed: Default::default(),
                    })
                })
                .clone();
            *renderer.location.borrow_mut() = (path, root.clone());
            let set = editor.read(cx).markdown_renderer();
            let wanted: Rc<dyn MarkdownRenderer> = renderer;
            if !set.is_some_and(|set| Rc::ptr_eq(set, &wanted)) {
                editor.update(cx, |view, _| view.set_markdown_renderer(Some(wanted)));
            }
        }
    }
}
