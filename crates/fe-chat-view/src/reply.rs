//! Replies drawn as Markdown (the decision record's § 1): each turn's
//! reply and thinking parsed with `fe_markdown`, at most once a frame while
//! they stream, and drawn with `fe_markdown_view`'s document element, with
//! a header over each code block to copy or apply it (§ 8).

use std::{
    collections::HashMap,
    path::{Component, Path},
    rc::Rc,
    sync::Arc,
};

use fe_gpui::EditorTheme;
use fe_markdown::{Block, BlockKind, Document, LinkTarget, link_target};
use fe_markdown_view::{
    CodeHeader, ElementOptions, Images, LinkHandler, MarkdownStyle, document_element,
};
use gpui::{
    AnyElement, App, ClipboardItem, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Styled as _, WeakEntity, Window, div,
};
use kiln_ui::{Button, UiTheme, h_flex};

use crate::{ChatView, ChatViewEvent};

/// What a code block's header shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
    /// The language the fence names, as written: its info string's first
    /// word.
    pub language: Option<String>,
    /// The file the block is for: its info string's `path=`.
    pub path: Option<String>,
    /// The code, with `\n` line endings.
    pub code: String,
}

impl CodeBlock {
    /// The block a fence with info string `info` holds `code`.
    pub(crate) fn new(info: &str, code: &str) -> CodeBlock {
        // A fence may name only the path.
        let language = (info.split_whitespace().next()).filter(|word| !word.starts_with("path="));
        CodeBlock {
            language: language.map(str::to_owned),
            path: info_path(info).map(str::to_owned),
            code: code.to_owned(),
        }
    }
}

/// The `path=` an info string gives, a word or in double quotes.
fn info_path(info: &str) -> Option<&str> {
    let (at, _) = (info.match_indices("path=")).find(|(at, _)| {
        info[..*at]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace)
    })?;
    let value = &info[at + "path=".len()..];
    let path = match value.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next(),
        None => value.split_whitespace().next(),
    };
    path.filter(|path| !path.is_empty())
}

/// `document`'s code blocks, in document order: those in quotes and list
/// items where they stand, as the document element numbers them.
pub(crate) fn code_blocks(document: &Document) -> Vec<CodeBlock> {
    fn walk(blocks: &[Block], out: &mut Vec<CodeBlock>) {
        for block in blocks {
            match &block.kind {
                BlockKind::Code { info, code, .. } => out.push(CodeBlock::new(info, code)),
                BlockKind::Quote(blocks) => walk(blocks, out),
                BlockKind::List { items, .. } => {
                    for item in items {
                        walk(&item.blocks, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&document.blocks, &mut out);
    out
}

/// How many of `document`'s code blocks are for a file: those whose info
/// string has a `path=`, in quotes and lists too.
pub(crate) fn path_blocks(document: &Document) -> usize {
    fn walk(blocks: &[Block]) -> usize {
        blocks
            .iter()
            .map(|block| match &block.kind {
                BlockKind::Code { info, code, .. } => {
                    usize::from(match fe_chat::parse_edit_block(info, code) {
                        Ok(edit) => edit.path.is_some(),
                        Err(fe_chat::EditBlockError::Path(_)) => true,
                        Err(_) => fe_chat::info_path(info).is_some(),
                    })
                }
                BlockKind::Quote(blocks) => walk(blocks),
                BlockKind::List { items, .. } => items.iter().map(|item| walk(&item.blocks)).sum(),
                _ => 0,
            })
            .sum()
    }
    walk(&document.blocks)
}

/// A turn's reply or thinking as last parsed, and whether the turn had
/// finished.
struct Parsed {
    text: String,
    finished: bool,
    document: Arc<Document>,
}

/// The documents the transcript draws, by turn, each parsed again only
/// when its text has changed, and once more when its turn finishes: as the
/// view parses in `render`, a frame's deltas are parsed together, once,
/// with the finish if it came in the same frame.
#[derive(Default)]
pub(crate) struct Documents {
    replies: HashMap<usize, Parsed>,
    thinking: HashMap<usize, Parsed>,
    parses: usize,
}

impl Documents {
    /// Turn `turn`'s reply, `text`, parsed, the turn `finished` or still
    /// streaming. An unclosed fence is code to the end, so a block half
    /// streamed is drawn as code.
    pub(crate) fn reply(&mut self, turn: usize, text: &str, finished: bool) -> Arc<Document> {
        parsed(&mut self.replies, turn, text, finished, &mut self.parses)
    }

    /// Turn `turn`'s thinking summary, `text`, parsed, as
    /// [`Documents::reply`].
    pub(crate) fn thinking(&mut self, turn: usize, text: &str, finished: bool) -> Arc<Document> {
        parsed(&mut self.thinking, turn, text, finished, &mut self.parses)
    }

    /// How many times a reply or thinking was parsed.
    pub(crate) fn parses(&self) -> usize {
        self.parses
    }
}

fn parsed(
    documents: &mut HashMap<usize, Parsed>,
    turn: usize,
    text: &str,
    finished: bool,
    parses: &mut usize,
) -> Arc<Document> {
    if let Some(parsed) = documents.get(&turn)
        && parsed.text == text
        && parsed.finished == finished
    {
        return parsed.document.clone();
    }
    *parses += 1;
    let document = Arc::new(fe_markdown::parse(text));
    let parsed = Parsed {
        text: text.to_owned(),
        finished,
        document: document.clone(),
    };
    documents.insert(turn, parsed);
    document
}

/// How replies are drawn: in the editor's colours and code font, as the
/// Markdown preview is, with code on the editor's background, since the
/// chat's own is the panel's, which the preview's code is drawn on.
pub(crate) fn reply_style(editor: &EditorTheme, ui: &UiTheme) -> MarkdownStyle {
    MarkdownStyle {
        code_background: editor.background,
        ..MarkdownStyle::new(editor, ui)
    }
}

/// How thinking is drawn: as a reply, muted.
pub(crate) fn thinking_style(reply: &MarkdownStyle) -> MarkdownStyle {
    MarkdownStyle {
        foreground: reply.muted,
        ..reply.clone()
    }
}

/// Turn `turn`'s reply, `document`, with a header over each code block.
/// Apply is off while the reply streams, its blocks unfinished.
pub(crate) fn reply_element(
    turn: usize,
    document: &Arc<Document>,
    style: &MarkdownStyle,
    streaming: bool,
    ui: UiTheme,
    view: WeakEntity<ChatView>,
) -> AnyElement {
    let header = {
        let view = view.clone();
        Rc::new(move |block: CodeHeader<'_>, _: &mut Window, _: &mut App| {
            code_header(turn, block, streaming, &ui, view.clone())
        })
    };
    let options = ElementOptions {
        images: Images::AltText,
        on_link: Some(link_handler(view)),
        code_header: Some(header),
    };
    document_element(document, style, options)
}

/// A thinking summary, `document`.
pub(crate) fn thinking_element(
    document: &Arc<Document>,
    style: &MarkdownStyle,
    view: WeakEntity<ChatView>,
) -> AnyElement {
    let options = ElementOptions {
        images: Images::AltText,
        on_link: Some(link_handler(view)),
        code_header: None,
    };
    document_element(document, style, options)
}

/// Code block `block.index` of turn `turn`'s reply's header: its language
/// and path, Copy, and Apply, which asks the host to apply it. Its
/// `debug_selector` is `chat-code-<turn>-<block>`, and its buttons'
/// `chat-copy-<turn>-<block>` and `chat-apply-<turn>-<block>`.
fn code_header(
    turn: usize,
    block: CodeHeader<'_>,
    streaming: bool,
    ui: &UiTheme,
    view: WeakEntity<ChatView>,
) -> AnyElement {
    let index = block.index;
    let CodeBlock {
        language,
        path,
        code,
    } = CodeBlock::new(block.info, block.code);
    let copy = move |_: &_, _: &mut Window, cx: &mut App| {
        cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
    };
    let apply = move |_: &_, _: &mut Window, cx: &mut App| {
        view.update(cx, |_, cx| {
            cx.emit(ChatViewEvent::ApplyBlock { turn, block: index })
        })
        .ok();
    };
    let id = |name: &str| SharedString::from(format!("{name}-{turn}-{index}"));
    h_flex()
        .debug_selector(move || format!("chat-code-{turn}-{index}"))
        .gap_2()
        .px_2()
        .py_1()
        .items_center()
        .border_b_1()
        .border_color(ui.border)
        .text_color(ui.muted_text)
        .children(language.map(|language| div().child(language)))
        .children(path.map(|path| div().min_w_0().text_color(ui.text).child(path)))
        .child(div().flex_1())
        .child(
            Button::new(id("chat-copy"), "Copy")
                .small()
                .debug_selector(move || format!("chat-copy-{turn}-{index}"))
                .on_click(copy),
        )
        .child(
            Button::new(id("chat-apply"), "Apply")
                .small()
                .disabled(streaming)
                .debug_selector(move || format!("chat-apply-{turn}-{index}"))
                .on_click(apply),
        )
        .into_any_element()
}

/// Where a link in a reply goes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Link {
    /// An `http:` or `https:` URL, for the browser.
    Web(String),
    /// A path relative to the project, and the 1-based row a `#L12`
    /// fragment names.
    Path { path: String, row: Option<u32> },
}

/// Where the link `dest` goes, as written: `None` for an anchor, an
/// absolute path, a path out of the project, or any scheme but `http:` and
/// `https:`, `file:` among them.
fn link(dest: &str) -> Option<Link> {
    let dest = dest.trim();
    if dest.starts_with(['/', '\\']) {
        return None;
    }
    let scheme = dest.split_once(':').map(|(scheme, _)| scheme);
    let web = scheme.is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    });
    // As fe-markdown reads a scheme: a letter, then letters, digits, `+`,
    // `-` or `.`.
    let schemed = scheme.is_some_and(|scheme| {
        let mut chars = scheme.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if schemed && !web {
        return None;
    }
    match link_target(dest, Path::new(""), None) {
        LinkTarget::Web(url) if web => Some(Link::Web(url)),
        LinkTarget::File { path, line }
            if !path.as_os_str().is_empty()
                && path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))) =>
        {
            Some(Link::Path {
                path: path.to_string_lossy().into_owned(),
                row: line.map(|line| line + 1),
            })
        }
        _ => None,
    }
}

/// A clicked link: a URL opens in the browser, and a path is
/// [`ChatViewEvent::OpenPath`] for the host.
fn link_handler(view: WeakEntity<ChatView>) -> LinkHandler {
    Rc::new(move |dest, _, cx| match link(dest) {
        Some(Link::Web(url)) => cx.open_url(&url),
        Some(Link::Path { path, row }) => {
            view.update(cx, |_, cx| cx.emit(ChatViewEvent::OpenPath { path, row }))
                .ok();
        }
        None => {}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_strings_give_language_and_path() {
        let block = |info| {
            let block = CodeBlock::new(info, "");
            (block.language, block.path)
        };
        let some = |text: &str| Some(text.to_owned());
        assert_eq!(
            block("rust path=src/lib.rs"),
            (some("rust"), some("src/lib.rs"))
        );
        assert_eq!(
            block("rust  path=\"src/a b.rs\" title=x"),
            (some("rust"), some("src/a b.rs"))
        );
        assert_eq!(block("rust xpath=a"), (some("rust"), None));
        assert_eq!(
            block("ts path=\"web/app.ts\""),
            (some("ts"), some("web/app.ts"))
        );
        assert_eq!(block("path=Cargo.toml"), (None, some("Cargo.toml")));
        assert_eq!(block("py"), (some("py"), None));
        assert_eq!(block("rust path="), (some("rust"), None));
        assert_eq!(block(""), (None, None));
    }

    #[test]
    fn links_go_to_the_browser_or_a_project_path() {
        let path = |path: &str, row| {
            Some(Link::Path {
                path: path.to_owned(),
                row,
            })
        };
        assert_eq!(
            link("https://example.com/a?b#c"),
            Some(Link::Web("https://example.com/a?b#c".to_owned()))
        );
        assert_eq!(
            link("HTTP://example.com"),
            Some(Link::Web("HTTP://example.com".to_owned()))
        );
        assert_eq!(link("src/lib.rs#L12"), path("src/lib.rs", Some(12)));
        assert_eq!(link("src/lib.rs#L12C3"), path("src/lib.rs", Some(12)));
        assert_eq!(link("./src/a%20b.rs"), path("src/a b.rs", None));
        assert_eq!(link("src/../Cargo.toml"), path("Cargo.toml", None));
        // A colon past the first path component isn't a scheme.
        assert_eq!(link("src/a:b.rs"), path("src/a:b.rs", None));
        for nowhere in [
            "mailto:a@example.com",
            "#heading",
            "#L3",
            "?query",
            "/etc/passwd",
            "\\\\server\\share",
            "../outside.rs",
            "file:///etc/passwd",
            "file:src/lib.rs",
            "FILE:src/lib.rs#L3",
            "vscode:src/lib.rs",
            "c:src/lib.rs",
            "javascript:alert(1)",
            "",
        ] {
            assert_eq!(link(nowhere), None, "{nowhere}");
        }
    }
}
