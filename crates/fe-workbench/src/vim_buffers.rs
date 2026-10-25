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
//! pane when the last goes.
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
use fe_vim::{BufferAddress, BufferCommand, BufferTarget};
use gpui::{App, Context, Entity, EntityId, Global, Subscription, WeakEntity, Window};
use kiln_shell::ItemHandle;
use kiln_shell::PaneView as _;

use super::message;
use crate::{Pane, Workbench};

const E16: &str = "E16: Invalid range";
const E23: &str = "E23: No alternate file";
const E85: &str = "E85: There is no listed buffer";
const E516: &str = "E516: No buffers were deleted";

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
    entry: Logged,
}

/// Notes that `item` of `pane` had `kind` happen to it. The window reads
/// them as it next follows its workbench, in the order they happened,
/// whether or not the pane is then still in it.
pub(crate) fn record(pane: &WeakEntity<Pane>, kind: Kind, item: &dyn ItemHandle, cx: &mut App) {
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
}

impl Global for VimBuffers {}

struct WindowBuffers {
    workbench: WeakEntity<Workbench>,
    /// The panes this window has had, so a pane closed since its last
    /// transition is still its.
    panes: std::collections::HashSet<EntityId>,
    /// To the window's workspace changing.
    _watch: Option<Subscription>,
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
        cx.default_global::<VimBuffers>();
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
            panes: Default::default(),
            buffers: Buffers::default(),
        });
        window.panes.extend(live.iter().copied());
        if watch.is_some() {
            window._watch = watch;
        }
        // Known limit (T337 carries the fix): a transition is owned by the
        // window that has seen its pane. A pane added and removed within one
        // update before any registry read was seen by none, so with several
        // windows it replays into whichever reads next, which may not be the
        // one it was in. Panes don't yet carry their workbench.
        let pending = std::mem::take(&mut all.pending);
        let (mine, rest): (Vec<_>, Vec<_>) = pending.into_iter().partition(|pending| {
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
        for Pending { pane, entry } in mine {
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
            BufferAddress::Number(number) if number > last => Err(E16.to_owned()),
            BufferAddress::Number(number) => Ok(number),
            BufferAddress::Current => current.ok_or_else(|| E16.to_owned()),
            BufferAddress::Last => listed.last().copied().ok_or_else(|| E16.to_owned()),
        };
        let alternate =
            || (buffers.alternate(pane.entity_id(), current)).ok_or_else(|| E23.to_owned());
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
            BufferCommand::Switch(target) => {
                let number = match target {
                    BufferTarget::Current => return Ok(()),
                    BufferTarget::Alternate => alternate()?,
                    BufferTarget::Number(number) if number > last => {
                        return Err(format!("E86: Buffer {number} does not exist"));
                    }
                    BufferTarget::Number(number) => number,
                    BufferTarget::Name(name) => named(&name, false)?,
                    BufferTarget::Range { last, .. } => address(last)?,
                };
                self.show_vim_buffer(&pane, number, current, window, cx)
            }
            BufferCommand::Next(count) | BufferCommand::Previous(count) => {
                if listed.is_empty() {
                    return Err(E85.to_owned());
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
            }
            BufferCommand::First | BufferCommand::Last => {
                let ends = (listed.first().copied()).zip(listed.last().copied());
                let (first, last) = ends.ok_or_else(|| E85.to_owned())?;
                let to = match command {
                    BufferCommand::First => first,
                    _ => last,
                };
                self.show_vim_buffer(&pane, to, current, window, cx)
            }
            BufferCommand::Delete { target, force } => {
                let deleting: Vec<u32> = match target {
                    BufferTarget::Current => current.into_iter().collect(),
                    BufferTarget::Alternate => vec![alternate()?],
                    BufferTarget::Number(number) => vec![number],
                    BufferTarget::Name(name) => vec![named(&name, true)?],
                    BufferTarget::Range { first, last } => {
                        let (first, last) = (address(first)?, address(last)?);
                        if first > last {
                            return Err("E493: Backwards range given".to_owned());
                        }
                        (first..=last).collect()
                    }
                };
                let deleting: Vec<u32> = (deleting.into_iter())
                    .filter(|number| listed.contains(number))
                    .collect();
                if deleting.is_empty() {
                    return Err(E516.to_owned());
                }
                if !force {
                    let dirty = (deleting.iter())
                        .find(|&&n| tabs.iter().any(|tab| tab.number == n && tab.dirty));
                    if let Some(number) = dirty {
                        return Err(format!(
                            "E89: No write since last change for buffer {number} (add ! to override)"
                        ));
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
                if let Some(next) = next {
                    self.show_vim_buffer(&pane, next, current, window, cx)?;
                }
                let mut tabs = self.buffer_tabs(cx);
                self.vim_buffers(&mut tabs, cx, |_| ());
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
                if self.panes(cx).contains(&pane) && !pane.read(cx).tabs().is_empty() {
                    let focus = pane.read(cx).focus_target(cx);
                    window.focus(&focus, cx);
                }
                if deleting.len() > 1 {
                    let report = format!("{} buffers deleted", deleting.len());
                    editor.update(cx, |editor, cx| editor.report_vim_message(report, cx));
                }
                Ok(())
            }
        }
    }

    /// Shows buffer `number` in `pane`, whose shown buffer is `current`: its
    /// tab there, or else a new one, of its file or sharing another pane's
    /// untitled text; noted as what the pane shows now, but for a file
    /// still to be read, which is noted as it lands.
    fn show_vim_buffer(
        &mut self,
        pane: &Entity<Pane>,
        number: u32,
        current: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if Some(number) == current {
            return Ok(());
        }
        let mut tabs = self.buffer_tabs(cx);
        let entry = self.vim_buffers(&mut tabs, cx, |buffers| buffers.entry(number).cloned());
        let Some(entry) = entry else {
            return Err(format!("E86: Buffer {number} does not exist"));
        };
        let here = (tabs.iter()).find(|tab| tab.pane == *pane && tab.number == number);
        if let Some(tab) = here {
            pane.update(cx, |pane, cx| pane.activate(tab.ix, window, cx));
        } else if let Some(path) = entry.path {
            pane.update(cx, |pane, cx| pane.open(path, window, cx));
            return Ok(());
        } else {
            let other = (tabs.iter())
                .find(|tab| tab.number == number)
                .and_then(|tab| tab.pane.read(cx).tabs()[tab.ix].editor().cloned());
            let Some(other) = other else {
                return Err(format!("E86: Buffer {number} does not exist"));
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
        Ok(())
    }
}

/// The one buffer `name` matches among the first of `rounds` where any
/// does, as nvim's `buflist_findpat` finds it: see the module docs. A path
/// in `root` is matched relative to it, as well as whole.
fn find_named(name: &str, rounds: &[&[(u32, &Path)]], root: Option<&Path>) -> Result<u32, String> {
    let fold = !name.chars().any(char::is_uppercase);
    let folded = |text: &str| match fold {
        true => text.to_lowercase(),
        false => text.to_owned(),
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
        true => format!("E93: More than one match for {name}"),
        false => format!("E94: No matching buffer for {name}"),
    })
}

#[cfg(test)]
#[path = "vim_buffers_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "vim_buffers_transition_tests.rs"]
mod transition_tests;
