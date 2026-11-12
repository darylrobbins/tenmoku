//! Formatting on type and on paste, as VS Code's `editor.formatOnType` and
//! `editor.formatOnPaste` do, through the buffer's language server only (an
//! external formatter never runs for either).
//!
//! - **On type.** With `editor.format_on_type` on, a character typed from
//!   the keyboard ([`EditorInputEvent::Typed`]: not a paste, an input
//!   method's composition, nor a vim replay or a map; Enter is `\n`) that
//!   is one of the buffer's on-type formatting triggers asks the server
//!   (`textDocument/onTypeFormatting`) at the caret, with one caret only.
//! - **On paste.** With `editor.format_on_paste` on, a paste
//!   ([`EditorInputEvent::Pasted`]: `editor.paste`, vim's `p` and `P`, the
//!   rows put and not a line break added before them) with
//!   one caret, whose text spans more than one row, asks for range
//!   formatting (`textDocument/rangeFormatting`) of exactly the pasted
//!   range, if the server has the provider.
//! - **The edits.** Asked for at the buffer's version just after the typing
//!   or the paste, and applied only if the buffer is still at it, under the
//!   same name, with the setting still on: anything typed, undone or saved
//!   elsewhere meanwhile drops them, silently. They're added to the undo
//!   step the typing or paste landed in ([`Buffer::edit_in`]), so one undo
//!   takes back both. A newer request from the same editor drops the one
//!   in flight. The options are the editor's, as for Format Document.
//! - **Failures.** A server without the provider, one that times out or
//!   fails, or edits that overlap, are logged, never shown.
//!
//! [`Buffer::edit_in`]: fe_buffer::Buffer::edit_in

use std::{collections::HashMap, path::PathBuf};

use fe_buffer::{Origin, TransactionId, Version};
use fe_editor::IndentStyle;
use fe_gpui::{EditorView, commands::EditorInputEvent};
use fe_lsp::{
    LspError,
    formatting::{Edits, FormatOptions},
};
use futures::FutureExt as _;
use gpui::{App, AppContext as _, Context, Entity, EntityId, Subscription, Task, WeakEntity};

use kiln_l10n::verbatim;

use crate::Workbench;

/// Every open editor's input, heard, and the request each has in flight.
#[derive(Default)]
pub(super) struct OnType {
    heard: HashMap<EntityId, Subscription>,
    /// Dropping one cancels it.
    tasks: HashMap<EntityId, Task<()>>,
}

/// What a request is for: the buffer as the typing or paste left it.
struct Target {
    editor: WeakEntity<EditorView>,
    version: Version,
    path: Option<PathBuf>,
    /// The undo step the edits join.
    transaction: TransactionId,
    /// Whether it's on type, else on paste: which setting must still be on.
    on_type: bool,
}

impl Workbench {
    /// Hears every open editor's typing and pasting, and forgets closed
    /// ones.
    pub(super) fn sync_format_on_type(
        &mut self,
        editors: &[Entity<EditorView>],
        cx: &mut Context<Self>,
    ) {
        let open: Vec<EntityId> = editors.iter().map(Entity::entity_id).collect();
        let state = &mut self.lsp.on_type;
        state.heard.retain(|id, _| open.contains(id));
        state.tasks.retain(|id, _| open.contains(id));
        for editor in editors {
            if self.lsp.on_type.heard.contains_key(&editor.entity_id()) {
                continue;
            }
            let subscription = cx.subscribe(editor, |workbench, editor, event, cx| {
                workbench.format_on_input(&editor, event, cx);
            });
            (self.lsp.on_type.heard).insert(editor.entity_id(), subscription);
        }
    }

    /// Asks `editor`'s server to format after `event`, if its setting is
    /// on: see the [module](self).
    fn format_on_input(
        &mut self,
        editor: &Entity<EditorView>,
        event: &EditorInputEvent,
        cx: &mut Context<Self>,
    ) {
        let on_type = matches!(event, EditorInputEvent::Typed { .. });
        if !self.format_on_input_allowed(on_type) {
            return;
        }
        let Some(manager) = self.lsp.manager() else {
            return;
        };
        let reply = {
            let view = editor.read(cx);
            let model = view.model();
            if model.selections().len() != 1 {
                return;
            }
            let buffer = model.buffer();
            let id = buffer.id();
            let options = FormatOptions {
                tab_size: model.tab_size() as u32,
                insert_spaces: !matches!(model.indent_style(), Some(IndentStyle::Tabs)),
            };
            let manager = super::lock(&manager);
            let (version, reply) = match event {
                EditorInputEvent::Typed { text, version } => {
                    let selection = model.primary_selection();
                    let Some(ch) = text.chars().next_back() else {
                        return;
                    };
                    // Only a trigger is asked about, so typing anything else
                    // costs no more than this.
                    if !selection.range.is_empty()
                        || !manager.on_type_formatting_triggers(id).contains(&ch)
                    {
                        return;
                    }
                    let at = selection.head();
                    let reply = manager.on_type_formatting(id, *version, at, ch, options);
                    (*version, reply.boxed())
                }
                EditorInputEvent::Pasted {
                    range,
                    rows,
                    version,
                } => {
                    if *rows < 2 {
                        return;
                    }
                    let reply = manager.format(id, *version, Some(range.clone()), options);
                    (*version, reply.boxed())
                }
            };
            // Sent at once: the event comes as the buffer's at `version`.
            if buffer.version() != version {
                return;
            }
            let transaction = buffer
                .history_times(|origin| *origin == Origin::User)
                .into_iter()
                .rev()
                .find(|time| !time.undone)
                .map(|time| time.transaction);
            let Some(transaction) = transaction else {
                return;
            };
            let target = Target {
                editor: editor.downgrade(),
                version,
                path: model.path().map(ToOwned::to_owned),
                transaction,
                on_type,
            };
            (target, cx.background_spawn(reply))
        };
        let (target, reply) = reply;
        let task = cx.spawn(async move |this, cx| {
            let edits = match reply.await {
                Ok(Some(edits)) => edits,
                Ok(None) => return log::debug!("{}: no formatter", target.what()),
                Err(LspError::Unavailable(why)) if why == "buffer changed" => return,
                Err(error) => return log::debug!("{}: {error}", target.what()),
            };
            let _ = this.update(cx, |workbench, cx| {
                if workbench.format_on_input_allowed(target.on_type) {
                    target.apply(edits, cx);
                }
            });
        });
        (self.lsp.on_type.tasks).insert(editor.entity_id(), task);
    }

    /// Whether the setting for formatting on type, or on paste, is on.
    fn format_on_input_allowed(&self, on_type: bool) -> bool {
        match on_type {
            true => self.lsp.format_on_type_on.get(),
            false => self.lsp.format_on_paste_on.get(),
        }
    }
}

impl Target {
    /// Which it is, as the debug log names it.
    fn what(&self) -> &'static str {
        match self.on_type {
            true => verbatim!("format on type"),
            false => verbatim!("format on paste"),
        }
    }

    /// Adds `edits` to the typing's or paste's undo step, unless the buffer
    /// has moved on since.
    fn apply(&self, edits: Edits, cx: &mut App) {
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        editor.update(cx, |view, cx| {
            let model = view.model_mut();
            model.sync_display();
            // Saved under another name since: the edits were for the
            // document under the old one.
            if model.path() != self.path.as_deref() {
                return;
            }
            let mut buffer = model.buffer_mut();
            if buffer.version() != self.version {
                return;
            }
            let text = buffer.current().text();
            let edits: Edits = (merge_inserts(edits).into_iter())
                .filter(|(range, new_text)| !text.range_eq(range.clone(), new_text))
                .collect();
            if let Err(error) = buffer.edit_in(self.transaction, edits) {
                log::debug!("{}: {error}", self.what());
            }
            drop(buffer);
            cx.notify();
        });
    }
}

/// `edits`, sorted and apart, with inserts at one place joined in their
/// order, as one batch takes them.
fn merge_inserts(edits: Edits) -> Edits {
    let mut merged: Edits = Vec::with_capacity(edits.len());
    for (range, text) in edits {
        match merged.last_mut() {
            Some((last, last_text)) if last.start == last.end && range == *last => {
                last_text.push_str(&text);
            }
            _ => merged.push((range, text)),
        }
    }
    merged
}

#[cfg(test)]
#[path = "lsp_format_on_type_tests.rs"]
mod lsp_format_on_type_tests;
