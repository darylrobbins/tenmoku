//! Language servers' document links, as VS Code's `editor.links` has them:
//! asked for when a buffer opens on its server and 500 ms after its last
//! edit, and handed to each of its editors as link ranges
//! ([`EditorView::set_link_ranges`]), which a bound modifier click
//! underlines whole.
//!
//! An answer replaces a buffer's links only if it is for the buffer's
//! current version; until one lands the old links stay, following the
//! edits. `editor.links` off clears them and asks for nothing.
//!
//! Opening a link (`editor.open_link` at the caret, or
//! `nav.goto_definition`, which a cmd-click runs, over a link): one without
//! a target is resolved first. A `file:` target opens in an editor tab, at
//! the line and column a fragment `#L12`, `#L12,5` or `#12,5` names
//! (1-based); `http`, `https` and `mailto` go to the system's opener; any
//! other scheme is refused with a toast.

use std::{collections::HashMap, ops::Range, path::PathBuf, time::Duration};

use fe_buffer::{Anchor, BufferId, PointUtf16, Version};
use fe_command::Source;
use fe_gpui::EditorView;
use fe_lsp::{DocumentLink, IncarnationId, LspError, ServerId, SessionId};
use gpui::{AppContext as _, Context, Entity, EntityId, Subscription, Task, Window};

use crate::{
    Workbench,
    pane::{Pane, Reveal},
    workbench::toast::{self, Toast},
};

/// How long a buffer holds still before its links are asked for again,
/// and how long a server that asks for a retry waits.
const REST: Duration = Duration::from_millis(500);

/// How many times a request is retried on the server's asking.
const RETRIES: u32 = 8;

/// The opening of a document a server has: its session and incarnation.
type Identity = (SessionId, IncarnationId);

/// What a request answers.
type Answer = Result<Option<Vec<DocumentLink>>, LspError>;

struct Slot {
    /// The editors given the links.
    editors: HashMap<EntityId, Entity<EditorView>>,
    /// The server the buffer is on while links are wanted.
    server: Option<ServerId>,
    /// The opening the server had when links were last asked for. A new
    /// one (a restarted server, a document reopened) retires the links of
    /// the old, whose `data` only it can resolve.
    identity: Option<Identity>,
    /// The version last seen, to ask again when it changes.
    key: Option<Version>,
    /// The version of the latest request.
    asked: Option<Version>,
    /// The server that said it has no provider.
    no_provider: Option<ServerId>,
    /// The wait, then the request.
    task: Option<Task<()>>,
    /// The links, anchored at `found`, the version they were found at.
    links: Vec<(Range<Anchor>, DocumentLink)>,
    found: Option<Version>,
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
            task: None,
            links: Vec::new(),
            found: None,
        }
    }

    /// The links' ranges in `editor`'s text now, with the links; none if
    /// the buffer can't map from where they were found any more.
    fn current(&self, editor: &EditorView) -> Vec<(Range<usize>, &DocumentLink)> {
        let snapshot = editor.model().snapshot();
        let Some(found) = self.found else {
            return Vec::new();
        };
        if found < snapshot.oldest_version() || found > snapshot.version() {
            return Vec::new();
        }
        (self.links.iter())
            .map(|(range, link)| (snapshot.resolve_range(range), link))
            .filter(|(range, _)| !range.is_empty())
            .collect()
    }
}

/// Every open buffer's links.
#[derive(Default)]
pub(super) struct Links {
    slots: HashMap<BufferId, Slot>,
    /// Watching the pane for a file link's row to be revealed, to move the
    /// caret to its column ([`Workbench::land_link_column`]).
    landing: Option<Subscription>,
}

impl Workbench {
    /// Brings each buffer's links up to date with its text and server: run
    /// by [`Self::sync_lsp`] after each draw.
    pub(super) fn sync_links(&mut self, cx: &mut Context<Self>) {
        let on = self.lsp.links_on.get();
        let mut wanted: HashMap<BufferId, (Version, Vec<Entity<EditorView>>)> = HashMap::new();
        for editor in self.open_editors(cx) {
            let model = editor.read(cx).model();
            let (buffer, version) = (model.buffer().id(), model.snapshot().version());
            (wanted.entry(buffer))
                .or_insert_with(|| (version, Vec::new()))
                .1
                .push(editor);
        }
        let gone: Vec<BufferId> = (self.lsp.links.slots.keys())
            .filter(|buffer| !wanted.contains_key(buffer))
            .copied()
            .collect();
        for buffer in gone {
            self.drop_links(buffer, cx);
        }
        for (buffer, (version, editors)) in wanted {
            let server = (self.lsp.manager.as_ref())
                .and_then(|manager| super::lock(manager).server_of(buffer))
                .filter(|_| on);
            let Some(server) = server else {
                self.drop_links(buffer, cx);
                continue;
            };
            let identity = (self.lsp.manager.as_ref())
                .and_then(|manager| super::lock(manager).active_identity(buffer, server));
            let slot = self.lsp.links.slots.entry(buffer).or_insert_with(Slot::new);
            if slot.server != Some(server) || slot.identity != identity {
                // Another server's, or opening's, links aren't this one's.
                let moved = slot.server.is_some_and(|was| was != server)
                    || (slot.identity.is_some() && slot.identity != identity);
                slot.server = Some(server);
                slot.identity = identity;
                slot.key = None;
                slot.no_provider = None;
                if moved {
                    slot.asked = None;
                    slot.task = None;
                    slot.links.clear();
                    slot.found = None;
                    for editor in slot.editors.values() {
                        give_links(editor, Vec::new(), cx);
                    }
                }
            }
            // Editors closed are forgotten; new ones get the links.
            slot.editors
                .retain(|id, _| editors.iter().any(|editor| editor.entity_id() == *id));
            for editor in &editors {
                if slot.editors.contains_key(&editor.entity_id()) {
                    continue;
                }
                slot.editors.insert(editor.entity_id(), editor.clone());
                let ranges = (slot.current(editor.read(cx)).into_iter())
                    .map(|(range, _)| range)
                    .collect();
                give_links(editor, ranges, cx);
            }
            if slot.no_provider == Some(server) || slot.key == Some(version) {
                continue;
            }
            // The first ask goes at once; later ones wait for the text to
            // hold still.
            let first = slot.key.is_none();
            slot.key = Some(version);
            let delay = if first { Duration::ZERO } else { REST };
            self.ask_links_in(buffer, delay, 0, cx);
        }
    }

    /// Forgets `buffer`'s links and takes them from its editors.
    fn drop_links(&mut self, buffer: BufferId, cx: &mut Context<Self>) {
        let Some(slot) = self.lsp.links.slots.remove(&buffer) else {
            return;
        };
        for editor in slot.editors.values() {
            give_links(editor, Vec::new(), cx);
        }
    }

    /// Has `buffer` ask for its links after `delay`, in place of any
    /// request waiting or in flight; the `attempt`th retry, from 0.
    fn ask_links_in(
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
                workbench.update(cx, |workbench, cx| workbench.send_links(buffer, cx))
            else {
                return;
            };
            let answer = reply.await;
            workbench
                .update(cx, |workbench, cx| {
                    workbench.links_landed(buffer, version, identity, attempt, answer, cx)
                })
                .ok();
        });
        if let Some(slot) = self.lsp.links.slots.get_mut(&buffer) {
            slot.task = Some(task);
        }
    }

    /// The request for `buffer`'s links at its version now, sent.
    #[allow(clippy::type_complexity)]
    fn send_links(
        &mut self,
        buffer: BufferId,
        cx: &mut Context<Self>,
    ) -> Option<(
        Version,
        Option<Identity>,
        std::pin::Pin<Box<dyn std::future::Future<Output = Answer> + Send>>,
    )> {
        let manager = self.lsp.manager.clone()?;
        let slot = self.lsp.links.slots.get_mut(&buffer)?;
        let editor = slot.editors.values().next()?;
        let version = editor.read(cx).model().snapshot().version();
        slot.asked = Some(version);
        let reply = super::lock(&manager).document_links(buffer, version);
        Some((version, slot.identity, Box::pin(reply)))
    }

    /// Hands `answer`, the reply to the request for `buffer` at `version`,
    /// made on the opening `identity`, to its editors, if it is still the
    /// latest, the server still has that opening, and the text hasn't
    /// moved on. A server's asking for a retry is asked again, as the
    /// `attempt`th retry, after [`REST`], up to [`RETRIES`] times.
    fn links_landed(
        &mut self,
        buffer: BufferId,
        version: Version,
        identity: Option<Identity>,
        attempt: u32,
        answer: Answer,
        cx: &mut Context<Self>,
    ) {
        if !self.lsp.links_on.get() {
            return;
        }
        let Some(slot) = self.lsp.links.slots.get_mut(&buffer) else {
            return;
        };
        let current = (self.lsp.manager.as_ref())
            .zip(slot.server)
            .and_then(|(manager, server)| super::lock(manager).active_identity(buffer, server));
        if slot.asked != Some(version) || identity.is_none() || identity != current {
            return;
        }
        let snapshot =
            (slot.editors.values().next()).map(|editor| editor.read(cx).model().snapshot().clone());
        let Some(snapshot) = snapshot.filter(|snapshot| snapshot.version() == version) else {
            return;
        };
        let links = match answer {
            Ok(Some(links)) => links,
            Ok(None) => {
                slot.no_provider = slot.server;
                Vec::new()
            }
            Err(LspError::Retry) => {
                if attempt < RETRIES {
                    self.ask_links_in(buffer, REST, attempt + 1, cx);
                }
                return;
            }
            Err(error) => return log::debug!("no document links: {error}"),
        };
        let ranges: Vec<Range<usize>> = links.iter().map(|link| link.range.clone()).collect();
        // Text typed at a link's edge stays outside it.
        slot.links = (links.into_iter())
            .map(|link| {
                let range = &link.range;
                let anchors = snapshot.anchor_after(range.start)..snapshot.anchor_before(range.end);
                (anchors, link)
            })
            .collect();
        slot.found = Some(version);
        for editor in slot.editors.values() {
            give_links(editor, ranges.clone(), cx);
        }
    }

    /// The link `editor` has at byte `offset`, with its buffer and the
    /// version it was found at: one holding the char there, or else, if
    /// `at_end`, one ending there, as a caret just past it is.
    fn link_in(
        &self,
        editor: &Entity<EditorView>,
        offset: usize,
        at_end: bool,
        cx: &Context<Self>,
    ) -> Option<(BufferId, Version, DocumentLink)> {
        let view = editor.read(cx);
        let buffer = view.model().buffer().id();
        let slot = self.lsp.links.slots.get(&buffer)?;
        let current = slot.current(view);
        let (_, link) = (current.iter())
            .find(|(range, _)| range.contains(&offset))
            .or_else(|| (current.iter()).find(|(range, _)| at_end && range.end == offset))?;
        Some((buffer, slot.found?, (*link).clone()))
    }

    /// `editor.open_link`: opens the link at the active editor's caret, as
    /// VS Code's Open Link does, a caret just past one being at it.
    pub(crate) fn open_link_at_caret(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return;
        };
        let caret = editor.read(cx).model().primary_selection().head();
        self.open_link_at(&editor, caret, true, window, cx);
    }

    /// For `nav.goto_definition` run from `source`: opens the link the
    /// click that ran it was on ([`EditorView::take_clicked_link`]), or,
    /// from anything else, the one holding the caret's char, and says
    /// whether there was one, which it goes to instead. A click off a link
    /// opens none, wherever it put the caret.
    pub(crate) fn open_link_for_definition(
        &mut self,
        source: Option<Source>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(editor) = self.active_editor(cx).cloned() else {
            return false;
        };
        let offset = if source == Some(Source::Mouse) {
            match editor.update(cx, |view, _| view.take_clicked_link()) {
                Some(clicked) => clicked.start,
                None => return false,
            }
        } else {
            editor.read(cx).model().primary_selection().head()
        };
        self.open_link_at(&editor, offset, false, window, cx)
    }

    /// Opens `editor`'s link at `offset` ([`Self::link_in`]); whether there
    /// was one. With `editor.links` off there is none.
    fn open_link_at(
        &mut self,
        editor: &Entity<EditorView>,
        offset: usize,
        at_end: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.lsp.links_on.get() {
            return false;
        }
        let Some((buffer, version, link)) = self.link_in(editor, offset, at_end, cx) else {
            return false;
        };
        if link.target.is_some() {
            self.follow_link(&link, window, cx);
            return true;
        }
        // A link without a target resolves first.
        let Some(manager) = self.lsp.manager.clone() else {
            return true;
        };
        // Known limits (review F9, F10; follow-up T339): `version` is the
        // one the links were found at, so after an edit, before the refresh
        // 500 ms later, the server refuses the resolve as "buffer changed"
        // and this says the link has no target. And neither this open nor
        // `open_link_file`'s carries the pane it started in or a navigation
        // token, so one that a pane switch or a newer open has overtaken
        // still opens.
        let reply = super::lock(&manager).resolve_document_link(buffer, version, link);
        let handle = window.window_handle();
        cx.spawn(async move |workbench, cx| {
            let resolved = reply.await;
            cx.update_window(handle, |_, window, cx| {
                workbench
                    .update(cx, |workbench, cx| match resolved {
                        Ok(link) if link.target.is_some() => {
                            workbench.follow_link(&link, window, cx)
                        }
                        Ok(_) => toast::show(cx, Toast::info("This link has no target")),
                        Err(error) => {
                            log::debug!("document link not resolved: {error}");
                            toast::show(cx, Toast::info("This link has no target"));
                        }
                    })
                    .ok();
            })
            .ok();
        })
        .detach();
        true
    }

    /// Opens `link`'s target: a file in an editor tab, a web or mail
    /// address through the system's opener.
    fn follow_link(&mut self, link: &DocumentLink, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = link.target.clone() else {
            return;
        };
        match link.scheme().as_deref() {
            Some("file") => {
                let Some((path, fragment)) = link.file_target() else {
                    return toast::show(cx, Toast::error(format!("Can't open {target}")));
                };
                self.open_link_file(path, fragment.as_deref(), window, cx);
            }
            Some("http" | "https" | "mailto") => cx.open_url(&target),
            _ => toast::show(cx, Toast::error("Can't open link: unsupported scheme")),
        }
    }

    /// Opens `path` in the active pane, at the line and column `fragment`
    /// names, if it names one: the column counts UTF-16 units, as VS Code's
    /// do, of the row as the editor has it ([`Self::land_link_column`]).
    /// One that isn't a file is said in a toast; one that can't be read,
    /// in the pane's.
    fn open_link_file(
        &mut self,
        path: PathBuf,
        fragment: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let place = fragment.and_then(line_column);
        // The pane opens a missing file as a new one, which a link isn't:
        // looked for off the main thread first.
        let handle = window.window_handle();
        let pane = self.pane().clone();
        cx.spawn(async move |workbench, cx| {
            let (is_file, path) =
                (cx.background_spawn(async move { (path.is_file(), path) })).await;
            cx.update_window(handle, |_, window, cx| {
                if !is_file {
                    let name = path.file_name().unwrap_or(path.as_os_str());
                    let message = format!("Can't open {}", name.to_string_lossy());
                    return toast::show(cx, Toast::error(message));
                }
                // The row now; the column once the editor has the text.
                let reveal = place.map(|(row, _)| Reveal {
                    row,
                    columns: 0..0,
                    within_row: true,
                });
                let opened = path.clone();
                pane.update(cx, |pane, cx| pane.open_at(opened, reveal, window, cx));
                if let Some(place) = place.filter(|(_, column)| *column > 0) {
                    (workbench.update(cx, |workbench, cx| {
                        workbench.land_link_column(&pane, path, place, cx)
                    }))
                    .ok();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Once `pane` has revealed the row of `place` for the open last made,
    /// moves the caret along it to `place`'s column, in UTF-16 units of the
    /// row as its editor decoded it (BOM, encoding and line endings as
    /// loaded, unsaved edits and all). Nothing if another open supersedes
    /// it, or the caret has moved since.
    fn land_link_column(
        &mut self,
        pane: &Entity<Pane>,
        path: PathBuf,
        place: (u32, usize),
        cx: &mut Context<Self>,
    ) {
        let serial = pane.read(cx).open_serial();
        self.lsp.links.landing = None;
        if self.try_land_link_column(pane, serial, &path, place, cx) {
            return;
        }
        let watch = cx.observe(pane, move |workbench, pane, cx| {
            if workbench.try_land_link_column(&pane, serial, &path, place, cx) {
                workbench.lsp.links.landing = None;
            }
        });
        self.lsp.links.landing = Some(watch);
    }

    /// [`Self::land_link_column`]'s step: whether it's done, landed or not.
    fn try_land_link_column(
        &mut self,
        pane: &Entity<Pane>,
        serial: u64,
        path: &std::path::Path,
        (row, column): (u32, usize),
        cx: &mut Context<Self>,
    ) -> bool {
        let pane = pane.read(cx);
        if pane.open_serial() != serial {
            return true;
        }
        let Some(revealed) = pane.revealed().filter(|revealed| revealed.serial == serial) else {
            // Still being read; a viewer's reveal is never an editor's.
            return !pane.is_opening(path);
        };
        let editor = (pane.tabs().iter())
            .filter_map(|tab| tab.editor())
            .find(|editor| editor.entity_id() == revealed.editor)
            .cloned();
        let Some(editor) = editor else {
            return true;
        };
        if editor.read(cx).model().primary_selection() != revealed.selection {
            return true;
        }
        editor.update(cx, |view, cx| {
            let text = view.model().text();
            let offset = text.point_utf16_to_offset(PointUtf16::new(row, column as u32));
            view.model_mut().set_cursor(offset);
            cx.notify();
        });
        true
    }
}

/// Sets `editor`'s link ranges and redraws it, so a hover under a still
/// pointer, with the modifier held, finds the link, or loses it, at once.
fn give_links(editor: &Entity<EditorView>, ranges: Vec<Range<usize>>, cx: &mut Context<Workbench>) {
    editor.update(cx, |view, cx| {
        view.set_link_ranges(ranges);
        cx.notify();
    });
}

/// The 0-based row and column a fragment `L12`, `L12,5` or `12,5` names in
/// 1-based numbers, the column 1 if it names none.
fn line_column(fragment: &str) -> Option<(u32, usize)> {
    let numbers = fragment.strip_prefix('L').unwrap_or(fragment);
    let (line, column) = match numbers.split_once(',') {
        Some((line, column)) => (line, Some(column)),
        None => (numbers, None),
    };
    let line: u32 = line.parse().ok()?;
    let column: usize = match column {
        Some(column) => column.parse().ok()?,
        None => 1,
    };
    Some((line.checked_sub(1)?, column.saturating_sub(1)))
}

#[cfg(test)]
#[path = "lsp_links_tests.rs"]
mod tests;
