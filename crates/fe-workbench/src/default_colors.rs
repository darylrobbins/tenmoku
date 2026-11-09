//! The colours Tenmoku finds itself, with no language server, as VS Code's
//! `editor.defaultColorDecorators`: CSS colours in CSS, SCSS, Less, Sass
//! and Stylus, whole buffers or injected layers (an HTML `<style>`, a
//! Markdown fence), and `#rrggbb` or `#rrggbbaa` in other languages'
//! strings.
//!
//! As [`crate::todo_comments`], each editor shown scans the rows in view and
//! [`MARGIN`](crate::spell_squiggles::MARGIN) rows either side, from the
//! parse it has, on this thread: at once when it is first shown, and
//! [`REST`] after its last edit or a scroll to rows not scanned. Rows stay
//! scanned until the next edit or a change of language. A buffer over
//! [`MAX_SPELLABLE_BYTES`] or with no grammar has none.
//!
//! Which swatches an editor shows, the server's or these, is
//! [`Workbench::show_color_swatches`]'s to say, and nothing else sets them.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    time::Duration,
};

use fe_buffer::{BufferSnapshot, Version};
use fe_config::settings::DefaultColorDecorators;
use fe_gpui::{ColorSwatch, EditorView};
use fe_syntax::{InjectedNode, Syntax};
use fe_text::Text;
use gpui::{AppContext as _, Context, Entity, EntityId, Task};

use super::{REST, give_swatches};
use crate::{
    Workbench,
    color_literals::{FoundColor, css_colors, hex_colors},
    spell_ranges::MAX_SPELLABLE_BYTES,
    spell_squiggles::{add_rows, language_of, rows_to_check, unchecked_rows},
};

/// How many more times rows are scanned when their highlights didn't finish.
const RETRIES: u32 = 8;

/// The built-in colours of the editors shown, and the setting saying when
/// they're wanted.
#[derive(Default)]
pub(super) struct Defaults {
    /// `editor.default_color_decorators`.
    pub(super) mode: DefaultColorDecorators,
    scans: HashMap<EntityId, Scan>,
}

/// One editor's colours and what's known of its rows.
struct Scan {
    editor: Entity<EditorView>,
    version: Version,
    language: Option<&'static str>,
    /// The rows scanned, sorted and apart.
    scanned: Vec<Range<u32>>,
    /// The colours found, at `found_at`, in order.
    found: Vec<ColorSwatch>,
    found_at: Version,
    /// No scan has finished since the editor was first shown, so the next
    /// one doesn't wait.
    fresh: bool,
    task: Option<Task<()>>,
    retries: u32,
}

/// Whose swatches an editor shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    None,
    Server,
    BuiltIn,
}

/// Whose swatches an editor shows: `on` for `editor.color_decorators`,
/// and `answered` when its server's latest answer had colours.
fn source(on: bool, mode: DefaultColorDecorators, answered: bool) -> Source {
    match (on, mode) {
        (false, _) => Source::None,
        (true, DefaultColorDecorators::Always) => Source::BuiltIn,
        (true, _) if answered => Source::Server,
        (true, DefaultColorDecorators::Never) => Source::None,
        (true, DefaultColorDecorators::Auto) => Source::BuiltIn,
    }
}

impl Workbench {
    /// Follows `editor.default_color_decorators`: every open editor's
    /// swatches are picked again at once.
    pub(crate) fn set_default_color_decorators(
        &mut self,
        mode: DefaultColorDecorators,
        cx: &mut Context<Self>,
    ) {
        if self.lsp.colors.defaults.mode == mode {
            return;
        }
        self.lsp.colors.defaults.mode = mode;
        for editor in self.open_editors(cx) {
            self.show_color_swatches(&editor, cx);
        }
        cx.notify();
    }

    /// Gives `editor` the swatches it should show, the server's or the
    /// built-in ones, if they aren't what it shows.
    pub(super) fn show_color_swatches(
        &mut self,
        editor: &Entity<EditorView>,
        cx: &mut Context<Self>,
    ) {
        let colors = &self.lsp.colors;
        let view = editor.read(cx);
        let model = view.model();
        let snapshot = model.snapshot();
        let answered =
            (colors.slots.get(&model.buffer().id())).and_then(|slot| slot.answered.as_ref());
        let on = self.lsp.color_decorators_on.get();
        let wanted = match source(on, colors.defaults.mode, answered.is_some()) {
            Source::None => None,
            Source::Server => answered.map(|(at, swatches)| follow(snapshot, *at, swatches)),
            Source::BuiltIn => (colors.defaults.scans.get(&editor.entity_id()))
                .map(|scan| follow(snapshot, scan.found_at, &scan.found)),
        };
        let wanted = wanted.unwrap_or_default();
        if wanted != view.color_swatches() {
            give_swatches(editor, wanted, cx);
        }
    }

    /// Brings the built-in colours of what's shown up to date: run by
    /// [`Self::sync_colors`] after each draw.
    pub(super) fn sync_default_colors(&mut self, cx: &mut Context<Self>) {
        let defaults = &mut self.lsp.colors.defaults;
        if !self.lsp.color_decorators_on.get() || defaults.mode == DefaultColorDecorators::Never {
            let gone: Vec<_> = (defaults.scans.drain())
                .map(|(_, scan)| scan.editor)
                .collect();
            for editor in gone {
                self.show_color_swatches(&editor, cx);
            }
            return;
        }
        let open: HashSet<EntityId> = (self.open_editors(cx).iter())
            .map(Entity::entity_id)
            .collect();
        (self.lsp.colors.defaults.scans).retain(|id, _| open.contains(id));
        let shown: Vec<_> = (self.panes(cx).iter())
            .filter_map(|pane| pane.read(cx).active_editor().cloned())
            .collect();
        for editor in shown {
            let id = editor.entity_id();
            let view = editor.read(cx);
            let version = view.model().snapshot().version();
            let language = language_of(view);
            let rows = rows_to_check(view);
            let scan = (self.lsp.colors.defaults.scans.entry(id)).or_insert_with(|| Scan {
                editor: editor.clone(),
                version,
                language,
                scanned: Vec::new(),
                found: Vec::new(),
                found_at: version,
                fresh: true,
                task: None,
                retries: 0,
            });
            let relanguaged = scan.language != language;
            if relanguaged {
                scan.found.clear();
                scan.found_at = version;
            }
            if relanguaged || scan.version != version {
                scan.language = language;
                scan.version = version;
                scan.scanned.clear();
                scan.retries = 0;
                scan.task = Some(scan_after(id, REST, Vec::new(), cx));
            } else if scan.task.is_none()
                && rows.is_some_and(|rows| !unchecked_rows(&rows, &scan.scanned).is_empty())
            {
                let delay = if scan.fresh { Duration::ZERO } else { REST };
                scan.task = Some(scan_after(id, delay, Vec::new(), cx));
            }
            if relanguaged {
                self.show_color_swatches(&editor, cx);
            }
        }
    }

    /// The wait is over: scans the rows of editor `id` in view that aren't
    /// yet, once its parse is current.
    fn scan_default_colors(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(scan) = self.lsp.colors.defaults.scans.get_mut(&id) else {
            return;
        };
        scan.task = None;
        let view = scan.editor.read(cx);
        let model = view.model();
        let snapshot = model.snapshot().clone();
        let syntax = view.syntax();
        let behind = model.buffer().version() != snapshot.version()
            || scan.version != snapshot.version()
            || syntax.is_some_and(|it| it.version() != snapshot.version() || it.needs_parse());
        if view.is_loading_language() || behind {
            return;
        }
        let Some(window) = rows_to_check(view) else {
            return;
        };
        let text = snapshot.text();
        let rows = unchecked_rows(&window, &scan.scanned);
        let mut found = Vec::new();
        for rows in &rows {
            match colors_in(syntax, text, rows.clone()) {
                Some(colors) => found.extend(colors),
                None if scan.retries < RETRIES => {
                    let missing = syntax.map(|it| it.take_missing_languages());
                    scan.retries += 1;
                    scan.task = Some(scan_after(id, REST, missing.unwrap_or_default(), cx));
                    return;
                }
                // Out of tries: these rows have none until the next edit.
                None => {}
            }
        }
        if scan.found_at != snapshot.version() {
            scan.found.clear();
            scan.found_at = snapshot.version();
        }
        scan.found
            .extend(found.into_iter().map(|color| ColorSwatch {
                offset: color.range.start,
                rgba: color.rgba,
            }));
        scan.found.sort_by_key(|swatch| swatch.offset);
        scan.found.dedup_by_key(|swatch| swatch.offset);
        for rows in rows {
            add_rows(&mut scan.scanned, rows);
        }
        scan.retries = 0;
        scan.fresh = false;
        let editor = scan.editor.clone();
        self.show_color_swatches(&editor, cx);
    }
}

/// Scans editor `id` after `delay`, having compiled the `missing` languages
/// a scan found, off this thread.
fn scan_after(
    id: EntityId,
    delay: Duration,
    missing: Vec<&'static str>,
    cx: &mut Context<Workbench>,
) -> Task<()> {
    cx.spawn(async move |workbench, cx| {
        if !missing.is_empty() {
            cx.background_spawn(async move {
                for name in missing {
                    fe_syntax::language_by_name(name);
                }
            })
            .await;
        }
        if !delay.is_zero() {
            cx.background_executor().timer(delay).await;
        }
        workbench
            .update(cx, |workbench, cx| workbench.scan_default_colors(id, cx))
            .ok();
    })
}

/// Whether `language` is CSS or one of its kin, whose colours are CSS's.
fn is_css(language: &str) -> bool {
    matches!(language, "css" | "scss" | "less" | "sass" | "stylus")
}

/// The colours starting in `rows` of `text`, as `syntax` parses it, in
/// order. None when highlighting didn't finish: a language that isn't
/// compiled yet, or a query cut short. Plain text and a buffer too big
/// have none.
pub(super) fn colors_in(
    syntax: Option<&Syntax>,
    text: &Text,
    rows: Range<u32>,
) -> Option<Vec<FoundColor>> {
    let Some(syntax) = syntax else {
        return Some(Vec::new());
    };
    let bytes = row_bytes(text, rows);
    if bytes.is_empty()
        || text.len() > MAX_SPELLABLE_BYTES
        || syntax.failed()
        || syntax.tree().is_none()
        || syntax.needs_parse()
    {
        return Some(Vec::new());
    }
    let highlights = syntax.checked_highlights(bytes.clone());
    if !highlights.complete {
        return None;
    }
    let mut found = Vec::new();
    if is_css(syntax.language().name()) {
        found.extend(in_declarations(&host_nodes(syntax, bytes.clone()), text));
    } else {
        // A string's spans, merged, so a literal split across them is whole.
        let mut strings: Vec<Range<usize>> = (highlights.spans.into_iter())
            .filter(|span| span.highlight.name() == "string")
            .map(|span| span.range)
            .collect();
        strings.sort_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for range in strings {
            match merged.last_mut() {
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        for range in merged {
            let source = text.slice_to_string(range.clone());
            found.extend(hex_colors(&source).into_iter().map(|color| FoundColor {
                range: range.start + color.range.start..range.start + color.range.end,
                rgba: color.rgba,
            }));
        }
    }
    // Injected CSS: an HTML `<style>`, a Markdown fence.
    found.extend(in_declarations(&syntax.injected_nodes(bytes.clone()), text));
    found.retain(|color| bytes.contains(&color.range.start));
    found.sort_by_key(|color| color.range.start);
    found.dedup_by_key(|color| color.range.start);
    Some(found)
}

/// A CSS declaration among some nodes: its bytes, the comments in it, and
/// its `plain_value`s, the only places a named colour is one.
struct Declaration {
    range: Range<usize>,
    comments: Vec<Range<usize>>,
    plain: Vec<Range<usize>>,
}

/// The colours in the declarations of CSS and its kin among `nodes`, one or
/// more trees' nodes, parents before children: the value of `color: red`,
/// or SCSS's `$accent: #fff`, but not a selector's `#add` or a comment.
fn in_declarations(nodes: &[InjectedNode], text: &Text) -> Vec<FoundColor> {
    let mut declarations: Vec<Declaration> = Vec::new();
    // For each node, the declaration it's in.
    let mut holder: Vec<Option<usize>> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let own = match node.parent.and_then(|parent| holder[parent]) {
            Some(index) => {
                let declaration = &mut declarations[index];
                if node.kind.contains("comment") {
                    declaration.comments.push(node.range.clone());
                } else if node.kind == "plain_value" {
                    declaration.plain.push(node.range.clone());
                }
                Some(index)
            }
            None if node.kind == "declaration" && is_css(node.language) => {
                declarations.push(Declaration {
                    range: node.range.clone(),
                    comments: Vec::new(),
                    plain: Vec::new(),
                });
                Some(declarations.len() - 1)
            }
            None => None,
        };
        holder.push(own);
    }
    let mut found = Vec::new();
    for declaration in declarations {
        let start = declaration.range.start;
        let source = text.slice_to_string(declaration.range.clone());
        for color in css_colors(&source) {
            let range = start + color.range.start..start + color.range.end;
            let in_comment = (declaration.comments.iter())
                .any(|comment| comment.start < range.end && range.start < comment.end);
            let literal = &source[color.range.clone()];
            let named =
                literal.starts_with(|c: char| c.is_ascii_alphabetic()) && !literal.contains('(');
            if in_comment || (named && !declaration.plain.contains(&range)) {
                continue;
            }
            found.push(FoundColor {
                range,
                rgba: color.rgba,
            });
        }
    }
    found
}

/// The nodes of the host language's tree touching `range`, parents before
/// children, as [`InjectedNode`]s to read like a layer's.
fn host_nodes(syntax: &Syntax, range: Range<usize>) -> Vec<InjectedNode> {
    let mut nodes = Vec::new();
    let Some(tree) = syntax.tree() else {
        return nodes;
    };
    let language = syntax.language().name();
    let mut cursor = tree.walk();
    let mut parents: Vec<usize> = Vec::new();
    loop {
        let node = cursor.node();
        // Whatever follows in the tree starts later still.
        if node.start_byte() >= range.end {
            return nodes;
        }
        if node.end_byte() > range.start {
            nodes.push(InjectedNode {
                language,
                kind: node.kind().to_string(),
                named: node.is_named(),
                range: node.byte_range(),
                parent: parents.last().copied(),
            });
            if cursor.goto_first_child() {
                parents.push(nodes.len() - 1);
                continue;
            }
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return nodes;
            }
            parents.pop();
        }
    }
}

/// The bytes of `rows`, through the line break of the last.
fn row_bytes(text: &Text, rows: Range<u32>) -> Range<usize> {
    if rows.start >= text.row_count() {
        return text.len()..text.len();
    }
    let end = match rows.end < text.row_count() {
        true => text.row_start(rows.end),
        false => text.len(),
    };
    text.row_start(rows.start)..end
}

/// `swatches`, at their offsets in the buffer at `from`, moved to
/// `snapshot`: after text inserted at one, and gone where an edit replaced
/// or deleted text strictly around it. As an editor's own swatches follow.
fn follow(snapshot: &BufferSnapshot, from: Version, swatches: &[ColorSwatch]) -> Vec<ColorSwatch> {
    if from == snapshot.version() {
        return swatches.to_vec();
    }
    // Compacted away: the offsets can't be carried.
    if from < snapshot.oldest_version() {
        return Vec::new();
    }
    let mut offsets: Vec<Option<usize>> = (swatches.iter())
        .map(|swatch| Some(swatch.offset))
        .collect();
    for batch in snapshot.edits_since(from) {
        for slot in &mut offsets {
            let Some(offset) = *slot else { continue };
            let mut shift = 0isize;
            for edit in batch.iter() {
                let old = &edit.old;
                if old.start > offset || (old.start == offset && old.end > offset) {
                    break;
                }
                if old.end > offset {
                    *slot = None;
                    break;
                }
                shift = edit.new.end as isize - old.end as isize;
            }
            if slot.is_some() {
                *slot = Some(offset.saturating_add_signed(shift));
            }
        }
    }
    (offsets.into_iter().zip(swatches))
        .filter_map(|(offset, swatch)| {
            Some(ColorSwatch {
                offset: offset?,
                rgba: swatch.rgba,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "default_colors_tests.rs"]
mod tests;
