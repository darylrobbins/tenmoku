//! Which-key in the editor: once a prefix vim or its remaps hold (`z`, `Z`,
//! `<leader>`, a map's first keys) has waited `vim.which_key_delay_ms` for
//! its next key, a panel along the editor's bottom lists the keys that go on
//! from it, each with its title ([`fe_vim::VimState::which_key`]), as which-key.nvim
//! shows it. A key that comes first ends the wait, so a sequence typed
//! faster never shows it. Once shown, the next key that leaves a prefix
//! waiting repaints it at once with that prefix's keys (`<leader>`, then
//! `f`); any other key, a blur, a mode change or the prefix resolving
//! closes it. It never takes focus or keys.
//!
//! A prefix left with no key, by a held map's timeout, a command or the
//! rest of a map's replay, waits the same delay
//! ([`EditorView::which_key_later`]), unless it's the one the popup last
//! waited on since the last key: a mode change that keeps `Z` pending
//! closes its popup for good.
//!
//! Prefixes the keymap matcher holds (`] d`, `g b`) never reach vim, so
//! they show nothing.

use std::{cell::RefCell, time::Duration};

use fe_command::{CommandId, CommandMeta};
use fe_vim::{WhichKey, WhichKeyEntry, WhichKeyLabel};
use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, deferred, div,
    relative,
};

use super::{Vim, registry};
use crate::EditorView;

/// `vim.which_key` and `vim.which_key_delay_ms`.
#[derive(Clone, Copy, Debug)]
pub(super) struct WhichKeySettings {
    on: bool,
    delay: Duration,
}

impl Default for WhichKeySettings {
    /// The settings' defaults: on, after which-key.nvim's 200 ms.
    fn default() -> Self {
        Self {
            on: true,
            delay: Duration::from_millis(200),
        }
    }
}

/// What a popup shows or waits to show: the prefix's keys, and the vim
/// mode they wait in.
#[derive(Clone, Debug, PartialEq)]
struct Target {
    prefix: String,
    mode: &'static str,
}

/// An editor's which-key popup: the list it shows, or the wait before it
/// shows one. Dropped, the wait ends.
#[derive(Default)]
pub(crate) struct WhichKeyPopup {
    shown: Option<WhichKey>,
    /// What it shows or waits on; `None` while idle.
    target: Option<Target>,
    /// The last target's prefix since the last key, even once closed: what
    /// comes with no key doesn't wait on it again, in any mode.
    seen: Option<String>,
    timer: Option<Task<()>>,
    blur: Option<Subscription>,
}

impl WhichKeyPopup {
    /// Closes it, keeping what it last saw. Returns whether one showed.
    fn close(&mut self) -> bool {
        let seen = self.seen.take();
        let shown = std::mem::take(self).shown.is_some();
        self.seen = seen;
        shown
    }
}

/// Turns the which-key popup on or off in every editor, and sets how long a
/// prefix waits before it shows (`vim.which_key`, `vim.which_key_delay_ms`;
/// zero shows it at once). Turning it off closes every one open; a new
/// delay applies from the next prefix.
pub fn set_vim_which_key(cx: &mut App, on: bool, delay: Duration) {
    let vim = cx.default_global::<Vim>();
    vim.which_key = WhichKeySettings { on, delay };
    if on {
        return;
    }
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    for editor in editors {
        (editor.update(cx, |view, cx| view.close_which_key(cx))).ok();
    }
}

fn settings(cx: &App) -> WhichKeySettings {
    (cx.try_global::<Vim>())
        .map(|vim| vim.which_key)
        .unwrap_or_default()
}

impl EditorView {
    /// The which-key list the popup shows, if one does.
    pub(crate) fn which_key_shown(&self) -> Option<&WhichKey> {
        self.which_key.shown.as_ref()
    }

    /// What waits now, with the mode it waits in.
    fn which_key_now(&self) -> Option<(WhichKey, Target)> {
        let state = self.vim.as_ref()?;
        let which = state.which_key()?;
        let target = Target {
            prefix: which.prefix.clone(),
            mode: state.mode_name(),
        };
        Some((which, target))
    }

    /// After a keystroke reached vim: the popup or its wait ends, and if a
    /// prefix waits now, the popup shows once the delay passes with no
    /// other key, or at once if it showed already or the delay is zero.
    pub(super) fn which_key_after_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was_shown = std::mem::take(&mut self.which_key).shown.is_some();
        let now = (self.which_key_now()).filter(|_| settings(cx).on && self.takes_keys(window));
        match now {
            Some((which, target)) => self.arm_which_key(which, target, was_shown, window, cx),
            None if was_shown => cx.notify(),
            None => {}
        }
    }

    /// Waits on `target`, whose keys are `which`: shows them now if
    /// `at_once` or the delay is zero, otherwise once it passes, if they
    /// still wait then. A blur closes it.
    fn arm_which_key(
        &mut self,
        which: WhichKey,
        target: Target,
        at_once: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Blur listeners run while a frame draws, where a notify asks for
        // no frame: the one that clears the panel comes after it.
        let blur = cx.on_blur(&self.focus, window, |view, window, cx| {
            if view.which_key.close() {
                cx.defer_in(window, |_, _, cx| cx.notify());
            }
        });
        let popup = &mut self.which_key;
        popup.blur = Some(blur);
        popup.seen = Some(target.prefix.clone());
        popup.target = Some(target);
        let delay = settings(cx).delay;
        if at_once || delay.is_zero() {
            popup.shown = Some(which);
            cx.notify();
            return;
        }
        popup.timer = Some(cx.spawn(async move |view, cx| {
            cx.background_executor().timer(delay).await;
            (view.update(cx, |view, cx| view.which_key_due(cx))).ok();
        }));
    }

    /// The delay passed with no key: the popup shows if what it waited on
    /// still waits, in the same mode.
    fn which_key_due(&mut self, cx: &mut Context<Self>) {
        self.which_key.timer = None;
        let now = self.which_key_now();
        match now {
            Some((which, target)) if self.which_key.target.as_ref() == Some(&target) => {
                self.which_key.shown = Some(which);
                cx.notify();
            }
            _ => {
                self.which_key.close();
            }
        }
    }

    /// Closes the popup and ends its wait.
    pub(crate) fn close_which_key(&mut self, cx: &mut Context<Self>) {
        if self.which_key.close() {
            cx.notify();
        }
    }

    /// Closes the popup, or ends its wait, if what it lists no longer
    /// waits as it did: the prefix resolved, its list changed, or the mode
    /// did, with no key. Returns whether it closed one shown.
    pub(crate) fn settle_which_key(&mut self) -> bool {
        let Some(target) = &self.which_key.target else {
            return false;
        };
        let now = self.which_key_now();
        let stale = match (&now, &self.which_key.shown) {
            (None, _) => true,
            (Some((_, now)), _) if now != target => true,
            (Some((which, _)), Some(shown)) => which != shown,
            (Some(_), None) => false,
        };
        stale && self.which_key.close()
    }

    /// Once the update under way is over, in the active window: closes a
    /// stale popup ([`EditorView::settle_which_key`]), and if a prefix
    /// waits that this one hasn't waited on since the last key, waits on
    /// it. For prefixes that come with no key: a held map's timeout, a
    /// command, the rest of a map's replay.
    pub(super) fn which_key_later(&self, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        cx.defer(move |cx| {
            let Some(view) = view.upgrade() else {
                return;
            };
            let Some(active) = cx.active_window() else {
                return;
            };
            let sync = |_: gpui::AnyView, window: &mut Window, cx: &mut App| {
                view.update(cx, |view, cx| view.sync_which_key(window, cx));
            };
            active.update(cx, sync).ok();
        });
    }

    fn sync_which_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settle_which_key() {
            cx.notify();
        }
        if self.which_key.target.is_some() || !settings(cx).on || !self.takes_keys(window) {
            return;
        }
        let Some((which, target)) = self.which_key_now() else {
            return;
        };
        if self.which_key.seen.as_ref() != Some(&target.prefix) {
            self.arm_which_key(which, target, false, window, cx);
        }
    }
}

thread_local! {
    /// The host's commands with their titles, for the popup
    /// ([`set_host_commands`]).
    static HOST_COMMANDS: RefCell<Vec<CommandMeta>> = const { RefCell::new(Vec::new()) };
}

/// Hands fe-gpui the host's own commands, those of its registries (the
/// workbench's panels), so the which-key popup shows a map's `fe:` command
/// of one by its title. Typed commands and the app's need no handing over.
pub fn set_host_commands(commands: Vec<CommandMeta>) {
    HOST_COMMANDS.with_borrow_mut(|host| *host = commands);
}

/// What the popup calls an entry: a built-in key's title, a command's title
/// ([`command_title`]), a map's right side, or `+N` for a group of `N`
/// longer maps.
pub(crate) fn title(entry: &WhichKeyEntry, cx: &App) -> String {
    match &entry.label {
        WhichKeyLabel::Builtin(title) => (*title).to_owned(),
        WhichKeyLabel::Command(id) => command_title(id, cx),
        WhichKeyLabel::Keys(keys) => keys.clone(),
        WhichKeyLabel::Group(count) => format!("+{count}"),
    }
}

/// The title of the command `id`, from whichever registry has it: vim's,
/// the proposals', the diff view's, the editor's, the host's
/// ([`set_host_commands`]), the typed commands or the app's. Its ID if none
/// does.
pub(crate) fn command_title(id: &str, cx: &App) -> String {
    let Ok(command) = CommandId::parse(id) else {
        return id.to_owned();
    };
    let registered = (registry().get(&command).map(|meta| meta.title.clone()))
        .or_else(|| (crate::view::proposal_registry().get(&command)).map(|meta| meta.title.clone()))
        .or_else(|| (crate::diff_view::registry().get(&command)).map(|meta| meta.title.clone()));
    let listed = |list: Vec<CommandMeta>| {
        (list.into_iter())
            .find(|meta| meta.id == command)
            .map(|meta| meta.title)
    };
    registered
        .or_else(|| listed(crate::commands::command_list()))
        .or_else(|| listed(HOST_COMMANDS.with_borrow(Clone::clone)))
        .or_else(|| {
            kiln_platform::command_actions::typed_command(&command, cx).map(|typed| typed.title)
        })
        .or_else(|| listed(kiln_platform::commands::app_command_list(cx)))
        .unwrap_or_else(|| id.to_owned())
}

/// The width of one key and its title, in the popup's columns.
const COLUMN_CHARS: f32 = 30.0;

/// The most of the editor's height the popup takes; its keys scroll
/// within it.
const MAX_HEIGHT: f32 = 0.5;

/// The popup, while one shows: a panel along the bottom of the editor,
/// over its text, with the prefix, then each key and its title in columns,
/// at most half the editor's height, its keys scrolling past that.
pub(crate) fn popup_layer(view: &EditorView, cx: &App) -> Option<impl IntoElement + use<>> {
    let which = view.which_key_shown()?;
    let theme = &view.theme;
    let column = theme.font_size * COLUMN_CHARS;
    let entries = which.entries.iter().map(|entry| {
        let title = title(entry, cx);
        let title = match &entry.label {
            WhichKeyLabel::Group(_) => div().text_color(theme.information).child(title),
            _ if entry.group => div().child(format!("{title} +")),
            _ => div().child(title),
        };
        div()
            .w(column)
            .flex()
            .gap_2()
            .overflow_hidden()
            .child(div().text_color(theme.warning).child(entry.key.clone()))
            .child(div().text_color(theme.gutter_foreground).child("→"))
            .child(title)
    });
    let panel = div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .max_h(relative(MAX_HEIGHT))
        .flex()
        .flex_col()
        .px_2()
        .py_1()
        .bg(theme.popover_background)
        .border_t_1()
        .border_color(theme.popover_border)
        .font_family(theme.font_family.clone())
        .text_size(theme.font_size)
        .text_color(theme.foreground)
        .debug_selector(|| "which-key-popup".into())
        .child(
            div()
                .flex_none()
                .text_color(theme.gutter_foreground)
                .child(which.prefix.clone()),
        )
        .child(
            div()
                .id("which-key-entries")
                .debug_selector(|| "which-key-entries".into())
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_wrap()
                .children(entries),
        );
    Some(deferred(panel))
}
