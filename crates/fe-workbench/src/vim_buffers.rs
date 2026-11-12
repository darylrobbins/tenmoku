//! Vim's buffer list over the window's open files: `:b`, `:bn`, `:bp`,
//! `:bd`, `:bf` and `:bl` ([`BufferCommand`]), numbered as nvim numbers
//! buffers.
//!
//! A buffer is a file open in a tab of the window, in any pane, or an
//! untitled buffer: an editor's text (tabs splitting one text are one
//! buffer), or a viewer's file. Each gets the next number as its tab
//! comes, when the window's workspace changes after it (a tab opened,
//! shown or closed); tabs that come in one change are numbered in pane and
//! tab order. A buffer keeps its number as its file is renamed or saved
//! under another name, as nvim's does. A number is never reused: a file
//! closed and opened again has its old one, as nvim's unlisted buffers keep
//! theirs. The listed buffers are those with a tab open. Numbers last as
//! long as the window, and aren't saved.
//!
//! As in nvim, a buffer is shown in the current window, the pane of the
//! editor the line was typed in: its tab there is activated, or else the
//! file is opened there (a buffer in another pane gets a tab in this one,
//! sharing its text, rather than focus moving there). `:b N` opens a closed
//! file again. `#`, the alternate, is the buffer the pane showed before the
//! current one: each pane's are noted as the active pane shows another tab,
//! and as these commands show one. `:bd` closes the buffer's tabs in every
//! pane, showing in this pane first what nvim would: the buffer it showed
//! last that is still listed, else the next listed by number, else the one
//! before; a pane emptied closes, as nvim closes the other windows showing
//! it, and focus stays in this pane. The window stays open with an empty
//! pane when the last goes. When that buffer's file is still being read,
//! the tabs go once it lands. A range's `$` is the last buffer numbered,
//! listed or not, and a range is checked against the numbers there are
//! (E16), and for order (E493), as nvim does.
//!
//! A name matches as nvim's `:b {name}` does, a part of the file's path,
//! tried anywhere, then at the start, then at the end, then whole, until
//! one buffer matches; ignoring case when the name is in lower case. The
//! path is both the one relative to the folder, for a file in it, as
//! nvim's short names are, and the whole one. Wildcards aren't read.
//!
//! An editor shown outside the tabs (a diff view's side, the commit box)
//! runs them on the active pane, whose shown tab is the current buffer if
//! it's a file's.
//!
//! Each pane notes its tabs' transitions (added, shown, removed) as they
//! happen, in `Pane::activated` and `Pane::items_changed`'s tab sync, so
//! several in one update are numbered and noted in order; the workspace's
//! notifications, once per update, only bring the registry to read them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fe_buffer::BufferId;
use fe_gpui::EditorView;
use fe_gpui::FileViewer;
use fe_vim::{BufferAddress, BufferCommand, BufferRow, BufferTarget, Pick};
use gpui::{App, Context, Entity, EntityId, Global, Subscription, WeakEntity, Window};
use kiln_shell::ItemHandle;
use kiln_shell::PaneView as _;

use super::{Said, message};
use crate::{Pane, Workbench};
use kiln_l10n::tr;

const E16: Said = Said(|| tr!("workbench-workbench-vim-e16"));
const E493: Said = Said(|| tr!("workbench-workbench-vim-e493"));
const E23: Said = Said(|| tr!("workbench-workbench-vim-e23"));
const E85: Said = Said(|| tr!("workbench-workbench-vim-e85"));
const E516: Said = Said(|| tr!("workbench-workbench-vim-e516"));

/// A tab transition a pane noted at its mutation site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Added,
    Activated,
    Removed,
}

/// A transition, with the tab's item as it was then, and its order among
/// every pane's.
pub(crate) struct Logged {
    seq: u64,
    kind: Kind,
    path: Option<PathBuf>,
    buffer: Option<BufferId>,
    /// Whether the item is a buffer: an editor, or a viewer of a file.
    listed: bool,
}

/// A transition noted, with the pane it was in.
pub(crate) struct Pending {
    pane: WeakEntity<Pane>,
    /// The workbench the pane was made for, if it knew.
    owner: Option<EntityId>,
    entry: Logged,
}

/// Notes that `item` of `pane` had `kind` happen to it. The window reads
/// them as it next follows its workbench, in the order they happened,
/// whether or not the pane is then still in it.
pub(crate) fn record(
    pane: &WeakEntity<Pane>,
    owner: Option<EntityId>,
    kind: Kind,
    item: &dyn ItemHandle,
    cx: &mut App,
) {
    if owner.is_some_and(|owner| cx.default_global::<VimBuffers>().released.contains(&owner)) {
        return;
    }
    let editor = item.downcast_ref::<EditorView>();
    let buffer = editor.map(|editor| editor.read(cx).model().buffer().id());
    let path = item.path(cx);
    let listed =
        buffer.is_some() || (item.downcast_ref::<FileViewer>().is_some() && path.is_some());
    let all = cx.default_global::<VimBuffers>();
    all.seq += 1;
    let entry = Logged {
        seq: all.seq,
        kind,
        path,
        buffer,
        listed,
    };
    all.pending.push(Pending {
        pane: pane.clone(),
        owner,
        entry,
    });
}

/// Each window's buffers, by its workbench.
#[derive(Default)]
struct VimBuffers {
    windows: HashMap<EntityId, WindowBuffers>,
    /// The last transition's order.
    seq: u64,
    /// Transitions noted and not yet read, in order.
    pending: Vec<Pending>,
    /// The workbenches released since they were first read, whose panes'
    /// later transitions are dropped: nothing will read them.
    released: std::collections::HashSet<EntityId>,
}

impl Global for VimBuffers {}

/// Drops the window of the released workbench `id`, and the transitions its
/// panes noted that were never read.
fn forget(id: EntityId, cx: &mut App) {
    let all = cx.global_mut::<VimBuffers>();
    all.windows.remove(&id);
    all.pending.retain(|pending| pending.owner != Some(id));
    all.released.insert(id);
}

struct WindowBuffers {
    workbench: WeakEntity<Workbench>,
    /// The panes this window has had, so a pane closed since its last
    /// transition is still its.
    panes: std::collections::HashSet<EntityId>,
    /// To the window's workspace changing.
    _watch: Option<Subscription>,
    /// Deletions waiting for the file that replaces a buffer to open.
    waiting: HashMap<u64, Subscription>,
    buffers: Buffers,
}

/// A buffer numbered: its file, if it has one, and its editor's text.
#[derive(Clone, Debug, Default)]
struct Entry {
    path: Option<PathBuf>,
    buffer: Option<BufferId>,
}

#[derive(Clone, Default)]
struct Buffers {
    /// Each buffer numbered, buffer `n` at `n - 1`.
    numbered: Vec<Entry>,
    by_path: HashMap<PathBuf, u32>,
    by_buffer: HashMap<BufferId, u32>,
    /// What each pane has shown, by number, the last shown last, each once.
    shown: HashMap<EntityId, Vec<u32>>,
}

impl Buffers {
    /// The number of the buffer at `path` with the text `buffer`, numbering
    /// it if it has none. An editor's text keeps its number whatever its
    /// path is now; a file opened again takes its old one.
    fn number(&mut self, path: Option<&Path>, buffer: Option<BufferId>) -> u32 {
        if let Some(&number) = buffer
            .as_ref()
            .and_then(|buffer| self.by_buffer.get(buffer))
        {
            let entry = &mut self.numbered[number as usize - 1];
            if entry.path.as_deref() != path {
                if let Some(old) = entry.path.take()
                    && self.by_path.get(&old) == Some(&number)
                {
                    self.by_path.remove(&old);
                }
                entry.path = path.map(Path::to_owned);
                if let Some(path) = path {
                    // A closed buffer of the file it takes keeps its number,
                    // but not the file: there's one buffer of a file.
                    let held = self.by_path.insert(path.to_owned(), number);
                    if let Some(held) = held.filter(|&held| held != number) {
                        self.numbered[held as usize - 1].path = None;
                    }
                }
            }
            return number;
        }
        let known = path.and_then(|path| self.by_path.get(path)).copied();
        let number = known.unwrap_or_else(|| {
            self.numbered.push(Entry {
                path: path.map(Path::to_owned),
                buffer: None,
            });
            let number = self.numbered.len() as u32;
            if let Some(path) = path {
                self.by_path.insert(path.to_owned(), number);
            }
            number
        });
        if let Some(buffer) = buffer {
            self.numbered[number as usize - 1].buffer = Some(buffer);
            self.by_buffer.insert(buffer, number);
        }
        number
    }

    fn entry(&self, number: u32) -> Option<&Entry> {
        self.numbered.get(number.checked_sub(1)? as usize)
    }

    /// Notes `number` as what `pane` shows now.
    fn show(&mut self, pane: EntityId, number: u32) {
        let shown = self.shown.entry(pane).or_default();
        if shown.last() != Some(&number) {
            shown.retain(|&n| n != number);
            shown.push(number);
        }
    }

    /// What `pane` showed before `current`.
    fn alternate(&self, pane: EntityId, current: Option<u32>) -> Option<u32> {
        let shown = self.shown.get(&pane)?;
        shown.iter().rev().copied().find(|&n| Some(n) != current)
    }
}

/// A tab of a buffer: its pane, its index there, and its buffer.
struct BufferTab {
    pane: Entity<Pane>,
    ix: usize,
    path: Option<PathBuf>,
    /// The text's buffer, for an editor's tab.
    buffer: Option<BufferId>,
    dirty: bool,
    /// Its buffer's number, once numbered.
    number: u32,
}

impl Workbench {
    /// Every tab of a buffer, an editor's or a viewer's, in pane and tab
    /// order, not yet numbered.
    fn buffer_tabs(&self, cx: &App) -> Vec<BufferTab> {
        let mut tabs = Vec::new();
        for pane in self.panes(cx) {
            for (ix, tab) in pane.read(cx).tabs().iter().enumerate() {
                let buffer = (tab.editor()).map(|editor| editor.read(cx).model().buffer().id());
                let path = tab.item().path(cx);
                if buffer.is_none() && (tab.viewer().is_none() || path.is_none()) {
                    continue;
                }
                tabs.push(BufferTab {
                    pane: pane.clone(),
                    ix,
                    path,
                    buffer,
                    dirty: tab.item().is_dirty(cx),
                    number: 0,
                });
            }
        }
        tabs
    }

    /// Runs `f` on this window's buffers, with every one of `tabs`
    /// numbered, starting to note the workspace's changes if not yet.
    fn vim_buffers<R>(
        &self,
        tabs: &mut [BufferTab],
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Buffers) -> R,
    ) -> R {
        let (id, workbench) = (cx.entity_id(), cx.weak_entity());
        let watching = (cx.try_global::<VimBuffers>())
            .and_then(|all| all.windows.get(&id))
            .is_some_and(|window| window._watch.is_some());
        let watch = (!watching).then(|| {
            cx.observe(&self.workspace, |workbench, _, cx| {
                workbench.note_vim_buffer(cx)
            })
        });
        let known = (cx.default_global::<VimBuffers>().windows).contains_key(&id);
        if !known {
            // Once released nothing reads this window's transitions, noted
            // or to come, so they go.
            cx.on_release(move |_, cx| forget(id, cx)).detach();
        }
        // Every transition the panes noted, in the order they happened,
        // before the final state: a tab that came and went has a number.
        // A pane that is gone and was never seen is taken to be this
        // window's.
        let live: Vec<EntityId> = self.panes(cx).iter().map(Entity::entity_id).collect();
        let all = cx.global_mut::<VimBuffers>();
        all.windows
            .retain(|_, window| window.workbench.upgrade().is_some());
        let window = (all.windows.entry(id)).or_insert_with(|| WindowBuffers {
            workbench,
            _watch: None,
            waiting: HashMap::new(),
            panes: Default::default(),
            buffers: Buffers::default(),
        });
        window.panes.extend(live.iter().copied());
        if watch.is_some() {
            window._watch = watch;
        }
        // A transition is its pane's workbench's, even if the pane came and
        // went before any read. A pane that doesn't know its workbench is
        // the window that has seen it, else (if gone and never seen) the
        // reader's.
        let pending = std::mem::take(&mut all.pending);
        let (mine, rest): (Vec<_>, Vec<_>) = pending.into_iter().partition(|pending| {
            if let Some(owner) = pending.owner {
                return owner == id;
            }
            let pane = pending.pane.entity_id();
            let seen = |window: &WindowBuffers| window.panes.contains(&pane);
            if all.windows[&id].panes.contains(&pane) {
                return true;
            }
            pending.pane.upgrade().is_none() && !all.windows.values().any(seen)
        });
        all.pending = rest;
        let window = all.windows.get_mut(&id).expect("just inserted");
        let buffers = &mut window.buffers;
        let mut mine = mine;
        mine.sort_by_key(|pending| pending.entry.seq);
        for Pending { pane, entry, .. } in mine {
            let pane = pane.entity_id();
            if !entry.listed || entry.kind == Kind::Removed {
                continue;
            }
            let number = buffers.number(entry.path.as_deref(), entry.buffer);
            if entry.kind == Kind::Activated {
                buffers.show(pane, number);
            }
        }
        for tab in tabs {
            tab.number = buffers.number(tab.path.as_deref(), tab.buffer);
        }
        f(buffers)
    }

    /// Numbers the window's buffers and notes the active pane's shown one:
    /// see the module docs. Run as the workspace changes, and as the
    /// workbench follows its active editor, which starts that.
    pub(super) fn note_vim_buffer(&self, cx: &mut Context<Self>) {
        let mut tabs = self.buffer_tabs(cx);
        let pane = self.workspace.read(cx).active_pane().clone();
        let active = pane.read(cx).active();
        let shown = (tabs.iter()).position(|tab| tab.pane == pane && Some(tab.ix) == active);
        self.vim_buffers(&mut tabs, cx, |_| ());
        if let Some(shown) = shown {
            let number = tabs[shown].number;
            self.vim_buffers(&mut [], cx, |buffers| {
                buffers.show(pane.entity_id(), number)
            });
        }
    }

    /// Runs `command`, typed in `editor`: see the module docs. Every
    /// answer, such as `E86`, is `editor`'s vim message.
    pub(super) fn run_vim_buffer_command(
        &mut self,
        editor: &Entity<EditorView>,
        command: BufferCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.vim_buffer_command(editor, command, window, cx) {
            message(editor, error, cx);
        }
    }

    fn vim_buffer_command(
        &mut self,
        editor: &Entity<EditorView>,
        command: BufferCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut tabs = self.buffer_tabs(cx);
        let (pane, current) = match self.tab_of(editor, cx) {
            Some((pane, ix)) => (pane, Some(ix)),
            None => {
                let pane = self.pane().clone();
                let active = pane.read(cx).active();
                (pane, active)
            }
        };
        let buffers = self.vim_buffers(&mut tabs, cx, |buffers| buffers.clone());
        let current = (tabs.iter())
            .find(|tab| tab.pane == pane && Some(tab.ix) == current)
            .map(|tab| tab.number);
        let mut listed: Vec<u32> = tabs.iter().map(|tab| tab.number).collect();
        listed.sort_unstable();
        listed.dedup();
        let last = buffers.numbered.len() as u32;
        let address = |address: BufferAddress| match address {
            BufferAddress::Number(number) if number > last => Err(String::from(E16)),
            BufferAddress::Number(number) => Ok(number),
            BufferAddress::Current => current.ok_or_else(|| String::from(E16)),
            // The last buffer there is, listed or not: a deleted highest
            // one is still what `$` addresses.
            BufferAddress::Last if last > 0 => Ok(last),
            BufferAddress::Last => Err(String::from(E16)),
        };
        let address_of = address;
        // A range's buffers: backwards is E493 before an end past the last
        // buffer is E16, as nvim says.
        let checked_range = |first: BufferAddress, last: BufferAddress| {
            let loose = |address: BufferAddress| match address {
                BufferAddress::Number(number) => Ok(number),
                address => address_of(address),
            };
            if loose(first)? > loose(last)? {
                return Err(String::from(E493));
            }
            Ok((address(first)?, address(last)?))
        };
        let alternate =
            || (buffers.alternate(pane.entity_id(), current)).ok_or_else(|| String::from(E23));
        let root = self.root.clone();
        let named = |name: &str, deleting: bool| {
            let files = (buffers.numbered.iter().zip(1..))
                .filter_map(|(entry, number)| Some((number, entry.path.as_deref()?)))
                .collect::<Vec<_>>();
            let listed_files = (files.iter().copied())
                .filter(|(number, _)| listed.contains(number))
                .collect::<Vec<_>>();
            let rounds: &[&[(u32, &Path)]] = match deleting {
                true => &[&listed_files],
                false => &[&listed_files, &files],
            };
            find_named(name, rounds, root.as_deref())
        };
        match command {
            BufferCommand::Ranged {
                first,
                last,
                command,
            } => {
                checked_range(first, last)?;
                self.vim_buffer_command(editor, *command, window, cx)
            }
            BufferCommand::Ordered {
                first,
                last: end,
                command,
            } => {
                // Only the order, in the editor's lines as nvim reads a count
                // address there: `:1,9bn` has a count that isn't a buffer.
                let (caret, lines) = editor.read_with(cx, |editor, _| {
                    let model = editor.model();
                    let text = model.text();
                    let head = model.primary_selection().head();
                    (
                        text.offset_to_point(head).row + 1,
                        // A newline ending the text starts no line.
                        match text.offset_to_point(text.len()) {
                            end if end.column == 0 && end.row > 0 => end.row,
                            end => end.row + 1,
                        },
                    )
                });
                let resolve = |address: BufferAddress| match address {
                    BufferAddress::Number(number) => number,
                    // The final empty row after a terminating newline is no line.
                    BufferAddress::Current => caret.min(lines),
                    BufferAddress::Last => lines,
                };
                if resolve(first) > resolve(end) {
                    return Err(String::from(E493));
                }
                self.vim_buffer_command(editor, *command, window, cx)
            }
            BufferCommand::List => {
                let alternate = buffers.alternate(pane.entity_id(), current);
                let rows = listed.iter().map(|&number| {
                    let of = || tabs.iter().filter(|tab| tab.number == number);
                    // The caret's line in the current pane's tab, else the
                    // first, an editor's or a viewer's.
                    let here = of().filter(|tab| tab.pane == pane);
                    let line = (here.chain(of()))
                        .find_map(|tab| {
                            let item = &tab.pane.read(cx).tabs()[tab.ix];
                            if let Some(editor) = item.editor() {
                                let model = editor.read(cx).model();
                                let head = model.primary_selection().head();
                                return Some(model.text().offset_to_point(head).row + 1);
                            }
                            let viewer = item.viewer()?.read(cx).model();
                            Some(viewer.text().offset_to_point(viewer.head()).row + 1)
                        })
                        .unwrap_or(1);
                    let entry = buffers.entry(number);
                    let path = entry.and_then(|entry| entry.path.as_deref());
                    let name = match path {
                        Some(path) => root
                            .as_deref()
                            .and_then(|root| path.strip_prefix(root).ok())
                            .unwrap_or(path),
                        None => Path::new(""),
                    };
                    let name = match name.as_os_str().is_empty() {
                        true => tr!("workbench-workbench-vim-no-name").into(),
                        false => name.display().to_string(),
                    };
                    BufferRow {
                        number,
                        current: Some(number) == current,
                        alternate: Some(number) == alternate,
                        active: of().any(|tab| tab.pane.read(cx).active() == Some(tab.ix)),
                        modified: of().any(|tab| tab.dirty),
                        name,
                        line,
                    }
                });
                let pick = Pick::buffers(rows);
                self.show_vim_pick(editor, pick, window, cx);
                Ok(())
            }
            BufferCommand::Switch(target) => {
                let number = match target {
                    BufferTarget::Current => return Ok(()),
                    BufferTarget::Alternate => alternate()?,
                    BufferTarget::Number(number) if number > last => {
                        return Err(tr!("workbench-workbench-vim-e86", number = number).into());
                    }
                    BufferTarget::Number(number) => number,
                    BufferTarget::Name(name) => named(&name, false)?,
                    BufferTarget::Range { last, .. } => address(last)?,
                };
                self.show_vim_buffer(&pane, number, current, window, cx)
                    .map(|_| ())
            }
            BufferCommand::Next(count) | BufferCommand::Previous(count) => {
                if listed.is_empty() {
                    return Err(String::from(E85));
                }
                let next = matches!(command, BufferCommand::Next(_));
                let len = listed.len();
                let at = current.and_then(|current| listed.iter().position(|&n| n == current));
                let at = at.unwrap_or(if next { len - 1 } else { 0 });
                let step = count as usize % len;
                let to = match next {
                    true => (at + step) % len,
                    false => (at + len - step) % len,
                };
                self.show_vim_buffer(&pane, listed[to], current, window, cx)
                    .map(|_| ())
            }
            BufferCommand::First | BufferCommand::Last => {
                let ends = (listed.first().copied()).zip(listed.last().copied());
                let (first, last) = ends.ok_or_else(|| String::from(E85))?;
                let to = match command {
                    BufferCommand::First => first,
                    _ => last,
                };
                self.show_vim_buffer(&pane, to, current, window, cx)
                    .map(|_| ())
            }
            BufferCommand::Delete { target, force } => {
                let deleting: Vec<u32> = match target {
                    BufferTarget::Current => current.into_iter().collect(),
                    BufferTarget::Alternate => vec![alternate()?],
                    BufferTarget::Number(number) => vec![number],
                    BufferTarget::Name(name) => vec![named(&name, true)?],
                    BufferTarget::Range { first, last } => {
                        let (first, last) = checked_range(first, last)?;
                        (first..=last).collect()
                    }
                };
                let deleting: Vec<u32> = (deleting.into_iter())
                    .filter(|number| listed.contains(number))
                    .collect();
                if deleting.is_empty() {
                    return Err(String::from(E516));
                }
                if !force {
                    let dirty = (deleting.iter())
                        .find(|&&n| tabs.iter().any(|tab| tab.number == n && tab.dirty));
                    if let Some(number) = dirty {
                        return Err(tr!("workbench-workbench-vim-e89", number = number).into());
                    }
                }
                // What this pane shows next, if it shows one going.
                let staying = |n: &u32| listed.contains(n) && !deleting.contains(n);
                let next = current
                    .filter(|current| deleting.contains(current))
                    .and_then(|current| {
                        let shown = buffers.shown.get(&pane.entity_id());
                        (shown.into_iter().flatten().rev().copied().find(staying))
                            .or_else(|| {
                                listed.iter().copied().find(|&n| n > current && staying(&n))
                            })
                            .or_else(|| listed.iter().rev().copied().find(staying))
                    });
                // The tabs go once the buffer that replaces them is shown:
                // when its file is still being read, once that lands, so
                // the activations in between don't leave the pane with the
                // closed buffer's neighbour.
                let reading = match next {
                    Some(next) => self.show_vim_buffer(&pane, next, current, window, cx)?,
                    None => None,
                };
                match reading {
                    Some(path) => {
                        self.delete_when_opened(editor, &pane, path, (deleting, force), window, cx)
                    }
                    None => self.delete_vim_buffers(editor, &pane, &deleting, force, window, cx),
                }
                Ok(())
            }
        }
    }

    /// Runs the deletion of `deleting` once `pane` has opened `path`, or
    /// failed to.
    fn delete_when_opened(
        &mut self,
        editor: &Entity<EditorView>,
        pane: &Entity<Pane>,
        path: PathBuf,
        (deleting, force): (Vec<u32>, bool),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (id, editor) = (cx.entity_id(), editor.clone());
        let key = {
            let all = cx.global_mut::<VimBuffers>();
            all.seq += 1;
            all.seq
        };
        let subscription = cx.observe_in(pane, window, move |workbench, pane, window, cx| {
            // Whatever else the pane is shown or opens meanwhile, the read of
            // the replacement runs on until it lands or fails.
            if pane.read(cx).reading().any(|reading| reading == path) {
                return;
            }
            if let Some(window) = cx.global_mut::<VimBuffers>().windows.get_mut(&id) {
                window.waiting.remove(&key);
            }
            // The replacement didn't open (its read failed, or a Save As
            // holding it ended without it): the buffers stay.
            let opened =
                (pane.read(cx).tabs().iter()).any(|tab| tab.path.as_deref() == Some(&path));
            if opened {
                workbench.delete_vim_buffers(&editor, &pane, &deleting, force, window, cx);
            }
        });
        if let Some(window) = cx.global_mut::<VimBuffers>().windows.get_mut(&id) {
            window.waiting.insert(key, subscription);
        }
    }

    /// Closes every tab of the buffers `deleting`, in every pane, with
    /// `pane`, the one the command ran in, keeping the focus.
    fn delete_vim_buffers(
        &mut self,
        editor: &Entity<EditorView>,
        pane: &Entity<Pane>,
        deleting: &[u32],
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut tabs = self.buffer_tabs(cx);
        self.vim_buffers(&mut tabs, cx, |_| ());
        // A deletion that waited for a file to be read finds the buffers as
        // they are now: one changed meanwhile is still refused without `!`.
        if !force {
            let dirty =
                (deleting.iter()).find(|&&n| tabs.iter().any(|tab| tab.number == n && tab.dirty));
            if let Some(number) = dirty {
                let error = tr!("workbench-workbench-vim-e89", number = number);
                return message(editor, error, cx);
            }
        }
        for tab in tabs.into_iter().rev() {
            if deleting.contains(&tab.number) {
                tab.pane.update(cx, |pane, cx| {
                    pane.take_item(tab.ix, window, cx);
                    cx.notify();
                });
            }
        }
        // Closing another pane's shown tab focuses its neighbour:
        // focus goes back to this pane, unless it closed.
        if self.panes(cx).contains(pane) && !pane.read(cx).tabs().is_empty() {
            let focus = pane.read(cx).focus_target(cx);
            window.focus(&focus, cx);
        }
        if deleting.len() > 1 {
            let report = tr!(
                "workbench-workbench-vim-buffers-deleted",
                count = deleting.len()
            );
            editor.update(cx, |editor, cx| editor.report_vim_message(report, cx));
        }
    }

    /// Shows buffer `number` in `pane`, whose shown buffer is `current`: its
    /// tab there, or else a new one, of its file or sharing another pane's
    /// untitled text; noted as what the pane shows now, even where the
    /// file was opened at once. The file still being read, if it is, which
    /// is noted as it lands.
    fn show_vim_buffer(
        &mut self,
        pane: &Entity<Pane>,
        number: u32,
        current: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<PathBuf>, String> {
        if Some(number) == current {
            return Ok(None);
        }
        let mut tabs = self.buffer_tabs(cx);
        let entry = self.vim_buffers(&mut tabs, cx, |buffers| buffers.entry(number).cloned());
        let Some(entry) = entry else {
            return Err(tr!("workbench-workbench-vim-e86", number = number).into());
        };
        let here = (tabs.iter()).find(|tab| tab.pane == *pane && tab.number == number);
        if let Some(tab) = here {
            pane.update(cx, |pane, cx| pane.activate(tab.ix, window, cx));
        } else if let Some(path) = entry.path {
            pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx));
            // Opened at once, from text another pane holds or a tab there
            // already: the switch is done, and noted below.
            if pane.read(cx).is_opening(&path) {
                return Ok(Some(path));
            }
        } else {
            let other = (tabs.iter())
                .find(|tab| tab.number == number)
                .and_then(|tab| tab.pane.read(cx).tabs()[tab.ix].editor().cloned());
            let Some(other) = other else {
                return Err(tr!("workbench-workbench-vim-e86", number = number).into());
            };
            let model = other.read(cx).model().split();
            pane.update(cx, |pane, cx| {
                pane.add(model, cx);
                let ix = pane.tabs().len() - 1;
                pane.activate(ix, window, cx);
            });
        }
        let mut tabs = self.buffer_tabs(cx);
        let pane = pane.entity_id();
        self.vim_buffers(&mut tabs, cx, |buffers| buffers.show(pane, number));
        Ok(None)
    }
}

/// The one buffer `name` matches among the first of `rounds` where any
/// does, as nvim's `buflist_findpat` finds it: see the module docs. A path
/// in `root` is matched relative to it, as well as whole.
fn find_named(name: &str, rounds: &[&[(u32, &Path)]], root: Option<&Path>) -> Result<u32, String> {
    let fold = !name.chars().any(char::is_uppercase);
    // On Windows `/` and `\` separate alike, as nvim compares names there.
    let folded = |text: &str| {
        let text = match fold {
            true => text.to_lowercase(),
            false => text.to_owned(),
        };
        if cfg!(windows) {
            text.replace('\\', "/")
        } else {
            text
        }
    };
    let pattern = folded(name);
    let matches = |text: &str, attempt: u8| {
        let text = folded(text);
        match attempt {
            0 => text.contains(&pattern),
            1 => text.starts_with(&pattern),
            2 => text.ends_with(&pattern),
            _ => text == pattern,
        }
    };
    let mut several = false;
    for round in rounds {
        for attempt in 0..4 {
            let found: Vec<u32> = (round.iter())
                .filter(|(_, path)| {
                    let short = root.and_then(|root| path.strip_prefix(root).ok());
                    short.is_some_and(|short| matches(&short.to_string_lossy(), attempt))
                        || matches(&path.to_string_lossy(), attempt)
                })
                .map(|(number, _)| *number)
                .collect();
            match found.as_slice() {
                [one] => return Ok(*one),
                [] => {}
                _ => several = true,
            }
        }
        if several {
            break;
        }
    }
    Err(match several {
        true => tr!("workbench-workbench-vim-e93", name = name).into(),
        false => tr!("workbench-workbench-vim-e94", name = name).into(),
    })
}

#[cfg(test)]
#[path = "vim_buffers_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "vim_buffers_transition_tests.rs"]
mod transition_tests;

#[cfg(test)]
#[path = "vim_buffers_followup_tests.rs"]
mod followup_tests;

#[cfg(test)]
#[path = "vim_buffers_cancel_tests.rs"]
mod cancel_tests;
