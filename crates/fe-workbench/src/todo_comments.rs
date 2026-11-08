//! `TODO:`-style keywords in comments, found for `editor.todo_comments`.
//!
//! [`scan`] is pure: it takes the bytes that are comments and says where the
//! keywords are. [`comment_ranges`] finds those bytes from the parse an editor
//! already has. Drawing them is [`Todos`], below.

use std::{cell::RefCell, collections::HashMap, collections::HashSet, ops::Range, rc::Rc};

use fe_editor::decorations::{
    Decoration, DecorationKind, DecorationProvider, DecorationSet, ProviderId, Tint,
};
use fe_gpui::{EditorSettings, EditorView};
use fe_syntax::Syntax;
use fe_text::Text;
use gpui::{AppContext as _, Context, Entity, EntityId, Task};

use crate::{
    Workbench,
    spell_squiggles::{REST, add_rows, language_of, rows_to_check, unchecked_rows},
};

/// What a keyword is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Todo,
    Fix,
    Hack,
    Warn,
    Perf,
    Note,
    Test,
}

/// The words that are keywords, with the kind each is. Only in capitals.
#[derive(Clone, Debug)]
pub struct Keywords {
    words: Vec<(&'static str, Kind)>,
}

impl Default for Keywords {
    /// The defaults of todo-comments.nvim.
    fn default() -> Self {
        let words = [
            ("TODO", Kind::Todo),
            ("FIX", Kind::Fix),
            ("FIXME", Kind::Fix),
            ("BUG", Kind::Fix),
            ("FIXIT", Kind::Fix),
            ("ISSUE", Kind::Fix),
            ("HACK", Kind::Hack),
            ("XXX", Kind::Hack),
            ("WARN", Kind::Warn),
            ("WARNING", Kind::Warn),
            ("PERF", Kind::Perf),
            ("OPTIMIZE", Kind::Perf),
            ("PERFORMANCE", Kind::Perf),
            ("NOTE", Kind::Note),
            ("INFO", Kind::Note),
            ("TEST", Kind::Test),
            ("TESTING", Kind::Test),
            ("PASSED", Kind::Test),
            ("FAILED", Kind::Test),
        ];
        Self {
            words: words.to_vec(),
        }
    }
}

impl Keywords {
    fn kind_of(&self, word: &str) -> Option<Kind> {
        (self.words.iter())
            .find(|(known, _)| *known == word)
            .map(|(_, kind)| *kind)
    }
}

/// One keyword found in a comment. Ranges are bytes of the text scanned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoHit {
    /// The keyword alone.
    pub keyword_range: Range<usize>,
    pub kind: Kind,
    /// The keyword through its colon, with the user between if there is one.
    pub tag_range: Range<usize>,
    /// The name in `TODO(name):`, without the parentheses.
    pub user: Option<Range<usize>>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The keywords in `comment_ranges` of `text`, in order: one in capitals at a
/// word boundary, followed by `:` or by `(user):`.
pub fn scan(text: &Text, comment_ranges: &[Range<usize>], keywords: &Keywords) -> Vec<TodoHit> {
    let mut hits = Vec::new();
    for range in comment_ranges {
        let range = range.start.min(text.len())..range.end.min(text.len());
        let source = text.slice_to_string(range.clone());
        scan_source(&source, range.start, keywords, &mut hits);
    }
    hits
}

/// The nearest `)` or line break at or after some offset, found once and kept
/// for the later offsets it is still ahead of, so that a line of unclosed
/// `TODO(` candidates is read in linear time, however long a name may be.
struct Closing {
    /// The offset the search began at.
    from: usize,
    /// What it found: `None` for neither to the end of the text.
    found: Option<(usize, char)>,
}

impl Closing {
    fn at_or_after(
        cache: &mut Option<Closing>,
        source: &str,
        from: usize,
    ) -> Option<(usize, char)> {
        if let Some(known) = cache
            && known.from <= from
            && known.found.is_none_or(|(at, _)| at >= from)
        {
            return known.found;
        }
        let found = source[from..]
            .char_indices()
            .find(|(_, c)| matches!(c, ')' | '\n'))
            .map(|(at, c)| (from + at, c));
        *cache = Some(Closing { from, found });
        found
    }
}

fn scan_source(source: &str, base: usize, keywords: &Keywords, hits: &mut Vec<TodoHit>) {
    let mut previous: Option<char> = None;
    let mut closing = None;
    let mut chars = source.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if !c.is_ascii_uppercase() || previous.is_some_and(is_word) {
            previous = Some(c);
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(at, next)) = chars.peek() {
            if !is_word(next) {
                break;
            }
            end = at + next.len_utf8();
            chars.next();
        }
        previous = source[..end].chars().next_back();
        let Some(kind) = keywords.kind_of(&source[start..end]) else {
            continue;
        };
        let (user, after) = match source[end..].strip_prefix('(') {
            Some(_) => match Closing::at_or_after(&mut closing, source, end + 1) {
                Some((close, ')')) if close > end + 1 => {
                    (Some(base + end + 1..base + close), close + 1)
                }
                _ => continue,
            },
            None => (None, end),
        };
        if !source[after..].starts_with(':') {
            continue;
        }
        hits.push(TodoHit {
            keyword_range: base + start..base + end,
            kind,
            tag_range: base + start..base + after + 1,
            user,
        });
        // Nothing inside a tag is a tag of its own.
        while chars.next_if(|(at, _)| *at <= after).is_some() {}
        previous = Some(':');
    }
}

/// The bytes of the comments in `rows` of `text`, as `syntax` highlights them,
/// sorted and apart. None when highlighting didn't finish: a language that
/// isn't compiled yet, or a query cut short. Markdown, plain text and a buffer
/// too big for spell checking have none.
pub fn comment_ranges(
    syntax: Option<&Syntax>,
    text: &Text,
    rows: Range<u32>,
) -> Option<Vec<Range<usize>>> {
    let Some(syntax) = syntax else {
        return Some(Vec::new());
    };
    if syntax.language().name() == "markdown"
        || syntax.failed()
        || syntax.tree().is_none()
        || syntax.needs_parse()
        || text.len() > crate::spell_ranges::MAX_SPELLABLE_BYTES
        || rows.is_empty()
        || rows.start >= text.row_count()
    {
        return Some(Vec::new());
    }
    let end = match rows.end < text.row_count() {
        true => text.row_start(rows.end),
        false => text.len(),
    };
    let highlights = syntax.checked_highlights(text.row_start(rows.start)..end);
    if !highlights.complete {
        return None;
    }
    let mut spans: Vec<_> = (highlights.spans.into_iter())
        .filter(|span| matches!(span.highlight.name(), "comment" | "comment.documentation"))
        .collect();
    spans.sort_by_key(|span| span.range.start);
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for span in spans {
        match ranges.last_mut() {
            Some(last) if span.range.start <= last.end => {
                last.end = last.end.max(span.range.end);
            }
            _ => ranges.push(span.range),
        }
    }
    Some(ranges)
}

/// How many more times rows are scanned when their highlights didn't finish.
const RETRIES: u32 = 8;

impl Kind {
    /// The highlight name whose theme colour the badge's text takes. A
    /// theme without it leaves the text in the kind's [`tint`](Self::tint).
    pub fn highlight_name(self) -> &'static str {
        match self {
            Kind::Todo => "comment.todo",
            Kind::Fix => "comment.fixme",
            Kind::Hack => "comment.hack",
            Kind::Note | Kind::Test => "comment.note",
            Kind::Perf => "comment.perf",
            Kind::Warn => "comment.warn",
        }
    }

    /// The tint of a keyword's badge: from the theme's diagnostic palette.
    pub fn tint(self) -> Tint {
        match self {
            Kind::Fix => Tint::Error,
            Kind::Hack | Kind::Warn => Tint::Warning,
            Kind::Todo => Tint::Information,
            Kind::Note | Kind::Test => Tint::Hint,
            Kind::Perf => Tint::Perf,
        }
    }
}

/// An editor's keyword badges and what's known of its rows.
struct Slot {
    editor: Entity<EditorView>,
    provider: ProviderId,
    set: Rc<RefCell<DecorationSet>>,
    version: fe_buffer::Version,
    language: Option<&'static str>,
    /// The rows scanned, sorted and apart.
    scanned: Vec<Range<u32>>,
    task: Option<Task<()>>,
    retries: u32,
}

/// The workbench's keyword highlighting, behind `editor.todo_comments`.
pub(crate) struct Todos {
    on: bool,
    keywords: Keywords,
    slots: HashMap<EntityId, Slot>,
    /// Every provider given out since they last all went: a split copies its
    /// original's, so those go from an editor before it gets its own.
    issued: Vec<ProviderId>,
    /// Makes every scan find highlights unfinished.
    #[cfg(test)]
    pub(crate) incomplete: bool,
}

impl Todos {
    pub(crate) fn new(settings: &EditorSettings) -> Self {
        Self {
            on: settings.todo_comments,
            keywords: Keywords::default(),
            slots: HashMap::new(),
            issued: Vec::new(),
            #[cfg(test)]
            incomplete: false,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.on
    }

    /// How many editors have a provider.
    #[cfg(test)]
    pub(crate) fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

impl Workbench {
    /// Follows reloaded settings: switching it off takes every badge away.
    pub(crate) fn reload_todo_comments(
        &mut self,
        settings: &EditorSettings,
        cx: &mut Context<Self>,
    ) {
        if settings.todo_comments == self.todos.on {
            return;
        }
        self.todos.on = settings.todo_comments;
        // Every open editor, not just those with a slot: a split holds its
        // original's provider until the next sync gives it a slot.
        let issued = std::mem::take(&mut self.todos.issued);
        for editor in self.open_editors(cx) {
            editor.update(cx, |view, cx| {
                for id in &issued {
                    view.model_mut().remove_decoration_provider(*id);
                }
                cx.notify();
            });
        }
        for (_, slot) in self.todos.slots.drain() {
            slot.editor.update(cx, |view, cx| {
                view.model_mut().remove_decoration_provider(slot.provider);
                cx.notify();
            });
        }
        cx.notify();
    }

    /// The keyword badges drawn in `editor`, in its model's text, in order.
    pub fn todo_decorations(&self, editor: &Entity<EditorView>, cx: &gpui::App) -> Vec<Decoration> {
        let Some(slot) = self.todos.slots.get(&editor.entity_id()) else {
            return Vec::new();
        };
        let snapshot = editor.read(cx).model().snapshot();
        let mut out = Vec::new();
        slot.set
            .borrow()
            .decorations(snapshot, 0..snapshot.len(), &mut out);
        out.retain(|it| !it.range.is_empty());
        out
    }

    /// Brings the badges of what's shown up to date: run after each draw.
    pub(crate) fn sync_todo_comments(&mut self, cx: &mut Context<Self>) {
        if !self.todos.on {
            return;
        }
        let open: HashSet<EntityId> = (self.open_editors(cx).iter())
            .map(Entity::entity_id)
            .collect();
        self.todos.slots.retain(|id, _| open.contains(id));
        let shown: Vec<_> = (self.panes(cx).iter())
            .filter_map(|pane| pane.read(cx).active_editor().cloned())
            .collect();
        for editor in shown {
            let id = editor.entity_id();
            if !self.todos.slots.contains_key(&id) {
                self.attach_todo_comments(editor.clone(), cx);
            }
            let view = editor.read(cx);
            let version = view.model().snapshot().version();
            let language = language_of(view);
            let rows = rows_to_check(view);
            let Some(slot) = self.todos.slots.get_mut(&id) else {
                continue;
            };
            if slot.language != language {
                slot.set.borrow_mut().clear();
            }
            if slot.language != language || slot.version != version {
                slot.language = language;
                slot.version = version;
                slot.scanned.clear();
                slot.retries = 0;
                slot.task = Some(wait_then_scan(id, Vec::new(), cx));
            } else if slot.task.is_none()
                && rows.is_some_and(|rows| !unchecked_rows(&rows, &slot.scanned).is_empty())
            {
                slot.task = Some(wait_then_scan(id, Vec::new(), cx));
            }
        }
    }

    fn attach_todo_comments(&mut self, editor: Entity<EditorView>, cx: &mut Context<Self>) {
        let copied = self.todos.issued.clone();
        let set = Rc::new(RefCell::new(DecorationSet::new()));
        let provider = editor.update(cx, |view, _| {
            for id in copied {
                view.model_mut().remove_decoration_provider(id);
            }
            view.model_mut().add_decoration_provider(set.clone())
        });
        self.todos.issued.push(provider);
        let view = editor.read(cx);
        let slot = Slot {
            provider,
            set,
            version: view.model().snapshot().version(),
            language: language_of(view),
            scanned: Vec::new(),
            task: None,
            retries: 0,
            editor: editor.clone(),
        };
        self.todos.slots.insert(editor.entity_id(), slot);
    }

    /// The wait is over: scans the rows in view.
    fn scan_todo_comments(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let keywords = self.todos.keywords.clone();
        #[cfg(test)]
        let incomplete = self.todos.incomplete;
        #[cfg(not(test))]
        let incomplete = false;
        let Some(slot) = self.todos.slots.get_mut(&id) else {
            return;
        };
        slot.task = None;
        let view = slot.editor.read(cx);
        let model = view.model();
        let snapshot = model.snapshot().clone();
        let syntax = view.syntax();
        let behind = model.buffer().version() != snapshot.version()
            || syntax.is_some_and(|it| it.version() != snapshot.version() || it.needs_parse());
        if view.is_loading_language() || behind {
            return;
        }
        let Some(window) = rows_to_check(view) else {
            return;
        };
        let text = snapshot.text();
        let mut comments = Vec::new();
        for rows in &window {
            let found = match incomplete {
                true => None,
                false => comment_ranges(syntax, text, rows.clone()),
            };
            match found {
                Some(found) => comments.extend(found),
                None => {
                    let missing = syntax.map(|it| it.take_missing_languages());
                    if slot.retries < RETRIES {
                        slot.retries += 1;
                        slot.task = Some(wait_then_scan(id, missing.unwrap_or_default(), cx));
                    } else {
                        // Out of tries: what's there is of text that has
                        // changed since, so it goes, and these rows count
                        // as done until the next edit starts over.
                        slot.set.borrow_mut().clear();
                        slot.scanned.clear();
                        for rows in window {
                            add_rows(&mut slot.scanned, rows);
                        }
                        let editor = slot.editor.clone();
                        editor.update(cx, |_, cx| cx.notify());
                    }
                    return;
                }
            }
        }
        comments.sort_by_key(|range| range.start);
        comments.dedup();
        let items: Vec<_> = scan(text, &comments, &keywords)
            .into_iter()
            .flat_map(|hit| {
                let tint = hit.kind.tint();
                [
                    Decoration::new(hit.tag_range.clone(), DecorationKind::Highlight, tint),
                    Decoration::new(
                        hit.tag_range,
                        DecorationKind::Foreground(hit.kind.highlight_name().into()),
                        tint,
                    ),
                ]
            })
            .collect();
        slot.set
            .borrow_mut()
            .apply(&snapshot, snapshot.version(), items);
        slot.scanned.clear();
        for rows in window {
            add_rows(&mut slot.scanned, rows);
        }
        slot.retries = 0;
        let editor = slot.editor.clone();
        editor.update(cx, |_, cx| cx.notify());
    }
}

/// Scans editor `id` once it has held still. A scan can find `missing`
/// languages where nothing is drawn, which leaves none to compile them.
fn wait_then_scan(
    id: EntityId,
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
        cx.background_executor().timer(REST).await;
        workbench
            .update(cx, |workbench, cx| workbench.scan_todo_comments(id, cx))
            .ok();
    })
}
