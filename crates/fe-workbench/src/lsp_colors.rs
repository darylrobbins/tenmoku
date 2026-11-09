//! Language servers' document colours, as VS Code's
//! `editor.colorDecorators` has them: asked for when a buffer opens on its
//! server and 500 ms after its last edit, and handed to each of its
//! editors as colour swatches ([`EditorView::set_color_swatches`]), a
//! square in the colour before each.
//!
//! An answer replaces a buffer's swatches only if it is for the buffer's
//! current version and is the latest asked; until one lands the old
//! swatches stay, following the edits. An empty answer, an error, or a
//! server without colours clears them; `editor.color_decorators` off
//! clears them and asks for nothing.
//!
//! Without a server's colours, Tenmoku finds its own ([`default_colors`]),
//! as `editor.default_color_decorators` says; which an editor shows is
//! [`Workbench::show_color_swatches`]'s to say.

use std::{collections::HashMap, time::Duration};

use fe_buffer::{BufferId, Version};
use fe_config::settings::DefaultColorDecorators;
use fe_gpui::{ColorSwatch, EditorView};
use fe_lsp::{ColorInformation, IncarnationId, LspError, ServerId, SessionId};
use gpui::{Context, Entity, EntityId, Task};

use crate::Workbench;

#[path = "default_colors.rs"]
mod default_colors;

/// How long a buffer holds still before its colours are asked for again,
/// and how long a server that asks for a retry waits.
const REST: Duration = Duration::from_millis(500);

/// How many times a request is retried on the server's asking.
const RETRIES: u32 = 8;

/// The opening of a document a server has: its session and incarnation.
type Identity = (SessionId, IncarnationId);

/// What a request answers.
type Answer = Result<Option<Vec<ColorInformation>>, LspError>;

struct Slot {
    /// The editors given the swatches.
    editors: HashMap<EntityId, Entity<EditorView>>,
    /// The server the buffer is on while colours are wanted.
    server: Option<ServerId>,
    /// The opening the server had when colours were last asked for: a new
    /// one (a restarted server, a document reopened) is asked again.
    identity: Option<Identity>,
    /// The version last seen, to ask again when it changes.
    key: Option<Version>,
    /// The version of the latest request.
    asked: Option<Version>,
    /// The server that said it has no provider.
    no_provider: Option<ServerId>,
    /// The colours of the latest answer shown, at the version asked, when
    /// it had some.
    answered: Option<(Version, Vec<ColorSwatch>)>,
    /// The wait, then the request.
    task: Option<Task<()>>,
}

impl Slot {
    fn new() -> Self {
        Self {
            editors: HashMap::new(),
            server: None,
            identity: None,
            key: None,
            asked: None,
            no_provider: None,
            answered: None,
            task: None,
        }
    }
}

/// Every open buffer's colours.
#[derive(Default)]
pub(super) struct Colors {
    slots: HashMap<BufferId, Slot>,
    /// The colours Tenmoku finds itself.
    defaults: default_colors::Defaults,
}

impl Workbench {
    /// Brings each buffer's swatches up to date with its text and server:
    /// run by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_colors(&mut self, cx: &mut Context<Self>) {
        // With `always`, as if the server had no colours.
        let on = self.lsp.color_decorators_on.get()
            && self.lsp.colors.defaults.mode != DefaultColorDecorators::Always;
        let mut wanted: HashMap<BufferId, (Version, Vec<Entity<EditorView>>)> = HashMap::new();
        for editor in self.open_editors(cx) {
            let model = editor.read(cx).model();
            let (buffer, version) = (model.buffer().id(), model.snapshot().version());
            (wanted.entry(buffer))
                .or_insert_with(|| (version, Vec::new()))
                .1
                .push(editor);
        }
        let gone: Vec<BufferId> = (self.lsp.colors.slots.keys())
            .filter(|buffer| !wanted.contains_key(buffer))
            .copied()
            .collect();
        for buffer in gone {
            self.drop_colors(buffer, cx);
        }
        // The editors whose swatches may change.
        let mut changed = Vec::new();
        for (buffer, (version, editors)) in wanted {
            let server = (self.lsp.manager.as_ref())
                .and_then(|manager| super::lock(manager).server_of(buffer))
                .filter(|_| on);
            let Some(server) = server else {
                self.drop_colors(buffer, cx);
                continue;
            };
            let identity = (self.lsp.manager.as_ref())
                .and_then(|manager| super::lock(manager).active_identity(buffer, server));
            // Nothing is asked of a document the server hasn't been told of;
            // the next draw looks again once it activates.
            if identity.is_none() {
                continue;
            }
            let slot = self
                .lsp
                .colors
                .slots
                .entry(buffer)
                .or_insert_with(Slot::new);
            if slot.server != Some(server) || slot.identity != identity {
                // Another server's, or opening's, colours aren't this one's.
                let moved = slot.server.is_some_and(|was| was != server)
                    || (slot.identity.is_some() && slot.identity != identity);
                slot.server = Some(server);
                slot.identity = identity;
                slot.key = None;
                slot.no_provider = None;
                if moved {
                    slot.asked = None;
                    slot.task = None;
                    slot.answered = None;
                    changed.extend(slot.editors.values().cloned());
                }
            }
            // Editors closed are forgotten; new ones get the swatches the
            // others have.
            slot.editors
                .retain(|id, _| editors.iter().any(|editor| editor.entity_id() == *id));
            for editor in editors {
                if let std::collections::hash_map::Entry::Vacant(entry) =
                    slot.editors.entry(editor.entity_id())
                {
                    entry.insert(editor.clone());
                    changed.push(editor);
                }
            }
            if slot.no_provider == Some(server) || slot.key == Some(version) {
                continue;
            }
            // The first ask goes at once; later ones wait for the text to
            // hold still.
            let first = slot.key.is_none();
            slot.key = Some(version);
            let delay = if first { Duration::ZERO } else { REST };
            self.ask_colors_in(buffer, delay, 0, cx);
        }
        for editor in changed {
            self.show_color_swatches(&editor, cx);
        }
        self.sync_default_colors(cx);
    }

    /// Forgets `buffer`'s colours and takes the server's swatches from its
    /// editors.
    fn drop_colors(&mut self, buffer: BufferId, cx: &mut Context<Self>) {
        let Some(slot) = self.lsp.colors.slots.remove(&buffer) else {
            return;
        };
        for editor in slot.editors.values() {
            self.show_color_swatches(editor, cx);
        }
    }

    /// Has `buffer` ask for its colours after `delay`, in place of any
    /// request waiting or in flight; the `attempt`th retry, from 0.
    fn ask_colors_in(
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
            let Ok(Some((version, identity, reply))) =
                workbench.update(cx, |workbench, cx| workbench.send_colors(buffer, cx))
            else {
                return;
            };
            let answer = reply.await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.colors_landed(buffer, version, identity, attempt, answer, cx)
                })
                .ok();
        });
        if let Some(slot) = self.lsp.colors.slots.get_mut(&buffer) {
            slot.task = Some(task);
        }
    }

    /// The request for `buffer`'s colours at its version now, sent.
    #[allow(clippy::type_complexity)]
    fn send_colors(
        &mut self,
        buffer: BufferId,
        cx: &mut Context<Self>,
    ) -> Option<(
        Version,
        Option<Identity>,
        std::pin::Pin<Box<dyn std::future::Future<Output = Answer> + Send>>,
    )> {
        let manager = self.lsp.manager.clone()?;
        let slot = self.lsp.colors.slots.get_mut(&buffer)?;
        let editor = slot.editors.values().next()?;
        let version = editor.read(cx).model().snapshot().version();
        slot.asked = Some(version);
        let reply = super::lock(&manager).document_colors(buffer, version);
        Some((version, slot.identity, Box::pin(reply)))
    }

    /// Hands `answer`, the reply to the request for `buffer` at `version`,
    /// made on the opening `identity`, to its editors as swatches, if it is
    /// still the latest, the server still has that opening, and the text
    /// hasn't moved on. A server's asking for a retry is asked again, as
    /// the `attempt`th retry, after [`REST`], up to [`RETRIES`] times;
    /// past those, the swatches clear as for any other error.
    fn colors_landed(
        &mut self,
        buffer: BufferId,
        version: Version,
        identity: Option<Identity>,
        attempt: u32,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        if !self.lsp.color_decorators_on.get() {
            return;
        }
        let Some(slot) = self.lsp.colors.slots.get_mut(&buffer) else {
            return;
        };
        let current = (self.lsp.manager.as_ref())
            .zip(slot.server)
            .and_then(|(manager, server)| super::lock(manager).active_identity(buffer, server));
        if slot.asked != Some(version) || identity.is_none() || identity != current {
            return;
        }
        let current = (slot.editors.values().next())
            .is_some_and(|editor| editor.read(cx).model().snapshot().version() == version);
        if !current {
            return;
        }
        let colors = match answer {
            Ok(Some(colors)) => colors,
            Ok(None) => {
                slot.no_provider = slot.server;
                Vec::new()
            }
            // Retried while it may, then given up on as any error is.
            Err(LspError::Retry) if attempt < RETRIES => {
                self.ask_colors_in(buffer, REST, attempt + 1, cx);
                return;
            }
            Err(error) => {
                log::debug!("no document colours: {error}");
                Vec::new()
            }
        };
        let swatches: Vec<ColorSwatch> = (colors.into_iter())
            .map(|info| ColorSwatch {
                offset: info.range.start,
                rgba: info.color,
            })
            .collect();
        slot.answered = (!swatches.is_empty()).then_some((version, swatches));
        let editors: Vec<_> = slot.editors.values().cloned().collect();
        for editor in editors {
            self.show_color_swatches(&editor, cx);
        }
    }
}

/// Sets `editor`'s colour swatches and redraws it.
fn give_swatches(
    editor: &Entity<EditorView>,
    swatches: Vec<ColorSwatch>,
    cx: &mut Context<Workbench>,
) {
    editor.update(cx, |view, cx| {
        view.set_color_swatches(swatches);
        cx.notify();
    });
}

#[cfg(test)]
#[path = "lsp_colors_tests.rs"]
mod tests;
