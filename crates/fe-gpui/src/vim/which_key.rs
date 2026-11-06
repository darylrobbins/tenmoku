//! Which-key in the editor: once a prefix vim or its remaps hold in normal
//! or a visual mode (`z`, `Z`, `<leader>`, a map's first keys; not insert
//! mode's `j` of `jk`) has waited `vim.which_key_delay_ms` for
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
//! A prefix the keymap matcher holds for a chord (`]` of `] d`, `g` of
//! `g b`, `<C-w>`) never reaches vim: while one is held for an editor in
//! normal or a visual mode, the popup waits the same delay, then lists
//! the chord's next keys ([`kiln_platform::keys::pending_continuations`]),
//! each with its command's title, merged with the keys vim takes after the
//! same prefix (`g`'s built-in `gg`, `ge`...), a chord's key winning. The
//! matcher tells of each change to the chord ([`chord_changed`]): a key
//! that goes on with it repaints the popup at once, and one that ends it,
//! resolved or failed, its timeout, `<Esc>` or a blur closes it. The keys
//! a failed chord replays to vim then wait anew there. Chords held outside
//! an editor (`cmd-k` in a panel) show nothing.
//!
//! A prefix a timer holds (a chord, or a map's keys) stops timing out once
//! its popup shows: it waits for the next key, `<Esc>`, a blur or a keymap
//! reload, as which-key.nvim does. Until then the timeouts are as ever.

use std::{cell::RefCell, time::Duration};

use fe_command::{CommandId, CommandMeta};
use fe_keymap::{Key, Keystroke};
use fe_vim::{Mode, Step, VimKey, WhichKey, WhichKeyEntry, WhichKeyLabel};
use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, deferred, div,
    relative,
};
use kiln_platform::keys::{PendingChord, pending_continuations};

use super::{Vim, registry, which_key_layout as layout};
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

/// What a popup shows or waits to show: the prefix's keys, the vim mode
/// they wait in, and whether the keymap matcher holds them for a chord
/// rather than vim.
#[derive(Clone, Debug, PartialEq)]
struct Target {
    prefix: String,
    mode: &'static str,
    chord: bool,
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

    /// Whether it shows or waits on a chord's keys.
    fn on_chord(&self) -> bool {
        self.target.as_ref().is_some_and(|target| target.chord)
    }
}

/// The chord held in `window` may have changed
/// ([`kiln_platform::keys::KeyOwner::chord_changed`]): the focused
/// editor's popup follows it.
pub(crate) fn chord_changed(window: &mut Window, cx: &mut App) {
    if let Some(editor) = super::focused_editor(window, cx) {
        editor.update(cx, |view, cx| view.which_key_after_chord(window, cx));
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

    /// What waits now, with the mode it waits in: in normal or a visual
    /// mode only. Insert, replace and the command line hold their maps'
    /// keys too, but a popup there would stop the map timing out, holding
    /// back the `j` of `inoremap jk <Esc>`, so none shows.
    fn which_key_now(&self) -> Option<(WhichKey, Target)> {
        let state = self.vim.as_ref()?;
        if !matches!(state.mode(), Mode::Normal | Mode::Visual(_)) {
            return None;
        }
        let which = state.which_key()?;
        let target = Target {
            prefix: which.prefix.clone(),
            mode: state.mode_name(),
            chord: false,
        };
        Some((which, target))
    }

    /// What goes on from the chord the keymap matcher holds for this
    /// editor, in normal or a visual mode: the chord's next keys, merged
    /// with vim's after the same key if it's one key.
    fn which_key_chord(&self, window: &Window, cx: &App) -> Option<(WhichKey, Target)> {
        let state = self.vim.as_ref()?;
        if !matches!(state.mode(), Mode::Normal | Mode::Visual(_)) {
            return None;
        }
        let chord = pending_continuations(window, cx)?;
        let which = chord_which_key(&chord, state, cx)?;
        let target = Target {
            prefix: which.prefix.clone(),
            mode: state.mode_name(),
            chord: true,
        };
        Some((which, target))
    }

    /// After the keymap matcher saw a keystroke, timed a chord out or
    /// abandoned one ([`chord_changed`]): a chord held now has the popup
    /// wait on it, or repaint at once if it showed one already; a popup
    /// of a chord no longer held closes. One of vim's is left be.
    fn which_key_after_chord(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = (self.which_key_chord(window, cx))
            .filter(|_| settings(cx).on && self.takes_keys(window));
        let on_chord = self.which_key.on_chord();
        match now {
            Some((_, target)) if self.which_key.target.as_ref() == Some(&target) => {}
            Some((which, target)) => {
                let was_shown = self.which_key.close();
                if was_shown {
                    cx.notify();
                }
                self.arm_which_key(which, target, was_shown && on_chord, window, cx);
            }
            None if on_chord => self.close_which_key(cx),
            None => {}
        }
    }

    /// After a keystroke reached vim: the popup or its wait ends, and if a
    /// prefix waits now, the popup shows once the delay passes with no
    /// other key, or at once if it showed already or the delay is zero.
    pub(super) fn which_key_after_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A chord's popup closes: the keys it held, which the matcher
        // replays to vim once the chord fails, wait anew there.
        let on_chord = self.which_key.on_chord();
        let was_shown = std::mem::take(&mut self.which_key).shown.is_some();
        if on_chord && was_shown {
            cx.notify();
        }
        let was_shown = was_shown && !on_chord;
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
            self.hold_prefix(window, true, cx);
            cx.notify();
            return;
        }
        // A chord's keys don't change while it's held: the matcher tells of
        // each change, which arms the popup anew or closes it.
        let chord = popup.on_chord().then_some(which);
        popup.timer = Some(cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(delay).await;
            (view.update_in(cx, |view, window, cx| view.which_key_due(chord, window, cx))).ok();
        }));
    }

    /// The popup shows for a prefix a timer holds: it stops timing out and
    /// waits for the next key, `<Esc>`, a blur or a keymap reload.
    fn hold_prefix(&mut self, window: &Window, retry: bool, cx: &mut Context<Self>) {
        match self.which_key.target.as_ref() {
            Some(target) if target.chord => {
                kiln_platform::keys::hold_chord(window, cx);
            }
            Some(_) => {
                let id = cx.entity_id();
                let wait = cx
                    .default_global::<super::Vim>()
                    .remap_wait
                    .as_mut()
                    .filter(|wait| wait.editor.entity_id() == id);
                match wait {
                    Some(wait) => wait._timer = Task::ready(()),
                    // A replay's leftover prefix creates its timer after
                    // this update: hold it once it's there, if still shown.
                    None if retry => cx.defer_in(window, |view, window, cx| {
                        if view.which_key.shown.is_some() {
                            view.hold_prefix(window, false, cx);
                        }
                    }),
                    None => {}
                }
            }
            None => {}
        }
    }

    /// The delay passed with no key: the popup shows `chord`'s keys, or
    /// what it waited on of vim's if that still waits, in the same mode.
    fn which_key_due(
        &mut self,
        chord: Option<WhichKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.which_key.timer = None;
        if let Some(which) = chord {
            self.which_key.shown = Some(which);
            self.hold_prefix(window, true, cx);
            cx.notify();
            return;
        }
        let now = self.which_key_now();
        match now {
            Some((which, target)) if self.which_key.target.as_ref() == Some(&target) => {
                self.which_key.shown = Some(which);
                self.hold_prefix(window, true, cx);
                cx.notify();
            }
            _ => {
                self.which_key.close();
            }
        }
    }

    /// Holds `keys` as vim does when they're typed, then shows the
    /// which-key popup for what waits after them at once, as if its delay
    /// had passed: for screenshots of an off-screen window, which is never
    /// the active one, the only one where keys wait for a longer map or for
    /// the popup ([`EditorView::takes_keys`]). What it holds waits, with no
    /// timeout, for the next key. Nothing shows if vim is off or nothing
    /// waits after `keys`.
    #[doc(hidden)]
    pub fn show_which_key_for(
        &mut self,
        keys: &[VimKey],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_vim(cx);
        let Some(state) = self.vim.as_mut() else {
            return;
        };
        for &key in keys {
            if !matches!(state.feed(key), Step::Wait(_)) {
                return;
            }
        }
        if let Some((which, target)) = self.which_key_now() {
            self.which_key.close();
            self.arm_which_key(which, target, true, window, cx);
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
    /// did, with no key. Returns whether it closed one shown. A chord's
    /// is the matcher's to close ([`chord_changed`]).
    pub(crate) fn settle_which_key(&mut self) -> bool {
        let Some(target) = (self.which_key.target.as_ref()).filter(|target| !target.chord) else {
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

/// What goes on from `chord`: its next keys, each with the command the keys
/// up to it run, or as a group of the keys that go on past it, merged with
/// what vim takes after the chord's key if it's one ([`fe_vim::VimState::which_key_for`]),
/// the chord's key winning; `None` if nothing does.
fn chord_which_key(chord: &PendingChord, state: &fe_vim::VimState, cx: &App) -> Option<WhichKey> {
    let vim = match &chord.held[..] {
        [key] => vim_key(key).and_then(|key| state.which_key_for(&[key])),
        _ => None,
    };
    let mut entries = vim.map_or_else(Vec::new, |which| which.entries);
    let keymap = crate::keymap::current(cx);
    for next in &chord.next {
        let key = key_name(&next.next);
        let label = match &next.command {
            Some(id) => WhichKeyLabel::Command(id.to_string()),
            None => {
                let mut held = chord.held.clone();
                held.push(next.next);
                WhichKeyLabel::Group {
                    count: keymap.continuations(&held, &chord.stack).len(),
                    name: None,
                }
            }
        };
        entries.retain(|entry| entry.key != key);
        entries.push(WhichKeyEntry {
            key,
            label,
            group: next.longer,
        });
    }
    if entries.is_empty() {
        return None;
    }
    let prefix = chord.held.iter().map(key_name).collect();
    let mut which = WhichKey { prefix, entries };
    which.sort_entries();
    Some(which)
}

/// The key vim reads for `keystroke`, a keystroke of a chord, if any, as
/// [`super::vim_key`] reads GPUI's: a letter, with `shift` its capital, a
/// character, `<C-letter>`, or a named key.
fn vim_key(keystroke: &Keystroke) -> Option<VimKey> {
    let held = keystroke.modifiers;
    if held.cmd || held.alt || held.function {
        return None;
    }
    if held.ctrl {
        return match keystroke.key {
            Key::Char(letter) if !held.shift && letter.is_ascii_alphabetic() => {
                Some(VimKey::Ctrl(letter))
            }
            _ => None,
        };
    }
    Some(match keystroke.key {
        Key::Char(letter) if letter.is_ascii_alphabetic() => match held.shift {
            true => VimKey::Char(letter.to_ascii_uppercase()),
            false => VimKey::Char(letter),
        },
        _ if held.shift => return None,
        Key::Char(ch) => VimKey::Char(ch),
        Key::Space => VimKey::Char(' '),
        Key::Escape => VimKey::Esc,
        Key::Enter => VimKey::Enter,
        Key::Backspace => VimKey::Backspace,
        Key::Tab => VimKey::Tab,
        Key::Left => VimKey::Left,
        Key::Right => VimKey::Right,
        Key::Up => VimKey::Up,
        Key::Down => VimKey::Down,
        _ => return None,
    })
}

/// `keystroke`, a keystroke of a chord, in vim's notation, as vim's keys
/// list: `b`, `W`, `]`, `<C-h>`, `<Left>`, `<D-k>`.
fn key_name(keystroke: &Keystroke) -> String {
    let held = keystroke.modifiers;
    let plain = !(held.cmd || held.ctrl || held.alt || held.function);
    let name = match keystroke.key {
        Key::Char(letter) if plain && letter.is_ascii_alphabetic() => {
            return match held.shift {
                true => letter.to_ascii_uppercase().to_string(),
                false => letter.to_string(),
            };
        }
        Key::Char('<') => "lt".to_owned(),
        Key::Char('|') => "Bar".to_owned(),
        Key::Char(ch) if plain && !held.shift => return ch.to_string(),
        Key::Char(ch) => ch.to_string(),
        Key::Space => "Space".to_owned(),
        Key::Enter => "CR".to_owned(),
        Key::Escape => "Esc".to_owned(),
        Key::Backspace => "BS".to_owned(),
        Key::Tab => "Tab".to_owned(),
        Key::Delete => "Del".to_owned(),
        Key::Left => "Left".to_owned(),
        Key::Right => "Right".to_owned(),
        Key::Up => "Up".to_owned(),
        Key::Down => "Down".to_owned(),
        Key::Home => "Home".to_owned(),
        Key::End => "End".to_owned(),
        Key::PageUp => "PageUp".to_owned(),
        Key::PageDown => "PageDown".to_owned(),
        Key::F(n) => format!("F{n}"),
        Key::Click => "LeftMouse".to_owned(),
    };
    let modifiers = [
        (held.ctrl, "C-"),
        (held.alt, "M-"),
        (held.cmd, "D-"),
        (held.shift, "S-"),
    ];
    let modifiers: String = (modifiers.iter())
        .filter(|(on, _)| *on)
        .map(|(_, name)| *name)
        .collect();
    format!("<{modifiers}{name}>")
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
/// ([`command_title`]), a map's right side, or for a group of `N` longer
/// maps `+{name}` with a name for it, `+N` without.
pub(crate) fn title(entry: &WhichKeyEntry, cx: &App) -> String {
    match &entry.label {
        WhichKeyLabel::Builtin(title) => (*title).to_owned(),
        WhichKeyLabel::Command(id) => command_title(id, cx),
        WhichKeyLabel::Keys(keys) => keys.clone(),
        WhichKeyLabel::Group {
            name: Some(name), ..
        } => format!("+{name}"),
        WhichKeyLabel::Group { count, name: None } => format!("+{count}"),
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
/// items before groups, and a hint to close it along the bottom; at most
/// half the editor's height, its keys scrolling past that. Each key shows
/// as its glyph ([`layout::key_glyph`]) in a column as wide as the widest,
/// so every arrow starts at one x.
pub(crate) fn popup_layer(view: &EditorView, cx: &App) -> Option<impl IntoElement + use<>> {
    let which = view.which_key_shown()?;
    let theme = &view.theme;
    let column = theme.font_size * COLUMN_CHARS;
    let text = cx.text_system();
    let font = text.resolve_font(&theme.font());
    let ch = text.ch_advance(font, theme.font_size);
    let ch = ch.unwrap_or(theme.font_size * 0.6);
    let glyphs: Vec<String> = (which.entries.iter())
        .map(|entry| layout::key_glyph(&entry.key))
        .collect();
    let measure = |glyph: &str| {
        (glyph.chars())
            .map(|c| f32::from(text.layout_width(font, theme.font_size, c)))
            .sum()
    };
    let keys = ch * layout::key_column(glyphs.iter().map(String::as_str), measure, ch.into());
    let entries = which.entries.iter().zip(glyphs).map(|(entry, glyph)| {
        let colors = layout::entry_colors(entry, theme);
        let title = title(entry, cx);
        let title = match &entry.label {
            WhichKeyLabel::Group { .. } => title,
            _ if entry.group => format!("{title} +"),
            _ => title,
        };
        let key = entry.key.clone();
        let arrow = entry.key.clone();
        div()
            .debug_selector(move || format!("which-key-entry-{key}"))
            .w(column)
            .flex()
            .gap_2()
            .overflow_hidden()
            .child(
                // Shaped, a key can come out a hair wider than measured:
                // it never wraps.
                div()
                    .flex_none()
                    .w(keys)
                    .whitespace_nowrap()
                    .text_color(colors.key)
                    .child(glyph),
            )
            .child(
                div()
                    .debug_selector(move || format!("which-key-arrow-{arrow}"))
                    .flex_none()
                    .text_color(colors.arrow)
                    .child("→"),
            )
            .child(div().text_color(colors.title).child(title))
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
        // The wheel scrolls its keys, never the text under it: GPUI's
        // overflow scrolling lets the event go on to the editor's handler.
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex_none()
                .text_color(theme.gutter_foreground)
                .child(layout::keys_glyphs(&which.prefix)),
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
        )
        .child(
            div()
                .debug_selector(|| "which-key-footer".into())
                .flex_none()
                .text_color(theme.gutter_foreground)
                .child(layout::FOOTER),
        );
    Some(deferred(panel))
}
