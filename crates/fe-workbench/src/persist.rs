//! Saving View > Toggle Vim Mode, View > Toggle Status Bar and Spelling: Toggle
//! Spell Check to the user's `settings.toml`, as `vim.enabled`,
//! `status_bar.visible` and `editor.spell_check`, so they outlast a restart.
//! The settings watcher then sees the write, as it would a hand edit.
//!
//! Nothing is saved until the host calls [`save_toggles_to`]. Each toggle
//! queues the value it left, taken in the dispatch that ran it, so a reload
//! landing before the write can't change what's saved. Writes run on the background
//! executor, one at a time, and each key's last toggle wins: however many
//! toggles queue up, the file ends at the last one's value. Quitting waits
//! for them, up to GPUI's shutdown timeout. A failed write is logged; the
//! toggle still applies for the session.
//!
//! Saves and Settings… need no coordination: both put the file in place
//! whole, through a temporary file, so neither reads the other's half
//! written, and a creation that fails removes only its temporary file.

use std::{cell::Cell, path::PathBuf, rc::Rc};

use fe_gpui::{commands::RunCommand, menu::MenuCommand};
use gpui::{App, Global};
use kiln_shell::persist::{SettingKey, SettingsPath, Writer, setting_writer};

/// The setting View > Toggle Vim Mode saves.
const VIM: SettingKey = &["vim", "enabled"];
/// The setting View > Toggle Status Bar saves.
const STATUS_BAR: SettingKey = &["status_bar", "visible"];
/// The setting `spell.toggle` saves.
const SPELL_CHECK: SettingKey = &["editor", "spell_check"];

/// The command whose successful runs save [`VIM`].
const VIM_TOGGLE: &str = "vim.toggle";

/// The app's [`Writes`], once [`save_toggles_to`] is called.
struct Saver(Writes);

impl Global for Saver {}

/// The settings file the writes go to.
struct SaveTarget(SettingsPath);

impl Global for SaveTarget {}

/// The writes waiting, through kiln-shell's writer
/// ([`kiln_shell::persist`]), which takes them to the writer one at a
/// time, each key's last value winning.
type Writes = Writer<SettingKey, bool>;

/// From now on, saves each toggle of vim mode and of the status bar to the
/// settings file at `path`, editing it in place, or creating it from the
/// settings template. Called again, later writes go to the new `path`.
///
/// Call after [`fe_gpui::init`]. Observers of a `vim.toggle` don't get the
/// app, so app listeners for the actions that run it (`RunCommand`, and
/// `MenuCommand` with no window open) read the switch in the same
/// dispatch: GPUI runs the later registered first, so before fe-gpui's
/// `RunCommand` handler, and before or after the menu's, whichever
/// installed first.
pub fn save_toggles_to(cx: &mut App, path: PathBuf) {
    if let Some(SaveTarget(target)) = cx.try_global::<SaveTarget>() {
        target.set(path);
        return;
    }
    let target = SettingsPath::new(path);
    let writes = setting_writer(target.clone(), fe_config::USER_SETTINGS_TEMPLATE, cx);
    cx.set_global(Saver(writes.clone()));
    cx.set_global(SaveTarget(target));
    let vim = Rc::new(VimToggle::default());
    cx.on_app_quit({
        let (writes, vim) = (writes.clone(), vim.clone());
        move |cx| {
            vim.save_unread(&writes, cx);
            writes.finish()
        }
    })
    .detach();

    cx.on_action({
        let (writes, vim) = (writes.clone(), vim.clone());
        move |run: &RunCommand, cx| {
            vim.listen(run.0.command.as_str() == VIM_TOGGLE, &writes, cx);
            cx.propagate();
        }
    });
    cx.on_action({
        let (writes, vim) = (writes.clone(), vim.clone());
        move |_: &MenuCommand, cx| {
            // Which command the item runs is the menu's to know.
            vim.listen(true, &writes, cx);
            cx.propagate();
        }
    });
    let app = cx.to_async();
    fe_gpui::commands::observe_invocations(cx, move |invocation| {
        if invocation.command.as_str() != VIM_TOGGLE {
            return;
        }
        match vim.before.take() {
            Some(before) => writes.save(VIM, !before),
            None => {
                // A listener running after the command reads it; failing
                // that, as no action of theirs ran it, the next turn.
                vim.unread.set(true);
                let (writes, vim) = (writes.clone(), vim.clone());
                app.spawn(async move |cx| cx.update(|cx| vim.save_unread(&writes, cx)))
                    .detach();
            }
        }
    });
}

/// What the action listeners know of a `vim.toggle` being dispatched.
#[derive(Default)]
struct VimToggle {
    /// The switch before the toggle, read in the dispatch, until it ends.
    before: Cell<Option<bool>>,
    /// Whether a toggle ran with `before` unknown, and isn't saved yet.
    unread: Cell<bool>,
}

impl VimToggle {
    /// For an action being dispatched: saves the switch if a toggle ran
    /// before this listener did, else notes it for a toggle the action
    /// may run.
    fn listen(self: &Rc<Self>, may_toggle: bool, writes: &Writes, cx: &mut App) {
        if self.unread.get() {
            self.save_unread(writes, cx);
        } else if may_toggle {
            self.before.set(Some(fe_gpui::vim_enabled(cx)));
            let this = self.clone();
            cx.defer(move |_| this.before.set(None));
        }
    }

    /// Saves the switch, if a toggle ran unread.
    fn save_unread(&self, writes: &Writes, cx: &App) {
        if self.unread.take() {
            writes.save(VIM, fe_gpui::vim_enabled(cx));
        }
    }
}

/// Saves the status bar's visibility, if [`save_toggles_to`] was called.
pub(crate) fn save_status_bar(visible: bool, cx: &App) {
    if let Some(Saver(writes)) = cx.try_global::<Saver>() {
        writes.save(STATUS_BAR, visible);
    }
}

/// Saves whether spell check is on, if [`save_toggles_to`] was called.
pub(crate) fn save_spell_check(on: bool, cx: &App) {
    if let Some(Saver(writes)) = cx.try_global::<Saver>() {
        writes.save(SPELL_CHECK, on);
    }
}

#[cfg(test)]
#[path = "persist_tests.rs"]
mod tests;
