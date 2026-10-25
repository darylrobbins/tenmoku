//! Language servers' code lenses as clickable rows above their lines, as
//! VS Code shows them: asked for when a buffer opens on its server, 250 ms
//! after its last edit, and at once when the server asks for a refresh. The
//! lenses without a command are resolved, all of an answer's together,
//! before the answer shows; one that resolves to no command is dropped.
//!
//! An answer replaces a buffer's lenses only if it is for the buffer's
//! current version; until one lands the old lenses stay, their rows
//! following the edits. A buffer has one [`LensProvider`], added to each of
//! its editors. A click runs `lsp.run_lens`: the lens's command goes to its
//! server, and one the server doesn't list is said in an info toast; the
//! client commands that list references (`editor.action.showReferences`,
//! `rust-analyzer.showReferences`) open the References panel with them.
//! `editor.code_lens` off removes the providers and asks for nothing.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use fe_buffer::{Anchor, BufferId, BufferSnapshot, Version};
use fe_command::{Invocation, Value};
use fe_gpui::{EditorView, Lens, LensItem, LensProvider};
use fe_lsp::{CodeLens, IncarnationId, LspError, LspNavigation, ServerId, SessionId};
use gpui::{AppContext as _, Context, Entity, EntityId, SharedString, Task};

use crate::{
    Workbench,
    workbench::toast::{self, Toast},
};

/// How long a buffer holds still before its lenses are asked for again.
const REST: Duration = Duration::from_millis(250);

/// The opening of a document a server has: its session and incarnation.
type Identity = (SessionId, IncarnationId);

/// What a request answers.
type Answer = Result<Option<Vec<CodeLens>>, LspError>;

/// One lens row's items, on the row its anchor is on.
type Row = (Anchor, Vec<(SharedString, u64)>);

/// What a buffer's editors show: the rows, in row order.
#[derive(Default)]
struct Shown(Vec<Row>);

impl LensProvider for Shown {
    fn lenses(&mut self, snapshot: &BufferSnapshot) -> Vec<Lens> {
        let text = snapshot.text();
        (self.0.iter())
            .map(|(anchor, items)| Lens {
                row: text.offset_to_point(snapshot.resolve(anchor)).row,
                items: items
                    .iter()
                    .map(|(title, id)| LensItem {
                        title: title.clone(),
                        invocation: invocation(*id),
                    })
                    .collect(),
            })
            .collect()
    }
}

/// The click on lens `id`.
fn invocation(id: u64) -> Invocation {
    Invocation::of("lsp.run_lens")
        .expect("lsp.run_lens is registered")
        .args(Value::Object(
            [("lens".to_owned(), id.into())].into_iter().collect(),
        ))
}

struct Slot {
    /// The editors the provider is added to.
    editors: HashMap<EntityId, Entity<EditorView>>,
    /// The server the buffer is on while lenses are wanted.
    server: Option<ServerId>,
    /// The opening the server had when lenses were last asked for. A new one
    /// (a restarted server, a document reopened) retires the lenses and
    /// requests of the old.
    identity: Option<Identity>,
    /// The version last seen, to ask again when it changes.
    key: Option<Version>,
    /// The version of the latest request.
    asked: Option<Version>,
    /// The server that said it has no provider.
    no_provider: Option<ServerId>,
    /// The wait, then the request and its resolves.
    task: Option<Task<()>>,
    shown: Rc<RefCell<Shown>>,
    provider: Rc<RefCell<dyn LensProvider>>,
    /// The ids of the lenses shown, in `Lenses::table`.
    ids: Vec<u64>,
}

impl Slot {
    fn new() -> Self {
        let shown = Rc::new(RefCell::new(Shown::default()));
        Self {
            editors: HashMap::new(),
            server: None,
            identity: None,
            key: None,
            asked: None,
            no_provider: None,
            task: None,
            provider: shown.clone(),
            shown,
            ids: Vec::new(),
        }
    }
}

/// Every open buffer's lenses.
#[derive(Default)]
pub(super) struct Lenses {
    slots: HashMap<BufferId, Slot>,
    /// The lenses shown, by id: what a click runs.
    table: HashMap<u64, CodeLens>,
    /// The symbol of each references lens in `table`, read from the text it
    /// was asked on.
    origins: HashMap<u64, String>,
    next: u64,
}

impl Workbench {
    /// Brings each buffer's lenses up to date with its text and server: run
    /// by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_lenses(&mut self, cx: &mut Context<Self>) {
        let on = self.lsp.code_lens_on.get();
        let editors = self.open_editors(cx);
        let mut wanted: HashMap<BufferId, (Version, Vec<Entity<EditorView>>)> = HashMap::new();
        for editor in &editors {
            let model = editor.read(cx).model();
            let (buffer, version) = (model.buffer().id(), model.snapshot().version());
            wanted
                .entry(buffer)
                .or_insert_with(|| (version, Vec::new()))
                .1
                .push(editor.clone());
        }
        let gone: Vec<BufferId> = (self.lsp.lenses.slots.keys())
            .filter(|buffer| !wanted.contains_key(buffer))
            .copied()
            .collect();
        for buffer in gone {
            self.drop_lenses(buffer, cx);
        }
        for (buffer, (version, editors)) in wanted {
            let server = self
                .lsp
                .manager
                .as_ref()
                .and_then(|manager| super::lock(manager).server_of(buffer))
                .filter(|_| on);
            let Some(server) = server else {
                self.drop_lenses(buffer, cx);
                continue;
            };
            self.lsp
                .lenses
                .slots
                .entry(buffer)
                .or_insert_with(Slot::new);
            let identity = self
                .lsp
                .manager
                .as_ref()
                .and_then(|manager| super::lock(manager).active_identity(buffer, server));
            let slot = self.lsp.lenses.slots.get_mut(&buffer).expect("just added");
            // Another opening of the document on the same server, or none,
            // retires the old lenses as a move to another server does.
            let changed = slot.identity != identity;
            let reopened = changed && slot.identity.is_some();
            slot.identity = identity;
            if changed {
                slot.key = None;
                slot.no_provider = None;
            }
            let moved = slot.server.is_some_and(|was| was != server) || reopened;
            if slot.server != Some(server) {
                slot.server = Some(server);
                slot.key = None;
                slot.no_provider = None;
            }
            if moved {
                slot.asked = None;
                slot.task = None;
                self.clear_lenses(buffer);
            }
            let slot = self.lsp.lenses.slots.get_mut(&buffer).expect("just added");
            // Editors closed leave the provider with them; new ones get it.
            slot.editors
                .retain(|id, _| editors.iter().any(|editor| editor.entity_id() == *id));
            for editor in &editors {
                if slot
                    .editors
                    .insert(editor.entity_id(), editor.clone())
                    .is_none()
                {
                    let provider = slot.provider.clone();
                    editor.update(cx, |view, cx| {
                        view.add_lens_provider(provider);
                        cx.notify();
                    });
                }
            }
            // An answer with no opening's identity is discarded, and the
            // ask is made again once the identity turns active.
            if identity.is_none() || slot.no_provider == Some(server) || slot.key == Some(version) {
                continue;
            }
            // The first ask goes at once; later ones wait for the text to
            // hold still.
            let first = slot.key.is_none();
            slot.key = Some(version);
            let delay = if first { Duration::ZERO } else { REST };
            self.ask_lenses_in(buffer, delay, cx);
        }
    }

    /// Asks again, at once, for every buffer on `server`.
    pub(super) fn refresh_lenses(&mut self, server: ServerId, cx: &mut Context<Self>) {
        let buffers: Vec<BufferId> = (self.lsp.lenses.slots.iter())
            .filter(|(_, slot)| slot.server == Some(server))
            .map(|(buffer, _)| *buffer)
            .collect();
        for buffer in buffers {
            if let Some(slot) = self.lsp.lenses.slots.get_mut(&buffer) {
                slot.no_provider = None;
            }
            self.ask_lenses_in(buffer, Duration::ZERO, cx);
        }
    }

    /// Removes `buffer`'s lenses and providers.
    fn drop_lenses(&mut self, buffer: BufferId, cx: &mut Context<Self>) {
        self.clear_lenses(buffer);
        let Some(slot) = self.lsp.lenses.slots.remove(&buffer) else {
            return;
        };
        for editor in slot.editors.values() {
            let provider = slot.provider.clone();
            editor.update(cx, |view, cx| {
                view.remove_lens_provider(&provider);
                cx.notify();
            });
        }
    }

    /// Forgets the lenses `buffer` shows.
    fn clear_lenses(&mut self, buffer: BufferId) {
        let lenses = &mut self.lsp.lenses;
        if let Some(slot) = lenses.slots.get_mut(&buffer) {
            slot.shown.borrow_mut().0.clear();
            for id in slot.ids.drain(..) {
                lenses.table.remove(&id);
                lenses.origins.remove(&id);
            }
        }
    }

    /// Has `buffer` ask for its lenses after `delay`, in place of any
    /// request waiting or in flight.
    fn ask_lenses_in(&mut self, buffer: BufferId, delay: Duration, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |workbench, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok(Some((version, identity, reply))) =
                workbench.update(cx, |workbench, cx| workbench.send_lenses(buffer, cx))
            else {
                return;
            };
            let answer = match reply.await {
                Ok(Some(lenses)) => {
                    // Lenses asked on text the buffer has moved on from are
                    // neither resolved nor shown: the next ask replaces them.
                    let current = workbench
                        .update(cx, |workbench, cx| {
                            workbench.lens_request_current(buffer, version, identity, cx)
                        })
                        .unwrap_or(false);
                    if !current {
                        return;
                    }
                    // The ones without a command resolve together.
                    let resolving: Vec<_> = workbench
                        .update(cx, |workbench, cx| {
                            let manager = workbench.lsp.manager.clone();
                            lenses
                                .into_iter()
                                .map(|lens| match &manager {
                                    Some(manager) if lens.command.is_none() => {
                                        let reply =
                                            super::lock(manager).resolve_code_lens(lens.clone());
                                        (lens, Some(cx.background_spawn(reply)))
                                    }
                                    _ => (lens, None),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut done = Vec::new();
                    for (lens, resolve) in resolving {
                        let lens = match resolve {
                            Some(resolve) => resolve.await.unwrap_or(lens),
                            None => lens,
                        };
                        done.push(lens);
                    }
                    Ok(Some(done))
                }
                other => other,
            };
            workbench
                .update(cx, |workbench, cx| {
                    workbench.lenses_landed(buffer, version, identity, answer, cx)
                })
                .ok();
        });
        if let Some(slot) = self.lsp.lenses.slots.get_mut(&buffer) {
            slot.task = Some(task);
        }
    }

    /// Whether the request for `buffer`'s lenses at `version`, made on
    /// the opening `identity`, is still the latest and the text and the
    /// server's opening are as it was asked on.
    fn lens_request_current(
        &self,
        buffer: BufferId,
        version: Version,
        identity: Option<Identity>,
        cx: &Context<Self>,
    ) -> bool {
        let Some(slot) = self.lsp.lenses.slots.get(&buffer) else {
            return false;
        };
        let current = (self.lsp.manager.as_ref())
            .zip(slot.server)
            .and_then(|(manager, server)| super::lock(manager).active_identity(buffer, server));
        let text_is = slot
            .editors
            .values()
            .next()
            .is_some_and(|editor| editor.read(cx).model().snapshot().version() == version);
        slot.asked == Some(version) && identity.is_some() && identity == current && text_is
    }

    /// The request for `buffer`'s lenses at its version now, sent.
    #[allow(clippy::type_complexity)]
    fn send_lenses(
        &mut self,
        buffer: BufferId,
        cx: &mut Context<Self>,
    ) -> Option<(
        Version,
        Option<Identity>,
        std::pin::Pin<Box<dyn std::future::Future<Output = Answer> + Send>>,
    )> {
        let manager = self.lsp.manager.clone()?;
        let slot = self.lsp.lenses.slots.get_mut(&buffer)?;
        let editor = slot.editors.values().next()?;
        let version = editor.read(cx).model().snapshot().version();
        slot.asked = Some(version);
        let manager = super::lock(&manager);
        let identity = slot
            .server
            .and_then(|server| manager.active_identity(buffer, server));
        let reply = manager.code_lenses(buffer, version);
        Some((version, identity, Box::pin(reply)))
    }

    /// Shows `answer`, the reply to the request for `buffer` at `version`,
    /// if it is still the latest, from the opening the server still has, and the
    /// text hasn't moved on.
    fn lenses_landed(
        &mut self,
        buffer: BufferId,
        version: Version,
        identity: Option<Identity>,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        if !self.lsp.code_lens_on.get() {
            return;
        }
        let Some(slot) = self.lsp.lenses.slots.get_mut(&buffer) else {
            return;
        };
        // From the opening the server has now.
        let current = (self.lsp.manager.as_ref())
            .zip(slot.server)
            .and_then(|(manager, server)| super::lock(manager).active_identity(buffer, server));
        if slot.asked != Some(version) || identity.is_none() || identity != current {
            return;
        }
        let snapshot = slot
            .editors
            .values()
            .next()
            .map(|editor| editor.read(cx).model().snapshot());
        let Some(snapshot) = snapshot.filter(|snapshot| snapshot.version() == version) else {
            return;
        };
        let lenses = match answer {
            Ok(Some(lenses)) => lenses,
            Ok(None) => {
                slot.no_provider = slot.server;
                Vec::new()
            }
            Err(error) => return log::debug!("no code lenses: {error}"),
        };
        // A lens that resolved to no command is dropped.
        let text = snapshot.text();
        let Lenses {
            slots,
            table,
            origins,
            next,
        } = &mut self.lsp.lenses;
        let slot = slots.get_mut(&buffer).expect("looked up above");
        for id in slot.ids.drain(..) {
            table.remove(&id);
            origins.remove(&id);
        }
        let language = slot.editors.values().next().and_then(|editor| {
            editor
                .read(cx)
                .syntax()
                .map(|syntax| syntax.language().name())
        });
        let mut rows: Vec<(u32, Row)> = Vec::new();
        for lens in lenses {
            let Some(title) = lens
                .command
                .as_ref()
                .map(|c| SharedString::from(c.title.clone()))
            else {
                continue;
            };
            let row = text.offset_to_point(lens.range.start.min(text.len())).row;
            let id = *next;
            *next += 1;
            slot.ids.push(id);
            if (lens.command.as_ref()).is_some_and(LspNavigation::is_references_command) {
                let word = crate::references_panel::word_at(text, lens.range.start, language)
                    .unwrap_or_default();
                origins.insert(id, word);
            }
            table.insert(id, lens);
            match rows.last_mut() {
                Some((last, (_, items))) if *last == row => items.push((title, id)),
                _ => {
                    let anchor = snapshot.anchor_before(text.row_start(row));
                    rows.push((row, (anchor, vec![(title, id)])));
                }
            }
        }
        slot.shown.borrow_mut().0 = rows.into_iter().map(|(_, row)| row).collect();
        for editor in slot.editors.values() {
            editor.update(cx, |_, cx| cx.notify());
        }
    }

    /// Runs lens `id`'s command, as a click on it asks: a command its
    /// server lists goes to it, any other is said in a toast.
    pub(crate) fn run_lens(&mut self, id: u64, cx: &mut Context<Self>) {
        let (Some(lens), Some(manager)) = (
            self.lsp.lenses.table.get(&id).cloned(),
            self.lsp.manager.clone(),
        ) else {
            return;
        };
        let Some(title) = lens.command.as_ref().map(|c| c.title.clone()) else {
            return;
        };
        // The client commands that list references open the References
        // panel; nothing goes to the server.
        if (lens.command.as_ref()).is_some_and(LspNavigation::is_references_command) {
            let origin = self.lsp.lenses.origins.get(&id).cloned();
            let context = lens.context().clone();
            let locations = lens
                .command
                .as_ref()
                .and_then(LspNavigation::lens_locations);
            let (Some(word), Some(locations)) = (origin, locations) else {
                log::debug!("lens {title:?}: unusable arguments");
                toast::show(
                    cx,
                    Toast::info(format!("{title}: this command isn't supported")),
                );
                return;
            };
            // The panel takes focus in the window the click was in, once this
            // update is over.
            cx.spawn(async move |this, cx| {
                let Some(window) =
                    cx.update(|cx| cx.active_window().or_else(|| cx.windows().last().copied()))
                else {
                    return;
                };
                cx.update_window(window, |_, window, cx| {
                    this.update(cx, |this, cx| {
                        let decode = LspNavigation::decode_references;
                        this.show_references(word, context, locations, decode, window, cx)
                    })
                    .ok();
                })
                .ok();
            })
            .detach();
            return;
        }
        let reply = super::lock(&manager).run_lens_command(&lens);
        cx.spawn(async move |_, cx| match reply.await {
            Ok(()) => {}
            Err(LspError::Unavailable(why)) => {
                log::debug!("lens {title:?} not run: {why}");
                cx.update(|cx| {
                    toast::show(
                        cx,
                        Toast::info(format!("{title}: this command isn't supported")),
                    )
                });
            }
            Err(error) => {
                cx.update(|cx| toast::show(cx, Toast::info(format!("{title}: {error}"))));
            }
        })
        .detach();
    }
}
