//! Language servers' semantic tokens over the grammar's colours, as VS Code
//! shows them: asked for when a buffer opens on its server, 250 ms after its
//! last edit, and at once when the server asks for a refresh, each time with
//! the previous answer for a delta. `editor.semantic_highlighting` off shows
//! none and asks for none.
//!
//! Each buffer has one [`DecorationSet`] of `Foreground` decorations (and
//! `Underline` ones for the `mutable` modifier), shared
//! by its editors. An answer lands only for the buffer's current version; the
//! decorations already shown follow edits until the next answer.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use fe_buffer::{BufferId, Version};
use fe_editor::decorations::{Decoration, DecorationKind, DecorationSet, ProviderId, Tint};
use fe_gpui::EditorView;
use fe_lsp::{LspError, SemanticTokens, ServerId};
use gpui::{Context, Entity, EntityId, Task};

use crate::Workbench;

/// How long the buffer holds still before tokens are asked for.
const REST: Duration = Duration::from_millis(250);

/// How many times a request the server wasn't ready for is asked again.
const RETRIES: u32 = 8;

type Answer = Result<Option<SemanticTokens>, LspError>;

/// `token_type` with `modifiers`, as the name of a highlight the syntax
/// colours use; `None` for a server's own type, which leaves the grammar's
/// colour.
pub(super) fn highlight_name<M: AsRef<str>>(
    token_type: &str,
    modifiers: &[M],
) -> Option<&'static str> {
    let has = |name: &str| modifiers.iter().any(|m| m.as_ref() == name);
    let builtin = has("defaultLibrary");
    Some(match token_type {
        "namespace" => "module",
        "class" | "struct" | "enum" | "interface" | "type" | "typeParameter" => {
            if builtin {
                "type.builtin"
            } else {
                "type"
            }
        }
        "parameter" => "variable.parameter",
        "variable" => {
            if has("readonly") || has("static") {
                "constant"
            } else {
                "variable"
            }
        }
        "property" | "event" => "property",
        "enumMember" => "constant",
        "function" => {
            if builtin {
                "function.builtin"
            } else {
                "function"
            }
        }
        "method" => "function.method",
        "macro" => "function.macro",
        "decorator" => "attribute",
        "regexp" => "string.special",
        "keyword" => "keyword",
        "comment" => {
            if has("documentation") {
                "comment.documentation"
            } else {
                "comment"
            }
        }
        "string" => "string",
        "number" => "number",
        "operator" => "operator",
        _ => return None,
    })
}

struct Slot {
    set: Rc<RefCell<DecorationSet>>,
    /// Each editor of the buffer, with its registration of `set`.
    editors: HashMap<EntityId, (Entity<EditorView>, ProviderId)>,
    server: ServerId,
    /// The version last seen, to ask again when it changes.
    seen: Option<Version>,
    /// The latest answer, for a delta.
    previous: Option<SemanticTokens>,
    task: Option<Task<()>>,
}

/// Every shown buffer's tokens.
#[derive(Default)]
pub(super) struct Slots {
    slots: HashMap<BufferId, Slot>,
}

impl Slots {
    fn unregister(slot: &Slot, cx: &mut Context<Workbench>) {
        for (editor, id) in slot.editors.values() {
            editor.update(cx, |view, cx| {
                view.model_mut().remove_decoration_provider(*id);
                cx.notify();
            });
        }
    }
}

impl Workbench {
    /// Brings each buffer's tokens up to date with its version and server:
    /// run by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_semantic_tokens(&mut self, cx: &mut Context<Self>) {
        let on = self.lsp.semantic_highlighting_on.get();
        let mut wanted: HashMap<BufferId, (ServerId, Vec<Entity<EditorView>>)> = HashMap::new();
        if on && let Some(manager) = self.lsp.manager.clone() {
            for editor in self.open_editors(cx) {
                let buffer = editor.read(cx).model().buffer().id();
                let Some(server) = super::lock(&manager).server_of(buffer) else {
                    continue;
                };
                wanted
                    .entry(buffer)
                    .or_insert((server, Vec::new()))
                    .1
                    .push(editor);
            }
        }
        let gone: Vec<BufferId> = (self.lsp.semantic_tokens.slots.iter())
            .filter(|(buffer, slot)| {
                wanted
                    .get(buffer)
                    .is_none_or(|(server, _)| *server != slot.server)
            })
            .map(|(buffer, _)| *buffer)
            .collect();
        for buffer in gone {
            if let Some(slot) = self.lsp.semantic_tokens.slots.remove(&buffer) {
                Slots::unregister(&slot, cx);
            }
        }
        for (buffer, (server, editors)) in wanted {
            let slot = self
                .lsp
                .semantic_tokens
                .slots
                .entry(buffer)
                .or_insert_with(|| Slot {
                    set: Rc::new(RefCell::new(DecorationSet::new())),
                    editors: HashMap::new(),
                    server,
                    seen: None,
                    previous: None,
                    task: None,
                });
            let ids: Vec<EntityId> = editors.iter().map(|e| e.entity_id()).collect();
            slot.editors.retain(|id, _| ids.contains(id));
            for editor in &editors {
                if slot.editors.contains_key(&editor.entity_id()) {
                    continue;
                }
                // A split copies its original's providers: drop ours, to
                // register once.
                let copied: Vec<ProviderId> = slot.editors.values().map(|(_, id)| *id).collect();
                let set = slot.set.clone();
                let id = editor.update(cx, |view, _| {
                    for id in copied {
                        view.model_mut().remove_decoration_provider(id);
                    }
                    view.model_mut().add_decoration_provider(set)
                });
                slot.editors
                    .insert(editor.entity_id(), (editor.clone(), id));
            }
            let version = editors[0].read(cx).model().snapshot().version();
            if slot.seen == Some(version) {
                continue;
            }
            let first = slot.seen.is_none();
            slot.seen = Some(version);
            let delay = if first { Duration::ZERO } else { REST };
            self.ask_semantic_tokens_in(buffer, delay, 0, cx);
        }
    }

    /// Asks again, at once, for every buffer on `server`.
    pub(super) fn refresh_semantic_tokens(&mut self, server: ServerId, cx: &mut Context<Self>) {
        let buffers: Vec<BufferId> = (self.lsp.semantic_tokens.slots.iter())
            .filter(|(_, slot)| slot.server == server)
            .map(|(buffer, _)| *buffer)
            .collect();
        for buffer in buffers {
            self.ask_semantic_tokens_in(buffer, Duration::ZERO, 0, cx);
        }
    }

    /// Has `buffer` ask after `delay`, in place of any request waiting or
    /// in flight; `attempt` counts the asks a not-ready server turned away.
    fn ask_semantic_tokens_in(
        &mut self,
        buffer: BufferId,
        delay: Duration,
        attempt: u32,
        cx: &mut Context<Self>,
    ) {
        let task = cx.spawn(async move |workbench, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(Some((version, reply))) = workbench.update(cx, |workbench, cx| {
                workbench.send_semantic_tokens(buffer, cx)
            }) else {
                return;
            };
            let answer = reply.await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.semantic_tokens_landed(buffer, version, attempt, answer, cx)
                })
                .ok();
        });
        if let Some(slot) = self.lsp.semantic_tokens.slots.get_mut(&buffer) {
            slot.task = Some(task);
        }
    }

    #[allow(clippy::type_complexity)]
    fn send_semantic_tokens(
        &mut self,
        buffer: BufferId,
        cx: &mut Context<Self>,
    ) -> Option<(
        Version,
        std::pin::Pin<Box<dyn std::future::Future<Output = Answer> + Send>>,
    )> {
        let manager = self.lsp.manager.clone()?;
        let slot = self.lsp.semantic_tokens.slots.get(&buffer)?;
        let (editor, _) = slot.editors.values().next()?;
        let version = editor.read(cx).model().snapshot().version();
        let reply = super::lock(&manager).semantic_tokens(buffer, version, slot.previous.as_ref());
        Some((version, Box::pin(reply)))
    }

    fn semantic_tokens_landed(
        &mut self,
        buffer: BufferId,
        version: Version,
        attempt: u32,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        if !self.lsp.semantic_highlighting_on.get() {
            return;
        }
        let Some(slot) = self.lsp.semantic_tokens.slots.get_mut(&buffer) else {
            return;
        };
        let tokens = match answer {
            Ok(Some(tokens)) => tokens,
            Ok(None) => return,
            Err(LspError::Retry) => {
                // The server wasn't ready, or reopened the document.
                slot.previous = None;
                if attempt < RETRIES {
                    self.ask_semantic_tokens_in(buffer, REST, attempt + 1, cx);
                }
                return;
            }
            Err(error) => {
                log::debug!("no semantic tokens: {error}");
                return;
            }
        };
        let Some((editor, _)) = slot.editors.values().next().cloned() else {
            return;
        };
        let mut items: Vec<Decoration> = Vec::new();
        for token in &tokens.tokens {
            if let Some(name) = highlight_name(&token.token_type, &token.modifiers) {
                items.push(Decoration::new(
                    token.range.clone(),
                    DecorationKind::Foreground(Arc::from(name)),
                    Tint::Muted,
                ));
            }
            // As VS Code underlines what rust-analyzer marks `mutable`.
            if token.modifiers.iter().any(|m| m.as_ref() == "mutable") {
                items.push(Decoration::new(
                    token.range.clone(),
                    DecorationKind::Underline { wavy: false },
                    Tint::Muted,
                ));
            }
        }
        slot.previous = Some(tokens);
        let snapshot = editor.read(cx).model().snapshot().clone();
        // An answer for an older version is dropped.
        if snapshot.version() != version {
            return;
        }
        slot.set.borrow_mut().apply(&snapshot, version, items);
        for (editor, _) in slot.editors.values() {
            editor.update(cx, |_, cx| cx.notify());
        }
    }
}

#[cfg(test)]
#[path = "lsp_semantic_tokens_tests.rs"]
mod tests;
