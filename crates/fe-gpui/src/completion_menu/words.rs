//! The words the completion menu offers
//! (docs/decisions/mgacx379x8-completion-as-you-type.md §2): those of the
//! buffer typed in and, with `editor.completion.words = "open_buffers"`,
//! of the other open editors' buffers in its language, scanned off the
//! main thread ([`fe_editor::word_completion`]). Each buffer's scan is kept
//! for its version, so a buffer not edited since isn't read again.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use fe_buffer::{BufferId, Version};
use fe_config::settings::{CompletionSettings, CompletionWords};
use fe_editor::{
    completion::{CompletionItem, CompletionTrigger},
    word_completion::{self, BufferWords, OtherWords},
};
use gpui::{App, AppContext as _, Context, Global, Task, WeakEntity};
use kiln_l10n::tr;

use crate::EditorView;

/// Every editor made, for the words of the open buffers: those dropped
/// are pruned as they're found.
#[derive(Default)]
struct OpenEditors {
    views: Vec<OpenEditor>,
    /// Each buffer's words, as scanned at a version.
    scans: HashMap<BufferId, (Version, Arc<BufferWords>)>,
}

impl Global for OpenEditors {}

#[derive(Clone)]
struct OpenEditor {
    view: WeakEntity<EditorView>,
    /// The buffer it was made over.
    buffer: BufferId,
    /// Its `[editor.completion]`, readable without the view, which may be
    /// being updated (splitting itself, say).
    settings: Rc<RefCell<Option<CompletionSettings>>>,
}

/// Counts `view`, being made, among the open editors. Made over a buffer an
/// open editor with `[editor.completion]` shows (a split of it, say), it
/// takes that editor's, until its host gives it its own.
pub(crate) fn register(view: &mut EditorView, cx: &mut Context<EditorView>) {
    let buffer = view.model.buffer().id();
    let weak = cx.weak_entity();
    let open = cx.default_global::<OpenEditors>();
    open.views.retain(|editor| editor.view.upgrade().is_some());
    let inherited = (open.views.iter())
        .filter(|editor| editor.buffer == buffer)
        .find_map(|editor| editor.settings.borrow().clone());
    let settings = view.completion_auto.shared_settings();
    open.views.push(OpenEditor {
        view: weak,
        buffer,
        settings,
    });
    if let Some(inherited) = inherited {
        view.set_completion_settings(&inherited);
    }
}

/// The language words are shared within: a buffer's syntax's, `None` for
/// plain text; unknown (`None` outside) while its language loads, which
/// shares with no other buffer.
fn language(view: &EditorView) -> Option<Option<&'static str>> {
    if view.is_loading_language() {
        return None;
    }
    Some(view.syntax().map(|syntax| syntax.language().name()))
}

/// What a scan is to read, or has read: a buffer at a version.
struct Source {
    buffer: BufferId,
    version: Version,
    /// The text to scan, unless it's scanned at this version already.
    text: Option<(fe_buffer::Text, usize)>,
    scanned: Option<Arc<BufferWords>>,
    detail: Option<String>,
}

/// Whether `trigger` asks for words too: as invoked, or typing more into a
/// list that's incomplete, or typing a trigger character that's a word
/// char, not after punctuation such as `.`, where a server's members are
/// wanted.
pub(crate) fn offers_words(trigger: &CompletionTrigger) -> bool {
    match trigger {
        CompletionTrigger::Invoked | CompletionTrigger::Incomplete => true,
        CompletionTrigger::Character(typed) => {
            (typed.chars().next_back()).is_some_and(word_completion::is_word_char)
        }
    }
}

impl EditorView {
    /// The words to offer at the caret, for `trigger`, as
    /// `editor.completion.words` says: `None` for none.
    pub(crate) fn ask_words(
        &self,
        trigger: &CompletionTrigger,
        cx: &mut Context<Self>,
    ) -> Option<Task<Vec<CompletionItem>>> {
        let words = self.completion_auto.settings.as_ref()?.words;
        if words == CompletionWords::Off || !offers_words(trigger) {
            return None;
        }
        let snapshot = self.model.snapshot().clone();
        let caret = self.model.primary_selection().head();
        let typed = word_completion::word_at(snapshot.text(), caret);
        let edit = typed.start..caret.max(typed.start);
        let typed = snapshot.text().slice_to_string(typed);
        let me = cx.entity_id();
        let wanted = language(self);
        let mut sources = vec![(
            snapshot.id(),
            snapshot.version(),
            snapshot.text().clone(),
            caret,
        )];
        let mut details = vec![None];
        if words == CompletionWords::OpenBuffers && wanted.is_some() {
            let views = {
                let open = cx.default_global::<OpenEditors>();
                open.views.retain(|editor| editor.view.upgrade().is_some());
                open.views.clone()
            };
            for OpenEditor { view, .. } in views {
                if view.entity_id() == me {
                    continue;
                }
                let Some(view) = view.upgrade() else {
                    continue;
                };
                let other = view.read(cx);
                let buffer = other.model.snapshot();
                if language(other) != wanted || (sources.iter()).any(|(id, ..)| *id == buffer.id())
                {
                    continue;
                }
                let detail = match other.model.path().and_then(|path| path.file_name()) {
                    Some(name) => tr!("gpui-completion-word-in", file = name.to_string_lossy()),
                    None => tr!("gpui-completion-word-untitled"),
                };
                let caret = other.model.primary_selection().head();
                sources.push((buffer.id(), buffer.version(), buffer.text().clone(), caret));
                details.push(Some(detail.to_string()));
            }
        }
        let open = cx.default_global::<OpenEditors>();
        let sources: Vec<Source> = (sources.into_iter().zip(details))
            .map(|((buffer, version, text, caret), detail)| {
                let scanned = (open.scans.get(&buffer))
                    // A long buffer's scan read only around its caret then.
                    .filter(|(at, words)| *at == version && words.covers(caret))
                    .map(|(_, words)| words.clone());
                Source {
                    buffer,
                    version,
                    text: scanned.is_none().then_some((text, caret)),
                    scanned,
                    detail,
                }
            })
            .collect();
        // Only the buffers asked about now stay kept.
        (open.scans).retain(|buffer, _| sources.iter().any(|source| source.buffer == *buffer));
        let kind = tr!("gpui-completion-word-kind").to_string();
        // Scanned off the main thread; the items, which aren't `Send`,
        // made back on it.
        let scan = cx.background_spawn(async move {
            let mut sources = sources;
            for source in &mut sources {
                if let Some((text, caret)) = source.text.take() {
                    source.scanned = Some(Arc::new(BufferWords::scan(&text, caret)));
                }
            }
            sources
        });
        Some(cx.spawn(async move |_, cx| {
            let sources = scan.await;
            let mut scanned = sources.into_iter().filter_map(|source| {
                let words = source.scanned?;
                Some((source.buffer, source.version, words, source.detail))
            });
            let Some((buffer, version, current, _)) = scanned.next() else {
                return Vec::new();
            };
            let others: Vec<_> = scanned.collect();
            let items = word_completion::word_items(
                &current,
                &typed,
                &(others.iter())
                    .map(|(.., words, detail)| OtherWords {
                        words: words.clone(),
                        detail: detail.clone(),
                    })
                    .collect::<Vec<_>>(),
                edit,
                &kind,
            );
            let mut kept = vec![(buffer, version, current)];
            kept.extend(
                (others.into_iter()).map(|(buffer, version, words, _)| (buffer, version, words)),
            );
            cx.update(|cx| keep_scans(cx, kept));
            items
        }))
    }
}

/// Keeps each buffer's scan at its version, for the next ask.
fn keep_scans(cx: &mut App, kept: Vec<(BufferId, Version, Arc<BufferWords>)>) {
    let open = cx.default_global::<OpenEditors>();
    for (buffer, version, words) in kept {
        open.scans.insert(buffer, (version, words));
    }
}
