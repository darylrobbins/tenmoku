//! Auto-save: the workbench feeds fe-editor's [`AutoSavePolicy`], built
//! from `editor.auto_save` and `editor.auto_save_delay`, and saves what it
//! says is due.
//!
//! Each open buffer, however many editors show it, is one key, its
//! [`BufferId`], so a buffer in splits saves once. What the policy hears:
//!
//! - an edit, when the buffer's version moves while it's dirty, with
//!   whether it has a path, whether its editors are all read-only and
//!   whether one of them is in vim's insert or replace mode; those facts
//!   changing without an edit (insert mode entered, the editors made
//!   read-only) are told as an edit too, unless a failed save holds the
//!   buffer, and so is a buffer first seen dirty (restored from a backup);
//! - insert mode ending, when the last of its editors in insert or replace
//!   mode leaves it, or closes; with vim on, the first normal-mode edit is
//!   told as one too, so `on_insert_leave` saves after a normal-mode
//!   change that no insert came before;
//! - vim turning off, as the buffer closing and opening again, its holds
//!   kept, so `on_insert_leave` goes back to saving on focus changes;
//! - an editor losing focus (to another tab or pane, a panel, a palette or
//!   the vim line), from GPUI's focus-out, which lands at the next frame;
//! - the window deactivating;
//! - a save or a reload, when the buffer's disk stamp moves or it turns
//!   clean, and an auto-save's own result;
//! - its last editor closing.
//!
//! After each, and when the one timer for the policy's next deadline
//! fires, each due buffer saves through [`EditorView::save_with`] with the
//! mode's reason, which never prompts, once it's checked it's still dirty,
//! has a path, isn't read-only and, but on focus or window changes, isn't
//! mid-insert. One auto-save of a buffer runs at a time: one due while
//! another runs (an edit came, or a save fix edited) waits for it, and
//! runs once it has succeeded if the buffer is still dirty and auto-save
//! is still on; after a failure, or a change of mode, it doesn't. A save refused because the file
//! changed on disk shows one warning toast and holds the buffer dirty,
//! with no further auto-save, until the user saves it (cmd-s asks about
//! overwriting) or it's reloaded, as VS Code does; any other failure shows
//! an error toast and waits for the next edit. Hot exit is independent: a
//! buffer an auto-save leaves clean loses its backup as any saved one
//! does.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use fe_buffer::{BufferId, DiskStamp, Version};
use fe_config::settings::AutoSave as Setting;
use fe_editor::{
    SaveError,
    auto_save::{AutoSaveMode, AutoSavePolicy, Facts, SaveReason},
};
use fe_gpui::{EditorSettings, EditorView, pane_item::file_name};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, EntityId, Focusable as _, Subscription,
    Task, Window,
};

use crate::{
    Workbench,
    workbench::toast::{self, Toast},
};

/// The workbench's part: the policy and what it was last told.
pub(crate) struct AutoSave {
    policy: AutoSavePolicy<BufferId>,
    /// The workbench's window, where saves run and focus is followed.
    window: AnyWindowHandle,
    /// Per open buffer: what was last seen of it.
    buffers: HashMap<BufferId, Seen>,
    /// Per open editor: its buffer, and its focus.
    editors: HashMap<EntityId, Watched>,
    /// The wait for the policy's next deadline, and that deadline.
    timer: Option<(Instant, Task<()>)>,
    /// To the window activating and deactivating.
    _activation: Subscription,
    /// The buffers auto-saves were started for, oldest first.
    #[cfg(test)]
    pub(crate) saves: Vec<BufferId>,
}

/// A buffer as last seen.
struct Seen {
    version: Version,
    stamp: Option<DiskStamp>,
    dirty: bool,
    facts: Facts,
    /// Whether vim was on in its editors.
    vim_on: bool,
    /// Whether the policy knows vim is on for it.
    vim_told: bool,
    /// An auto-save was refused because its file changed on disk: until
    /// it's saved or reloaded, turning clean otherwise (an undo) isn't told,
    /// so the policy goes on holding it.
    conflict: bool,
    /// Another auto-save failure holds it until the next edit.
    failed: bool,
    /// An auto-save of it succeeded since last seen.
    landed: bool,
    /// An auto-save of it runs.
    saving: bool,
    /// Another came due meanwhile.
    again: bool,
}

impl Seen {
    fn held(&self) -> bool {
        self.conflict || self.failed
    }
}

/// An open editor as last seen.
struct Watched {
    buffer: BufferId,
    /// To focus leaving it, once subscribed.
    focus_out: Option<Subscription>,
}

/// The policy's mode for `settings`.
fn mode(settings: &EditorSettings) -> AutoSaveMode {
    match settings.auto_save {
        Setting::Off => AutoSaveMode::Off,
        Setting::AfterDelay => {
            AutoSaveMode::AfterDelay(Duration::from_millis(settings.auto_save_delay))
        }
        Setting::OnFocusChange => AutoSaveMode::OnFocusChange,
        Setting::OnWindowChange => AutoSaveMode::OnWindowChange,
        Setting::OnInsertLeave => AutoSaveMode::OnInsertLeave,
    }
}

/// Why an auto-save in `mode` saves.
fn reason(mode: AutoSaveMode) -> SaveReason {
    match mode {
        AutoSaveMode::AfterDelay(_) => SaveReason::AutoDelay,
        AutoSaveMode::OnWindowChange => SaveReason::AutoWindow,
        AutoSaveMode::OnInsertLeave => SaveReason::AutoInsertLeave,
        AutoSaveMode::Off | AutoSaveMode::OnFocusChange => SaveReason::AutoFocus,
    }
}

/// With vim on, whether `editor` is in insert or replace mode.
fn insert_mode(editor: &EditorView) -> Option<bool> {
    editor
        .vim_mode()
        .map(|mode| matches!(mode, "insert" | "replace"))
}

/// What's true of a buffer now, shown in `shown`.
struct Now {
    version: Version,
    stamp: Option<DiskStamp>,
    dirty: bool,
    facts: Facts,
    vim_on: bool,
}

impl Now {
    fn of(shown: &[Entity<EditorView>], cx: &App) -> Self {
        let model = shown[0].read(cx).model();
        let text = model.buffer();
        let modes: Vec<Option<bool>> = (shown.iter())
            .map(|editor| insert_mode(editor.read(cx)))
            .collect();
        Self {
            version: text.version(),
            stamp: text.disk_stamp(),
            dirty: model.is_dirty(),
            facts: Facts {
                has_path: model.path().is_some(),
                read_only: shown.iter().all(|editor| editor.read(cx).is_read_only()),
                vim_insert: modes.contains(&Some(true)),
            },
            vim_on: modes.iter().any(Option::is_some),
        }
    }
}

impl AutoSave {
    /// Auto-saves in `window` as `settings` say, from now on.
    pub(crate) fn new(
        settings: &EditorSettings,
        window: &mut Window,
        cx: &mut Context<Workbench>,
    ) -> Self {
        let activation = cx.observe_window_activation(window, |workbench, window, cx| {
            if !window.is_window_active() {
                workbench.sync_auto_save(cx);
                workbench.auto_save.policy.window_blurred();
                workbench.run_auto_save(cx);
            }
        });
        Self {
            policy: AutoSavePolicy::new(mode(settings)),
            window: window.window_handle(),
            buffers: HashMap::new(),
            editors: HashMap::new(),
            timer: None,
            _activation: activation,
            #[cfg(test)]
            saves: Vec::new(),
        }
    }

    /// Tells the policy `buffer` changed as `now` says, from `seen`.
    fn sync_buffer(&mut self, buffer: BufferId, now: &Now, at: Instant) {
        let policy = &mut self.policy;
        let Some(seen) = self.buffers.get_mut(&buffer) else {
            // First seen: dirty already, it's due as if just edited.
            if now.dirty {
                policy.edited(buffer, at, now.facts);
            }
            let seen = Seen {
                version: now.version,
                stamp: now.stamp,
                dirty: now.dirty,
                facts: now.facts,
                vim_on: now.vim_on,
                vim_told: now.dirty && now.facts.vim_insert,
                conflict: false,
                failed: false,
                landed: false,
                saving: false,
                again: false,
            };
            self.buffers.insert(buffer, seen);
            return;
        };
        // Vim off: the policy forgets it was on, keeping any hold.
        if seen.vim_on && !now.vim_on {
            policy.closed(buffer);
            seen.vim_told = false;
            if seen.dirty {
                policy.edited(buffer, at, now.facts);
            }
            if seen.held() {
                policy.save_failed(buffer, seen.conflict);
            }
        }
        // A save or a reload landed, or the user made it clean.
        if seen.stamp != now.stamp || std::mem::take(&mut seen.landed) {
            (seen.conflict, seen.failed) = (false, false);
            policy.saved(buffer);
            if now.dirty {
                policy.edited(buffer, at, now.facts);
            }
        } else if seen.dirty && !now.dirty && !seen.conflict {
            seen.failed = false;
            policy.saved(buffer);
        }
        if seen.version != now.version && now.dirty {
            if now.vim_on && !now.facts.vim_insert && !seen.vim_told {
                policy.insert_left(buffer, at);
            }
            seen.vim_told |= now.vim_on;
            seen.failed = false;
            policy.edited(buffer, at, now.facts);
        } else if now.dirty && !seen.held() {
            let entered = now.facts.vim_insert && !seen.facts.vim_insert;
            let other = (now.facts.has_path, now.facts.read_only)
                != (seen.facts.has_path, seen.facts.read_only);
            if entered || other {
                policy.edited(buffer, at, now.facts);
            }
        }
        // Left with vim still on: off, the reset above told it all.
        if seen.facts.vim_insert && !now.facts.vim_insert && now.vim_on {
            policy.insert_left(buffer, at);
            seen.vim_told = true;
        }
        (seen.version, seen.stamp, seen.dirty) = (now.version, now.stamp, now.dirty);
        (seen.facts, seen.vim_on) = (now.facts, now.vim_on);
    }
}

impl Workbench {
    /// Takes `editor.auto_save` and `editor.auto_save_delay` from reloaded
    /// `settings`.
    pub(crate) fn reload_auto_save(&mut self, settings: &EditorSettings, cx: &mut Context<Self>) {
        let mode = mode(settings);
        if mode != self.auto_save.policy.mode() {
            self.auto_save.policy.set_mode(mode);
            // What came due under the old mode is the new mode's to say.
            for seen in self.auto_save.buffers.values_mut() {
                seen.again = false;
            }
            self.run_auto_save(cx);
        }
    }

    /// The open buffers, each with the editors showing it.
    fn open_buffers(&self, cx: &App) -> Vec<(BufferId, Vec<Entity<EditorView>>)> {
        let mut buffers: Vec<(BufferId, Vec<Entity<EditorView>>)> = Vec::new();
        for editor in self.open_editors(cx) {
            let buffer = editor.read(cx).model().buffer().id();
            match buffers.iter_mut().find(|(id, _)| *id == buffer) {
                Some((_, shown)) => shown.push(editor),
                None => buffers.push((buffer, vec![editor])),
            }
        }
        buffers
    }

    /// Tells the policy what changed in the open editors since last looked,
    /// then saves what's due.
    pub(crate) fn sync_auto_save(&mut self, cx: &mut Context<Self>) {
        let at = cx.background_executor().now();
        let buffers = self.open_buffers(cx);
        let open: HashSet<EntityId> = (buffers.iter())
            .flat_map(|(_, shown)| shown.iter().map(Entity::entity_id))
            .collect();
        self.auto_save.editors.retain(|id, _| open.contains(id));
        let closed: Vec<BufferId> = (self.auto_save.buffers.keys())
            .filter(|id| !buffers.iter().any(|(buffer, _)| buffer == *id))
            .copied()
            .collect();
        for buffer in closed {
            self.auto_save.buffers.remove(&buffer);
            self.auto_save.policy.closed(buffer);
        }
        for (buffer, shown) in &buffers {
            for editor in shown {
                if !self.auto_save.editors.contains_key(&editor.entity_id()) {
                    self.watch_focus(editor, cx);
                    let watched = Watched {
                        buffer: *buffer,
                        focus_out: None,
                    };
                    self.auto_save.editors.insert(editor.entity_id(), watched);
                }
            }
            let now = Now::of(shown, cx);
            self.auto_save.sync_buffer(*buffer, &now, at);
        }
        self.run_auto_save(cx);
    }

    /// Follows focus leaving `editor`, from the end of this update, when
    /// the window is free.
    fn watch_focus(&mut self, editor: &Entity<EditorView>, cx: &mut Context<Self>) {
        let handle = editor.focus_handle(cx);
        let id = editor.entity_id();
        let window = self.auto_save.window;
        let workbench = cx.weak_entity();
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| {
                    workbench.update(cx, |workbench, cx| {
                        let focus_out =
                            cx.on_focus_out(&handle, window, move |workbench, _, _, cx| {
                                workbench.auto_save_focus_lost(id, cx)
                            });
                        if let Some(watched) = workbench.auto_save.editors.get_mut(&id) {
                            watched.focus_out = Some(focus_out);
                        }
                    })
                })
                .ok();
        });
    }

    /// Focus has left editor `id`.
    fn auto_save_focus_lost(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.sync_auto_save(cx);
        if let Some(watched) = self.auto_save.editors.get(&id) {
            self.auto_save.policy.focus_lost(watched.buffer);
            self.run_auto_save(cx);
        }
    }

    /// Saves each buffer the policy says is due, and waits for its next
    /// deadline.
    fn run_auto_save(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        for buffer in self.auto_save.policy.due(now) {
            self.auto_save_buffer(buffer, cx);
        }
        let next = self.auto_save.policy.next_deadline();
        if self.auto_save.timer.as_ref().map(|(at, _)| *at) == next {
            return;
        }
        self.auto_save.timer = next.map(|at| {
            let wait = at.saturating_duration_since(now);
            let timer = cx.spawn(async move |workbench, cx| {
                cx.background_executor().timer(wait).await;
                workbench
                    .update(cx, |workbench, cx| {
                        if let Some((_, timer)) = workbench.auto_save.timer.take() {
                            timer.detach();
                        }
                        workbench.sync_auto_save(cx);
                    })
                    .ok();
            });
            (at, timer)
        });
    }

    /// Saves `buffer` through one of its editors, then tells the policy how
    /// that went; or, with one of its auto-saves running, once that's done.
    fn auto_save_buffer(&mut self, buffer: BufferId, cx: &mut Context<Self>) {
        let shown = (self.open_buffers(cx).into_iter())
            .find_map(|(id, shown)| (id == buffer).then_some(shown));
        let (Some(shown), Some(seen)) = (shown, self.auto_save.buffers.get_mut(&buffer)) else {
            return;
        };
        if seen.saving {
            seen.again = true;
            return;
        }
        let now = Now::of(&shown, cx);
        let mode = self.auto_save.policy.mode();
        let mid_insert = now.facts.vim_insert
            && matches!(
                mode,
                AutoSaveMode::AfterDelay(_) | AutoSaveMode::OnInsertLeave
            );
        if mode == AutoSaveMode::Off
            || !now.dirty
            || !now.facts.has_path
            || now.facts.read_only
            || mid_insert
            || seen.held()
        {
            return;
        }
        seen.saving = true;
        #[cfg(test)]
        self.auto_save.saves.push(buffer);
        // Through a split that's inserting, if any: `save_with` then runs no
        // hook or fixes, which would edit the shared buffer under the insert.
        let editor = (shown.iter())
            .find(|editor| insert_mode(editor.read(cx)) == Some(true))
            .unwrap_or(&shown[0])
            .clone();
        let reason = reason(mode);
        let window = self.auto_save.window;
        cx.spawn(async move |workbench, cx| {
            let saving = cx.update_window(window, |_, window, cx| {
                editor.update(cx, |editor, cx| editor.save_with(reason, window, cx))
            });
            let saved = match saving {
                Ok(saving) => saving.await,
                Err(error) => Err(error),
            };
            workbench
                .update(cx, |workbench, cx| {
                    workbench.auto_saved(buffer, &editor, saved, cx)
                })
                .ok();
        })
        .detach();
    }

    /// An auto-save of `buffer` through `editor` is over.
    fn auto_saved(
        &mut self,
        buffer: BufferId,
        editor: &Entity<EditorView>,
        saved: gpui::Result<()>,
        cx: &mut Context<Self>,
    ) {
        let Some(seen) = self.auto_save.buffers.get_mut(&buffer) else {
            return;
        };
        seen.saving = false;
        let again = std::mem::take(&mut seen.again) && saved.is_ok();
        match saved {
            Ok(()) => seen.landed = true,
            Err(error) => {
                let conflict = matches!(
                    error
                        .downcast_ref::<std::io::Error>()
                        .and_then(|error| error.get_ref())
                        .and_then(|inner| inner.downcast_ref::<SaveError>()),
                    Some(SaveError::ChangedOnDisk)
                );
                seen.conflict |= conflict;
                seen.failed |= !conflict;
                self.auto_save.policy.save_failed(buffer, conflict);
                let name = file_name(editor.read(cx).model().path());
                if conflict {
                    let message = format!("Auto save skipped: {name} changed on disk");
                    toast::show(cx, Toast::warning(message));
                } else {
                    log::warn!("can't auto-save {name}: {error:#}");
                    let message = format!("Can't auto save {name}");
                    toast::show(cx, Toast::error(message).detail(format!("{error:#}")));
                }
            }
        }
        self.sync_auto_save(cx);
        // Due while that ran, and not saved since.
        if again {
            self.auto_save_buffer(buffer, cx);
        }
    }
}

#[cfg(test)]
#[path = "auto_save_tests.rs"]
mod auto_save_tests;
