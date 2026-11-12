//! Hover and completion documentation as Markdown, as VS Code draws it:
//! each editor's [`MarkdownRenderer`] parses it with fe-markdown and draws
//! the compact form, code highlighted in the editor's theme. A link opens
//! as the preview tab opens its links, except that an anchor has no
//! heading to scroll to and is ignored.

use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

use fe_gpui::{EditorTheme, MarkdownRenderer};
use fe_markdown::{Document, LinkTarget};
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
        let (workbench, location) = (self.workbench.clone(), self.location.borrow().clone());
        let on_link: LinkHandler = Rc::new(move |dest, window, cx| {
            let (doc, root) = &location;
            match fe_markdown::link_target(dest, doc, root.as_deref()) {
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
        });
        markdown_element(&document, &style, max_width, Some(on_link))
    }
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
