//! Vim mode in the editor: one app-wide switch, each editor's
//! [`VimState`], and the keys `fe-vim`'s reducer reads. The design is
//! `docs/decisions/vim-engine.md`, §9 and §10.
//!
//! With vim on, a focused editor's key context is
//! `editor vim_enabled vim_mode=<mode>`, plus `vim_pending` and
//! `vim_pending_kind` while a command is incomplete; [`crate::keymap`] reads
//! them as `vim.enabled`, `vim.mode`, `vim.pending` and `vim.pending_kind`. A keystroke in normal,
//! operator-pending or a visual mode goes:
//!
//! 1. to the keymap, as always, unless a command is pending. A match runs,
//!    except `vim.key`, which only says the keystroke is vim's here;
//! 2. otherwise to the editor's key listener, [`EditorView::on_vim_key`],
//!    which feeds it to the reducer: `Wait` and `Cancel` swallow it,
//!    `Commit` runs the invocation through `fe-vim`'s registry, and
//!    `Forward` lets it go on, to type nothing: no input handler is
//!    registered outside insert mode.
//!
//! In normal mode with nothing pending, `escape` also keeps only the primary
//! caret, as with vim off: the keymap adds carets there (`gb`), and vim's
//! commands act at each.
//!
//! In insert mode the reducer isn't called: an unmatched keystroke goes on
//! as text, and the editor's keys work as with vim off, but for `escape`
//! and `ctrl-[`, bound to `vim.normal_mode`. Text typed or composed by an
//! input method joins the insert session, and `Enter` goes in through
//! [`VimState::line_break`]. Any other editor command, and a mouse click
//! that moves the caret, splits the session
//! ([`VimState::split_session`]), as Vim's cursor keys do.
//!
//! Replace mode (`R`) takes keys as insert mode does, with the input
//! handler registered, but typed text goes through
//! [`VimState::replace_text`], overwriting, `editor.backspace` through
//! [`VimState::replace_backspace`], and `escape` and `ctrl-[`
//! (`vim.normal_mode`) run `vim.replace_normal_mode`. The caret is an
//! underline there and in operator-pending mode ([`CaretShape`]).
//!
//! While `q` records a macro, every key that reaches the editor goes into
//! it: the reducer records the keys it reads, and the editor the ones it
//! takes itself ([`VimState::record_key`]): in insert and replace mode the
//! text typed or committed by an input method (never what it's composing),
//! `<CR>`, `<BS>`, `<Esc>` and the arrows, and the command line's text and
//! the key that ended it, which the host's line reports
//! ([`EditorView::record_command_line`]). An editor command with no vim key,
//! such as `alt-backspace`, is left out, logged once a recording. `@` hands
//! the keys back ([`VimState::take_replay`]), which the editor feeds
//! through the same paths, the command line's into a line it doesn't show,
//! with no keystroke dispatched ([`EditorView::replay_vim`]).
//!
//! A remap (`vim.remap`) runs the same way: the reducer leaves its keys for
//! the editor to feed ([`VimState::take_replay`]), in the keystroke that
//! completed it. While keys are held for a longer map
//! ([`VimState::remap_waiting`]) the editor owns the next keystroke, as with
//! a pending command, and after [`CHORD_TIMEOUT`] with none, or a click or
//! a focus change, they resolve ([`VimState::remap_timeout`]). An ex line a
//! map runs leaves its file or window command for the host
//! ([`EditorView::take_vim_host_command`]), and one it leaves open opens on
//! the host's line with the map's text ([`VimState::command_line_prefill`]).
//!
//! Two known limits, each a card of its own:
//!
//! - **A map ends at its first host command.** The keys before a file or
//!   window command (`:w<CR>`, or `do` and `dp`) run, the command is left
//!   for the host as a typed line's is, and the keys after it aren't
//!   replayed: the status bar says "a map's keys after a file or window
//!   command aren't run". The host runs the command once the keystroke is
//!   over, asynchronously for a write, and nothing reports its outcome back
//!   to resume the map. So `:w<CR>:q<CR>` only writes (`:wq` and `:x` do
//!   both).
//! - **Only a plain click resolves a held prefix.** A click goes through
//!   [`EditorView::cancel_vim`], which resolves the held keys as a timeout
//!   would and then drops what they leave pending. Shift, double and triple
//!   clicks select without it, so the prefix stays held until its timeout,
//!   and a click's destination is measured before the held keys resolve,
//!   which can edit the text: both are the view's mouse handling.
//!
//! In the visual modes (`v`, `V`, `ctrl-v` or `ctrl-q`) the model's
//! selection is the visual extent, which `fe-vim` keeps it as; each frame
//! also makes it so again before its layout ([`EditorView::sync_visual`]),
//! after another view's edits or an editor command that moved it, so
//! `cmd-c` copies what `y` would yank and stays in visual mode. The block
//! cursor paints on the vim cursor, not the selection's head. With vim on
//! outside insert mode, a mouse drag enters charwise visual mode (a double
//! click's drag by words, a triple click linewise), and a click, a bound
//! click or a jump drops a pending command and leaves a visual mode
//! ([`EditorView::cancel_vim`]).
//!
//! The keymap matches against each editor's live state, stamped onto its
//! frame before matching ([`live_stack`]), not the context the last frame
//! drew, which can trail the keys typed since.

use std::{collections::VecDeque, rc::Rc, sync::Arc};

use fe_buffer::TransactionId;
use fe_command::{ContextStack, DispatchError, Dispatched, Effect, Invocation, Registry, Source};
use fe_config::{VimRemapMode, VimSettings};
use fe_editor::{EditorModel, commands::EditorHost};
use fe_vim::{
    Registers, Remap, RemapMode, RemapSet, Step, VimCommands, VimKey, VimState, VimTarget,
};
use gpui::{
    App, Context, Entity, FocusHandle, Global, KeyContext, KeyDownEvent, Subscription, Task,
    WeakEntity, WeakFocusHandle, Window,
};

use crate::{
    EditorView,
    commands::{ViewHost, loaded_language},
    keymap::CHORD_TIMEOUT,
    view::synced_syntax,
};

/// The command a vim-layer binding names to hand its keystroke to the
/// reducer. It's never dispatched from a key.
pub(crate) const KEY: &str = "vim.key";
/// Leaves a visual mode, as a click in one does.
pub(crate) const VISUAL_EXIT: &str = "vim.visual_exit";
/// Flips the switch, for this session.
pub(crate) const TOGGLE: &str = "vim.toggle";
/// The flag an editor's frame carries while vim is on, in every mode: the
/// editor's keys that would take vim's, such as `escape`, apply only
/// without it (`editor && !vim.enabled`).
pub(crate) const ENABLED: &str = "vim.enabled";

/// The line break `Enter` types: in insert mode, through
/// [`VimState::line_break`].
pub(crate) const NEWLINE: &str = "editor.newline";
/// Typed text: in replace mode, through [`VimState::replace_text`].
pub(crate) const INSERT: &str = "editor.insert";
/// `<BS>`: in replace mode, through [`VimState::replace_backspace`].
pub(crate) const BACKSPACE: &str = "editor.backspace";
/// Insert mode's `Esc` and `ctrl-[`, and replace mode's as the keymap binds
/// them: there the editor runs [`REPLACE_NORMAL_MODE`] for it.
pub(crate) const NORMAL_MODE: &str = "vim.normal_mode";
/// Replace mode's `Esc` in `fe-vim`.
pub(crate) const REPLACE_NORMAL_MODE: &str = "vim.replace_normal_mode";

/// The editor commands a recording holds in vim notation, as the keys the
/// keymap binds them to: in insert and replace mode, the arrows and `Tab`.
const MOVES: [(&str, VimKey); 5] = [
    ("editor.move_left", VimKey::Left),
    ("editor.move_right", VimKey::Right),
    ("editor.move_up", VimKey::Up),
    ("editor.move_down", VimKey::Down),
    ("editor.tab", VimKey::Tab),
];

/// How the carets paint (§9 item 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaretShape {
    /// Between graphemes: with vim off, and in insert mode.
    Bar,
    /// Over the grapheme: in normal and the visual modes.
    Block,
    /// Under the grapheme, [`UNDERLINE_HEIGHT`] tall: in replace and
    /// operator-pending mode.
    Underline,
}

/// How tall the underline caret is.
pub(crate) const UNDERLINE_HEIGHT: f32 = 2.;
/// The editor commands that type, besides [`NEWLINE`]: in insert mode they
/// join the session, as the input methods' edits do, where any other editor
/// command splits it ([`VimState::split_session`]).
pub(crate) const TEXT_INPUT: [&str; 5] = [
    "editor.insert",
    "editor.backspace",
    "editor.delete",
    "editor.delete_word_left",
    "editor.tab",
];

/// The prefixes of the editor commands that move carets or change
/// selections (`editor.move`, `editor.move_left`, `editor.select_all`,
/// `editor.page_up`, ...): in insert mode they split the session only if
/// they did, where any other non-text command always splits it.
pub(crate) const MOTION_PREFIXES: [&str; 3] = ["editor.move", "editor.select", "editor.page"];

/// The prefix of vim's keys in GPUI's key contexts, whose identifiers can't
/// hold `.`: `vim_mode` is `vim.mode`.
pub(crate) const GPUI_PREFIX: &str = "vim_";

/// The switch, and every editor, by its focus handle, so a keystroke can
/// find the focused one's live state.
#[derive(Default)]
struct Vim {
    enabled: bool,
    /// Bumped each time the switch flips, so an editor that hasn't seen the
    /// flip yet drops its state, even if the switch flipped back since.
    generation: u64,
    editors: Vec<(WeakFocusHandle, WeakEntity<EditorView>)>,
    /// Shared by every editor, as Vim's are by every window.
    registers: Registers,
    /// `vim.jump_on_s`: whether `s` and `S` start the jump.
    jump_on_s: bool,
    /// `vim.remap` and `vim.leader`, built.
    remaps: Arc<RemapSet>,
    /// The editor holding keys for a longer map: one at a time, as only the
    /// focused editor takes keys.
    remap_wait: Option<RemapWait>,
}

/// What ends an editor's wait for the rest of a map: [`CHORD_TIMEOUT`]
/// with no key, as nvim's 'timeoutlen', or its focus going. Dropped, it
/// ends neither.
struct RemapWait {
    editor: WeakEntity<EditorView>,
    _timer: Task<()>,
    _blur: Subscription,
}

impl Global for Vim {}

thread_local! {
    /// Built once, as `EDITOR_COMMANDS` is.
    static VIM_COMMANDS: Rc<Registry<VimCommands>> = Rc::new(fe_vim::registry());
}

/// `fe-vim`'s registry.
pub(crate) fn registry() -> Rc<Registry<VimCommands>> {
    VIM_COMMANDS.with(Rc::clone)
}

/// Turns vim on or off in every editor, and redraws every window. Turning
/// it off drops each editor's pending command. Turning it on moves no
/// caret: the first vim motion puts carets where normal mode keeps them.
pub fn set_vim_enabled(cx: &mut App, enabled: bool) {
    let vim = cx.default_global::<Vim>();
    if vim.enabled != enabled {
        vim.enabled = enabled;
        vim.generation += 1;
    }
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    // Once no view is being updated: so the status bar, which observes its
    // editor, sees the mode.
    cx.defer(move |cx| {
        for editor in editors {
            editor
                .update(cx, |view, cx| {
                    if view.sync_vim(cx) {
                        cx.notify();
                    }
                })
                .ok();
        }
    });
    cx.refresh_windows();
}

/// Sets `clipboard=unnamedplus` for every editor's registers: yanks,
/// deletes and puts that name no register then use the system clipboard.
/// `"+` and `"*` reach it either way.
pub fn set_vim_clipboard(cx: &mut App, unnamedplus: bool) {
    cx.default_global::<Vim>()
        .registers
        .set_clipboard_unnamed(unnamedplus);
}

/// Lets `s` and `S` start the jump in every editor, and in any opened or
/// switched on later (`vim.jump_on_s`).
pub fn set_vim_jump_on_s(cx: &mut App, on: bool) {
    let vim = cx.default_global::<Vim>();
    vim.jump_on_s = on;
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    for editor in editors {
        editor
            .update(cx, |view, _| {
                if let Some(state) = &mut view.vim {
                    state.set_jump_on_s(on);
                }
            })
            .ok();
    }
}

/// Gives every editor the remaps of `vim.remap` and `vim.leader`, and any
/// opened or switched on later, replacing the set before: keys an editor
/// holds for a longer map go on at once, unmapped. An entry that
/// can't load is left out alone and comes back as `vim.remap[N]: …`.
pub fn set_vim_remaps(cx: &mut App, settings: &VimSettings) -> Vec<String> {
    let remaps: Vec<Remap> = settings
        .remap
        .iter()
        .map(|remap| Remap {
            modes: remap_modes(remap.mode.modes()),
            from: remap.from.clone(),
            to: remap.to.clone(),
            recursive: remap.recursive,
        })
        .collect();
    let (set, issues) = RemapSet::new(&remaps, &settings.leader);
    let set = Arc::new(set);
    let vim = cx.default_global::<Vim>();
    vim.remaps = Arc::clone(&set);
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    for editor in editors {
        editor
            .update(cx, |view, cx| {
                if let Some(state) = &mut view.vim {
                    state.set_remaps(Arc::clone(&set));
                }
                // Keys held for a longer map go on now, unmapped.
                view.replay_vim(cx);
            })
            .ok();
    }
    issues
        .into_iter()
        .map(|issue| format!("vim.remap[{}]: {}", issue.index, issue.message))
        .collect()
}

/// `v` means `x`; a mode listed twice counts once.
fn remap_modes(modes: &[VimRemapMode]) -> Vec<RemapMode> {
    let mut out = Vec::new();
    for mode in modes {
        let mode = match mode {
            VimRemapMode::Normal => RemapMode::Normal,
            VimRemapMode::Visual | VimRemapMode::VisualSelect => RemapMode::Visual,
            VimRemapMode::OperatorPending => RemapMode::OperatorPending,
        };
        if !out.contains(&mode) {
            out.push(mode);
        }
    }
    out
}

/// Applies a settings reload's `vim.enabled` and `vim.clipboard`, each as
/// `(now, before)`: only a value the file changed, so that reloading
/// anything else keeps a Toggle Vim Mode.
pub fn apply_vim_reload(cx: &mut App, enabled: (bool, bool), unnamedplus: (bool, bool)) {
    if enabled.0 != enabled.1 {
        set_vim_enabled(cx, enabled.0);
    }
    if unnamedplus.0 != unnamedplus.1 {
        set_vim_clipboard(cx, unnamedplus.0);
    }
}

/// Whether vim is on.
pub fn vim_enabled(cx: &App) -> bool {
    cx.try_global::<Vim>().is_some_and(|vim| vim.enabled)
}

/// The registers every editor shares, with the command-line histories a
/// host's line recalls from ([`Registers::recall`]); `None` before any
/// editor has used vim.
pub fn vim_registers(cx: &App) -> Option<&Registers> {
    cx.try_global::<Vim>().map(|vim| &vim.registers)
}

/// The switch's state: whether it's on, and its generation.
fn switch(cx: &App) -> (bool, u64) {
    cx.try_global::<Vim>()
        .map_or((false, 0), |vim| (vim.enabled, vim.generation))
}

/// Lists `view`'s editor, focused by `focus`, until it's released.
pub(crate) fn register(focus: &FocusHandle, cx: &mut Context<EditorView>) {
    let view = cx.weak_entity();
    let id = view.entity_id();
    cx.default_global::<Vim>()
        .editors
        .push((focus.downgrade(), view));
    cx.on_release(move |_, cx| {
        cx.default_global::<Vim>()
            .editors
            .retain(|(_, view)| view.entity_id() != id);
    })
    .detach();
}

/// The focused editor, if the focused element is one.
pub(crate) fn focused_editor(window: &Window, cx: &App) -> Option<Entity<EditorView>> {
    let focused = window.focused(cx)?;
    cx.try_global::<Vim>()?
        .editors
        .iter()
        .find(|(focus, _)| *focus == focused)?
        .1
        .upgrade()
}

/// The focused editor, if vim owns its keys: vim is on and it isn't in
/// insert mode.
pub(crate) fn vim_editor(window: &Window, cx: &mut App) -> Option<Entity<EditorView>> {
    let editor = focused_editor(window, cx)?;
    editor
        .update(cx, |view, cx| {
            view.sync_vim(cx);
            view.vim.is_some() && !view.accepts_text()
        })
        .then_some(editor)
}

/// Feeds `keystrokes` to `editor`'s reducer, each as its key listener
/// would, with no event dispatched: so GPUI's propagation flag, shared by
/// every event, is left as it was.
pub(crate) fn feed(
    editor: &Entity<EditorView>,
    keystrokes: &[gpui::Keystroke],
    window: &mut Window,
    cx: &mut App,
) {
    for keystroke in keystrokes {
        editor.update(cx, |view, cx| view.feed_vim(keystroke, window, cx));
    }
}

/// The context a keystroke is matched in: `stack`, the focused element's
/// as last drawn, with the focused editor's frame stamped from its live
/// vim state, or with no vim keys if vim is off. Also whether a vim
/// command is pending there, or keys are held for a longer map, in which
/// case the editor owns the keystroke (§9.1).
pub(crate) fn live_stack(
    stack: ContextStack,
    window: &Window,
    cx: &mut App,
) -> (ContextStack, bool) {
    let Some(editor) = focused_editor(window, cx) else {
        return (stack, false);
    };
    editor.update(cx, |view, cx| {
        view.sync_vim(cx);
        // A key ends vim's message, whether it's bound or fed to the reducer.
        if let Some(state) = view.vim.as_mut().filter(|state| state.message().is_some()) {
            state.clear_message();
            cx.notify();
        }
        // So `cmd-c` copies the visual extent even if no frame was drawn
        // since another view's edit.
        view.sync_visual();
        let pending = view
            .vim
            .as_ref()
            .is_some_and(|state| state.pending().kind().is_some() || state.remap_waiting());
        // And the completion menu as it is now, not as the last frame
        // drew it: a key after the menu closed is the editor's again.
        view.drop_stale_completion();
        // And the suggestion, so `tab` after it went is the editor's.
        view.drop_stale_ghost_text(cx);
        let stack = stamp(stack, view.vim.as_ref());
        let stack = crate::completion_menu::stamp(stack, view.completion_shown());
        (
            crate::view::ghost_text::stamp(stack, view.ghost_text_shown()),
            pending,
        )
    })
}

/// `stack` with its innermost editor frame's vim keys replaced by
/// `state`'s, or removed for `None`.
pub(crate) fn stamp(stack: ContextStack, state: Option<&VimState>) -> ContextStack {
    let frames = stack.frames();
    let Some(editor) = frames
        .iter()
        .rposition(|frame| frame.name == fe_editor::commands::CONTEXT)
    else {
        return stack;
    };
    frames
        .iter()
        .enumerate()
        .map(|(ix, frame)| {
            let mut frame = frame.clone();
            if ix == editor {
                frame.flags.retain(|flag| !flag.starts_with("vim."));
                frame.values.retain(|key, _| !key.starts_with("vim."));
                if let Some(state) = state {
                    frame = state.stamp(frame).flag(ENABLED);
                }
            }
            frame
        })
        .collect()
}

/// `state`'s keys on the editor's GPUI key context, in GPUI's spelling.
pub(crate) fn add_to_key_context(context: &mut KeyContext, state: &VimState) {
    let frame = state.stamp(fe_command::ContextFrame::new("")).flag(ENABLED);
    let gpui_key = |key: &str| key.replacen("vim.", GPUI_PREFIX, 1);
    for flag in &frame.flags {
        context.add(gpui_key(flag));
    }
    for (key, value) in &frame.values {
        context.set(gpui_key(key), value.clone());
    }
}

/// Runs `invocation`, an `editor.newline`, in insert mode: the line break
/// goes in through [`VimState::line_break_in`], indenting the new row as
/// `host` suggests for the view's language, acting as the invocation's
/// actor, so the session knows the indentation it makes is 'autoindent',
/// which `Esc` drops if nothing is typed after it.
pub(crate) fn line_break(
    state: &VimState,
    model: &mut EditorModel,
    host: &dyn EditorHost,
    invocation: &Invocation,
) -> Result<Dispatched<TransactionId>, DispatchError> {
    let origin = fe_editor::commands::origin(&invocation.actor);
    let ((), transactions) = model.run_as(origin, |model| state.line_break_in(model, host));
    Ok(Dispatched::new(Effect::Changed).with_transactions(transactions))
}

/// The `vim.move` motion the keymap binds `key`'s arrow to in normal and
/// the visual modes.
fn arrow_motion(key: VimKey) -> Option<&'static str> {
    Some(match key {
        VimKey::Left => "left",
        VimKey::Right => "right",
        VimKey::Up => "up",
        VimKey::Down => "down",
        _ => return None,
    })
}

/// `text` with each `\r\n`, and each `\r` alone, made a `\n`: one line
/// break, as typing and a recording take it.
fn line_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Whether text committed at once in insert mode is typed as its keys
/// ([`type_as_keys`]): it holds a line break or a tab, which a recording
/// replays as `<CR>` and `<Tab>`.
pub(crate) fn types_keys(text: Option<&str>) -> bool {
    text.is_some_and(|text| text.contains(['\n', '\r', '\t']))
}

/// Types `text`, committed at once in insert or replace mode, as its keys
/// go in one at a time and as a macro's replay of them does, acting as
/// `invocation`'s actor, as [`line_break`] does: a line break as `Enter`
/// ([`VimState::line_break_in`], which indents as `host` suggests), a tab
/// as `Tab` (`editor.tab`), and the rest as typed, in replace mode a
/// character at a time ([`VimState::replace_text`]) when `overwrite`, so
/// even where a joiner joins the text after it, it overwrites what its
/// replay does.
pub(crate) fn type_as_keys(
    state: &VimState,
    model: &mut EditorModel,
    host: &dyn EditorHost,
    invocation: &Invocation,
    text: &str,
    overwrite: bool,
) -> Result<Dispatched<TransactionId>, DispatchError> {
    let origin = fe_editor::commands::origin(&invocation.actor);
    let ((), transactions) = model.run_as(origin, |model| {
        for piece in line_breaks(text).split_inclusive(['\n', '\t']) {
            let (run, key) = match piece.strip_suffix(['\n', '\t']) {
                Some(run) => (run, piece.chars().last()),
                None => (piece, None),
            };
            match overwrite {
                true => {
                    for ch in run.chars() {
                        state.replace_text(model, ch.encode_utf8(&mut [0; 4]));
                    }
                }
                false if !run.is_empty() => model.insert(run),
                false => {}
            }
            match key {
                Some('\n') => state.line_break_in(model, host),
                Some(_) if state.is_virtual_replace() => state.replace_text(model, "\t"),
                Some(_) => model.tab(),
                None => {}
            }
        }
    });
    Ok(Dispatched::new(Effect::Changed).with_transactions(transactions))
}

/// `<BS>` in replace mode ([`VimState::replace_backspace`]), acting as
/// `invocation`'s actor.
pub(crate) fn replace_backspace(
    state: &VimState,
    model: &mut EditorModel,
    invocation: &Invocation,
) -> Result<Dispatched<TransactionId>, DispatchError> {
    let origin = fe_editor::commands::origin(&invocation.actor);
    let ((), transactions) = model.run_as(origin, |model| state.replace_backspace(model));
    Ok(Dispatched::new(Effect::Changed).with_transactions(transactions))
}

/// The key the reducer reads for `keystroke`, if any: none with `cmd`;
/// `ctrl-[` is `<Esc>` and `ctrl-<letter>` is `<C-letter>`; `escape`,
/// `enter`, `backspace`, `tab` and the arrows are those keys; otherwise the
/// character typed, case kept, so `shift-4` is `$`.
pub(crate) fn vim_key(keystroke: &gpui::Keystroke) -> Option<VimKey> {
    let held = &keystroke.modifiers;
    let key = keystroke.key.as_str();
    if held.platform {
        return None;
    }
    if held.control {
        return match key.as_bytes() {
            b"[" => Some(VimKey::Esc),
            &[letter] if letter.is_ascii_alphabetic() => {
                Some(VimKey::Ctrl(letter.to_ascii_lowercase() as char))
            }
            _ => named(key),
        };
    }
    named(key).or_else(|| {
        let mut typed = keystroke.key_char.as_deref()?.chars();
        let ch = typed.next()?;
        typed.next().is_none().then_some(VimKey::Char(ch))
    })
}

/// Whether `key` is normal mode's `<Esc>` with nothing pending, which goes
/// back to one caret: the reducer only cancels it.
fn single_caret_key(state: &VimState, key: VimKey) -> bool {
    key == VimKey::Esc
        && matches!(state.mode(), fe_vim::Mode::Normal)
        && state.pending().kind().is_none()
}

fn named(key: &str) -> Option<VimKey> {
    Some(match key {
        "escape" => VimKey::Esc,
        "enter" => VimKey::Enter,
        "backspace" => VimKey::Backspace,
        "tab" => VimKey::Tab,
        "left" => VimKey::Left,
        "right" => VimKey::Right,
        "up" => VimKey::Up,
        "down" => VimKey::Down,
        _ => return None,
    })
}

impl EditorView {
    /// Creates the vim state when vim is on and drops it when it's off.
    /// Returns whether that changed anything.
    pub(crate) fn sync_vim(&mut self, cx: &App) -> bool {
        let (enabled, generation) = switch(cx);
        if self.vim_generation == generation {
            return false;
        }
        self.vim_generation = generation;
        let was_on = self.vim.is_some();
        self.vim = enabled.then(|| {
            let mut state = VimState::new();
            if let Some(vim) = cx.try_global::<Vim>() {
                state.set_jump_on_s(vim.jump_on_s);
                state.set_remaps(Arc::clone(&vim.remaps));
            }
            state
        });
        was_on || enabled
    }

    /// The vim mode, `vim.mode`'s value (`normal`, `operator_pending`, …),
    /// or `None` with vim off.
    pub fn vim_mode(&self) -> Option<&'static str> {
        self.vim.as_ref().map(VimState::mode_name)
    }

    /// The vim state, or `None` with vim off: for a host's command line
    /// and the messages it shows.
    pub fn vim_state(&self) -> Option<&VimState> {
        self.vim.as_ref()
    }

    /// Whether typing and input methods insert text: with vim off, or in
    /// insert or replace mode.
    pub(crate) fn accepts_text(&self) -> bool {
        self.vim.is_none() || self.typing_mode().is_some()
    }

    /// With vim in insert mode `Some(false)`, in replace mode `Some(true)`,
    /// and otherwise `None`: the modes where keys type, not vim's.
    pub(crate) fn typing_mode(&self) -> Option<bool> {
        match self.vim.as_ref()?.mode() {
            fe_vim::Mode::Insert => Some(false),
            fe_vim::Mode::Replace => Some(true),
            _ => None,
        }
    }

    /// In a visual mode, makes the model's selection the visual extent
    /// again ([`VimState::sync_visual`]): after another view's edits, or an
    /// editor command that moved it. Each frame does it before its layout,
    /// and each keystroke before it's matched ([`live_stack`]), so what's
    /// painted and what `cmd-c` copies is what an operator takes. An editor
    /// key the vim layer doesn't bind, such as `home` or `cmd-a`, so does
    /// nothing in a visual mode, and `cmd-x` and `cmd-v` edit and stay in
    /// it, as `cmd-c` does.
    pub(crate) fn sync_visual(&mut self) {
        if let Some(state) = &mut self.vim {
            state.sync_visual(&mut self.model);
        }
    }

    /// A frame was laid out: tells vim the view's height, so a counted
    /// 'scroll' set for another one is dropped even if no scroll key ran
    /// at the height between.
    pub(crate) fn viewport_height_changed(&mut self) {
        let Some(rows) = self.viewport().map(|view| view.rows) else {
            return;
        };
        if let Some(state) = &mut self.vim {
            state.viewport_height_changed(rows);
        }
    }

    /// Drops a pending command (as `escape` does), then leaves a visual
    /// mode through `vim.visual_exit`, so `gv` brings its extent back:
    /// before a click or a jump puts the caret somewhere else, so no count,
    /// operator or `r` waiting for its key acts there. Keys held for a
    /// longer map resolve first, as at a timeout. Outside a visual mode with
    /// nothing pending or held it does nothing.
    pub(crate) fn cancel_vim(&mut self, cx: &mut Context<Self>) {
        self.sync_vim(cx);
        if self.vim.as_ref().is_some_and(VimState::remap_waiting) {
            self.resolve_remap(cx);
        }
        let Some(state) = &mut self.vim else {
            return;
        };
        if matches!(state.mode(), fe_vim::Mode::Insert | fe_vim::Mode::Replace) {
            return;
        }
        // One `escape` drops the whole command; in a visual mode it may
        // also leave it. Keys still held for a map go on before it.
        if state.pending().kind().is_some() || state.remap_waiting() {
            if let Step::Commit(invocation) = state.feed(VimKey::Esc) {
                let context = stamp(fe_editor::commands::context(), Some(state));
                self.run_vim(&invocation.context(context), cx);
            }
            self.replay_vim(cx);
        }
        let Some(state) = &self.vim else {
            return;
        };
        if !matches!(state.mode(), fe_vim::Mode::Visual(_)) {
            return;
        }
        let exit = Invocation::of(VISUAL_EXIT).expect("a valid command ID");
        let context = stamp(fe_editor::commands::context(), Some(state));
        self.run_vim(&exit.context(context), cx);
    }

    /// Whether carets paint as blocks: in every vim mode but insert,
    /// replace and operator-pending ([`EditorView::caret_shape`]).
    #[cfg(test)]
    pub(crate) fn block_cursor(&self) -> bool {
        self.caret_shape() == CaretShape::Block
    }

    /// How the carets paint: a bar with vim off and in insert mode, an
    /// underline in replace and operator-pending mode, otherwise a block.
    pub(crate) fn caret_shape(&self) -> CaretShape {
        match self.vim_mode() {
            None | Some("insert") => CaretShape::Bar,
            Some("replace" | "operator_pending") => CaretShape::Underline,
            Some(_) => CaretShape::Block,
        }
    }

    /// Records `key`, one the editor took itself in insert or replace mode
    /// or on the command line, if a macro is recording
    /// ([`VimState::record_key`]); and so counts it, as a replay's keys
    /// are counted. Not while a replay feeds its keys through the typing
    /// paths, which records each of them once itself.
    fn record_vim_key(&mut self, key: VimKey) {
        if self.vim_replaying {
            return;
        }
        if let Some(state) = &mut self.vim {
            state.record_key(key);
        }
    }

    /// Records the keys of `invocation`, one of vim's run from the keymap
    /// or elsewhere, that `fe-vim`'s reducer didn't read: insert and
    /// replace mode's `Esc` (`vim.normal_mode`), and the arrows' moves in
    /// normal and the visual modes (`vim.move`), as `<Left>` and so on.
    pub(crate) fn record_vim_command(&mut self, invocation: &Invocation) {
        let args = &invocation.args;
        let counted = args.get("count").is_some_and(|count| !count.is_null());
        let key = match invocation.command.as_str() {
            NORMAL_MODE if self.typing_mode().is_some() => Some(VimKey::Esc),
            "vim.move" if self.typing_mode().is_none() && !counted => {
                let motion = args.get("motion").and_then(|motion| motion.as_str());
                [VimKey::Left, VimKey::Right, VimKey::Up, VimKey::Down]
                    .into_iter()
                    .find(|&key| motion.is_some() && arrow_motion(key) == motion)
            }
            _ => None,
        };
        if let Some(key) = key {
            self.record_vim_key(key);
        }
    }

    /// Records `invocation`, an editor command run in insert or replace
    /// mode, as the keys typed: its text (a line break as `<CR>`),
    /// `editor.newline` as `<CR>`, `editor.backspace` as `<BS>` and the
    /// arrows' moves as `<Left>` and so on. Any other has no vim notation:
    /// a recording skips it, and says so once.
    pub(crate) fn record_typing(&mut self, invocation: &Invocation) {
        let command = invocation.command.as_str();
        let keys: Vec<VimKey> = match command {
            // As it's typed ([`type_as_keys`]): a line break, `\r` too, as
            // `<CR>`, and a tab as `<Tab>`.
            INSERT => {
                let text = invocation.args.get("text").and_then(|text| text.as_str());
                (line_breaks(text.unwrap_or_default()).chars())
                    .map(|ch| match ch {
                        '\n' => VimKey::Enter,
                        '\t' => VimKey::Tab,
                        ch => VimKey::Char(ch),
                    })
                    .collect()
            }
            NEWLINE => vec![VimKey::Enter],
            BACKSPACE => vec![VimKey::Backspace],
            command => match MOVES.iter().find(|(id, _)| *id == command) {
                Some(&(_, key)) => vec![key],
                None => {
                    let recording = self.vim.as_ref().and_then(VimState::recording);
                    if recording.is_some()
                        && !self.vim_replaying
                        && !std::mem::replace(&mut self.vim_unrecorded_logged, true)
                    {
                        log::warn!(
                            "a macro can't record {command}, which has no vim key: it's left out"
                        );
                    }
                    Vec::new()
                }
            },
        };
        for key in keys {
            self.record_vim_key(key);
        }
    }

    /// Records what the command line took for a recording: `text`, the
    /// line's as it ends, then `end`, the key that ended it (`<CR>`, `<Esc>`
    /// or `<BS>` on an empty line). Typed after the line's prefill, which a
    /// replay starts it with too: any of that taken out as `<BS>`s first.
    /// The host's line calls it just before it submits or cancels.
    pub fn record_command_line(&mut self, text: &str, end: VimKey) {
        let Some(state) = &self.vim else {
            return;
        };
        if !matches!(state.mode(), fe_vim::Mode::CmdLine(_)) {
            return;
        }
        let prefill = state.command_line_prefill();
        let kept = prefill
            .chars()
            .zip(text.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let taken = prefill.chars().count() - kept;
        let typed: Vec<VimKey> = (0..taken)
            .map(|_| VimKey::Backspace)
            .chain(text.chars().skip(kept).map(VimKey::Char))
            .chain([end])
            .collect();
        for key in typed {
            self.record_vim_key(key);
        }
    }

    /// A keystroke the keymap left for the editor, in normal,
    /// operator-pending or a visual mode: fed to the reducer.
    pub(crate) fn on_vim_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.feed_vim(&event.keystroke, window, cx) {
            cx.stop_propagation();
        }
    }

    /// Feeds `keystroke` to the reducer, in any mode but insert. Returns
    /// whether vim took it: it waits, cancels or commits.
    /// Otherwise it's forwarded, to do nothing. The keys a map it completes
    /// resolves to run before it returns, and if it leaves keys held for a
    /// longer map, they wait [`CHORD_TIMEOUT`] for the next.
    fn feed_vim(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.sync_vim(cx);
        self.drop_remap_wait(cx);
        if self.accepts_text() {
            return false;
        }
        let Some(state) = &mut self.vim else {
            return false;
        };
        let taken = match vim_key(keystroke) {
            // A key vim has no name for cancels a pending command, or keys
            // held for a map, and otherwise goes on, to do nothing.
            None if state.pending().kind().is_none() && !state.remap_waiting() => false,
            None => {
                state.feed(VimKey::Esc);
                cx.notify();
                true
            }
            Some(key) => match (single_caret_key(state, key), state.feed(key)) {
                (single, Step::Wait(_) | Step::Cancel) => {
                    if single {
                        self.keep_primary_caret(cx);
                    }
                    // The key context changes.
                    cx.notify();
                    true
                }
                (_, Step::Forward) => false,
                (_, Step::Commit(invocation)) => {
                    // The stack the key came in, stamped as the reducer left
                    // the state: nothing pending.
                    let stack = crate::keymap::context_stack(&window.context_stack());
                    let invocation = invocation.context(stamp(stack, self.vim.as_ref()));
                    self.run_vim(&invocation, cx);
                    cx.notify();
                    true
                }
            },
        };
        self.replay_vim(cx);
        self.wait_for_remap(window, cx);
        taken
    }

    /// If keys are held for a longer map, ends the wait for the next after
    /// [`CHORD_TIMEOUT`], or when the editor loses focus, as nvim's
    /// 'timeoutlen' does ([`EditorView::resolve_remap`]).
    /// Only the editor keys go to waits ([`EditorView::takes_keys`]): one
    /// that doesn't resolves what it holds at once, and another editor's
    /// wait resolves before this one takes its place.
    fn wait_for_remap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.vim.as_ref().is_some_and(VimState::remap_waiting) {
            return;
        }
        if !self.takes_keys(window) {
            self.resolve_remap(cx);
            return;
        }
        let id = cx.entity_id();
        let other = (cx.default_global::<Vim>().remap_wait)
            .take_if(|wait| wait.editor.entity_id() != id)
            .map(|wait| wait.editor.clone());
        if let Some(other) = other {
            other.update(cx, |view, cx| view.resolve_remap(cx)).ok();
        }
        let timer = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(CHORD_TIMEOUT).await;
            view.update_in(cx, |view, window, cx| view.remap_timed_out(window, cx))
                .ok();
        });
        let blur = cx.on_blur(&self.focus, window, Self::remap_timed_out);
        let wait = RemapWait {
            editor: cx.weak_entity(),
            _timer: timer,
            _blur: blur,
        };
        let replaced = cx.default_global::<Vim>().remap_wait.replace(wait);
        drop(replaced);
    }

    /// [`EditorView::wait_for_remap`] once the update under way is over, in
    /// the active window if it has the editor focused, if this editor holds keys
    /// for a longer map and doesn't wait yet: for a replay that came some
    /// other way than a key fed here, such as a `@` the keymap dispatched
    /// or a click. With no window focusing it, what it holds resolves.
    fn wait_for_remap_later(&self, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        cx.defer(move |cx| {
            let Some(view) = view.upgrade() else {
                return;
            };
            let id = view.entity_id();
            let waits = cx.try_global::<Vim>().is_some_and(|vim| {
                (vim.remap_wait.as_ref()).is_some_and(|wait| wait.editor.entity_id() == id)
            });
            let editor = view.read(cx);
            if waits || !editor.vim.as_ref().is_some_and(VimState::remap_waiting) {
                return;
            }
            let active = cx.active_window();
            let armed = active.and_then(|handle| {
                handle
                    .update(cx, |_, window, cx| {
                        view.update(cx, |view, cx| {
                            let takes = view.takes_keys(window);
                            if takes {
                                view.wait_for_remap(window, cx);
                            }
                            takes
                        })
                    })
                    .ok()
            });
            if armed != Some(true) {
                view.update(cx, |view, cx| view.resolve_remap(cx));
            }
        });
    }

    /// Whether keys typed now come to this editor: it's focused in the
    /// active window. Another window's editor keeps its focus there.
    fn takes_keys(&self, window: &Window) -> bool {
        window.is_window_active() && self.focus.is_focused(window)
    }

    /// The wait for the rest of a map is over: what was held resolves, and
    /// if that holds keys again, they wait anew.
    fn remap_timed_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.resolve_remap(cx);
        self.wait_for_remap(window, cx);
    }

    /// Resolves the keys held for a longer map as a timeout does
    /// ([`VimState::remap_timeout`]) and runs what they resolve to.
    fn resolve_remap(&mut self, cx: &mut Context<Self>) {
        self.drop_remap_wait(cx);
        let Some(state) = self.vim.as_mut().filter(|state| state.remap_waiting()) else {
            return;
        };
        state.remap_timeout();
        self.replay_vim(cx);
        cx.notify();
    }

    /// Drops this editor's wait for the rest of a map, if it has one.
    fn drop_remap_wait(&self, cx: &mut Context<Self>) {
        let id = cx.entity_id();
        let Some(vim) = cx.try_global::<Vim>() else {
            return;
        };
        if vim
            .remap_wait
            .as_ref()
            .is_some_and(|wait| wait.editor.entity_id() == id)
        {
            let dropped = cx.global_mut::<Vim>().remap_wait.take();
            drop(dropped);
        }
    }

    /// Feeds the keys a `@` or a map left ([`VimState::take_replay`]),
    /// synchronously and in order, each where a typed key would go by the
    /// mode at that moment: no keystroke is dispatched, which could leave
    /// GPUI's propagation flag stopped. A `@` inside the macro leaves keys
    /// that are fed before the rest. A key that fails drops the rest of the
    /// replay ([`VimState::stop_replay`]). The command line's keys edit a
    /// line kept here, which isn't shown, and its `<CR>` or `<Esc>`
    /// submits it. A map's keys that leave it open put its text on the
    /// host's line, as the line's prefill
    /// ([`VimState::set_command_line_prefill`]); known limit: a `<Left>` or
    /// `<Right>` in it isn't followed, and the caret starts at the end.
    fn replay_vim(&mut self, cx: &mut Context<Self>) {
        if self.vim_replaying {
            // A key of the replay under way ran this: the loop below takes
            // what it left.
            return;
        }
        let Some(state) = &mut self.vim else {
            return;
        };
        let mut queue: VecDeque<VimKey> = state.take_replay().into();
        if queue.is_empty() {
            return;
        }
        self.vim_replaying = true;
        let mut line = None;
        let mut handed = false;
        while let Some(key) = queue.pop_front() {
            if !self.replay_key(key, &mut line, &mut handed, cx) {
                let rest = !queue.is_empty();
                queue.clear();
                if let Some(state) = &mut self.vim {
                    // Known limit: the host runs the command after this
                    // update, so what the map does after it isn't run.
                    let rest = rest || !state.take_replay().is_empty();
                    state.stop_replay();
                    if handed && rest {
                        let note = "a map's keys after a file or window command aren't run";
                        log::warn!("{note}");
                        state.set_host_message(note);
                    }
                }
                break;
            }
            let Some(state) = &mut self.vim else {
                break;
            };
            for key in state.take_replay().into_iter().rev() {
                queue.push_front(key);
            }
        }
        self.vim_replaying = false;
        self.wait_for_remap_later(cx);
        if let Some(text) = line
            && let Some(state) = &mut self.vim
        {
            match state.expanding() {
                true => state.set_command_line_prefill(text),
                false => log::warn!(
                    "a macro replay ended with the command line open: its text is dropped"
                ),
            }
        }
        cx.notify();
    }

    /// Feeds one replayed `key`, `line` holding the command line's text
    /// while it's open. Returns whether it worked, and whether the replay
    /// goes on: an ex line a map ran that left a file or window command
    /// (`:w`) ends it, `handed` set, as the host runs the command only once
    /// this update is over.
    fn replay_key(
        &mut self,
        key: VimKey,
        line: &mut Option<String>,
        handed: &mut bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.sync_visual();
        let Some(state) = &mut self.vim else {
            return false;
        };
        let mode = state.mode();
        if !matches!(mode, fe_vim::Mode::CmdLine(_)) {
            *line = None;
        }
        match mode {
            fe_vim::Mode::Insert | fe_vim::Mode::Replace => {
                state.record_key(key);
                let command = match key {
                    VimKey::Char(ch) => {
                        self.type_text(ch.encode_utf8(&mut [0; 4]), cx);
                        return true;
                    }
                    // `<C-j>` (`<NL>`) is `<CR>` here.
                    VimKey::Enter | VimKey::Ctrl('j') => NEWLINE,
                    VimKey::Backspace => BACKSPACE,
                    VimKey::Esc => {
                        let normal = match mode {
                            fe_vim::Mode::Replace => REPLACE_NORMAL_MODE,
                            _ => NORMAL_MODE,
                        };
                        let normal = Invocation::of(normal)
                            .expect("a valid command ID")
                            .source(Source::Keyboard);
                        let context = stamp(fe_editor::commands::context(), self.vim.as_ref());
                        return self.run_vim_once(&normal.context(context), cx);
                    }
                    key => match MOVES.iter().find(|(_, moved)| *moved == key) {
                        Some(&(command, _)) => command,
                        None => return false,
                    },
                };
                let invocation = Invocation::of(command)
                    .expect("a valid command ID")
                    .source(Source::Keyboard);
                self.run_command(invocation, None, cx);
                true
            }
            fe_vim::Mode::CmdLine(_) => {
                state.record_key(key);
                // Read before the line runs, which may read keys of its own.
                let mapped = state.expanding();
                let text = line.get_or_insert_with(|| state.command_line_prefill().to_owned());
                let invocation = match key {
                    VimKey::Char(ch) => {
                        text.push(ch);
                        return true;
                    }
                    VimKey::Backspace if text.pop().is_some() => return true,
                    // `<BS>` on an empty line, and `<C-c>`, cancel it. A
                    // replay's `<Esc>` runs it, as in nvim (`:help c_<Esc>`).
                    VimKey::Backspace | VimKey::Ctrl('c') => {
                        Invocation::of("vim.command_line_cancel").expect("a valid command ID")
                    }
                    VimKey::Enter | VimKey::Ctrl('j') | VimKey::Esc => {
                        Invocation::of("vim.command_line_submit")
                            .expect("a valid command ID")
                            .args(fe_command::Value::from_iter([("text", text.as_str())]))
                    }
                    _ => return false,
                };
                *line = None;
                let context = stamp(fe_editor::commands::context(), self.vim.as_ref());
                let invocation = invocation.source(Source::Keyboard).context(context);
                let ok = self.run_vim_once(&invocation, cx);
                // A file or window command (`:w`) is the host's line's to
                // run, not a macro replay's; but a map's is left for the
                // host, which takes it from the editor, and ends the map.
                if mapped && self.vim.as_ref().is_some_and(VimState::has_host_command) {
                    *handed = true;
                    return false;
                }
                if let Some(command) = self.take_vim_host_command() {
                    log::warn!("a macro replay doesn't run {command:?}");
                }
                ok && self.vim.as_ref().is_some_and(|s| !s.failed())
            }
            // The arrows too, so a map's are matched against the maps: with
            // nothing pending the reducer reads them as the keymap's
            // `vim.move`s do.
            _ => match (single_caret_key(state, key), state.feed(key)) {
                (single, Step::Wait(_) | Step::Cancel) => {
                    if single {
                        self.keep_primary_caret(cx);
                    }
                    self.vim.as_ref().is_some_and(|s| !s.failed())
                }
                (_, Step::Forward) => false,
                (_, Step::Commit(invocation)) => {
                    let mapped = state.expanding();
                    let stack = fe_editor::commands::context();
                    let invocation = invocation.context(stamp(stack, self.vim.as_ref()));
                    let ok = self.run_vim_once(&invocation, cx);
                    // A map's `do` or `dp` hands the host a command too, and
                    // ends the map, as an ex line's does.
                    if mapped && self.vim.as_ref().is_some_and(VimState::has_host_command) {
                        *handed = true;
                        return false;
                    }
                    ok && self.vim.as_ref().is_some_and(|s| !s.failed())
                }
            },
        }
    }

    /// Keeps only the primary caret, if there are more, as `escape` does
    /// with vim off (`editor.single_selection`).
    fn keep_primary_caret(&mut self, cx: &mut Context<Self>) {
        if self.model.selections().len() > 1 {
            let single = Invocation::of("editor.single_selection")
                .expect("a valid command ID")
                .source(Source::Keyboard);
            self.run_command(single, None, cx);
        }
    }

    /// Runs `invocation` through `fe-vim`'s registry, as `run_command` runs
    /// the editor's. Its context is used as given.
    /// Returns whether it ran without an error. The keys a `@` leaves are
    /// then replayed ([`EditorView::replay_vim`]), whichever way the
    /// invocation came.
    pub(crate) fn run_vim(&mut self, invocation: &Invocation, cx: &mut Context<Self>) -> bool {
        let ok = self.run_vim_once(invocation, cx);
        self.replay_vim(cx);
        ok
    }

    /// [`EditorView::run_vim`] without the replay: for the replay's own keys.
    pub(crate) fn run_vim_once(&mut self, invocation: &Invocation, cx: &mut Context<Self>) -> bool {
        // Every vim command comes this way, however its keys came: typed,
        // replayed by the keymap or a macro, or dispatched as an action.
        if self.refuses_vim(invocation) {
            self.refuse_vim(invocation, cx);
            return false;
        }
        self.note_command_line(invocation);
        if invocation.command.as_str() == "vim.record" {
            // A new recording: what it can't hold is logged again.
            self.vim_unrecorded_logged = false;
        }
        let registry = registry();
        let Some(result) = self.dispatch_vim(&registry, invocation, cx) else {
            log::warn!("{} needs vim on", invocation.command);
            return false;
        };
        let mut ok = result.is_ok();
        let mutates = registry
            .get(&invocation.command)
            .is_some_and(|meta| meta.mutates_buffer);
        if let Err(error) = self.dispatched(result, invocation, mutates, cx) {
            log::warn!("{error}");
            ok = false;
        }
        ok
    }

    /// Builds the [`VimTarget`] and dispatches; `None` with vim off.
    fn dispatch_vim(
        &mut self,
        registry: &Registry<VimCommands>,
        invocation: &Invocation,
        cx: &mut App,
    ) -> Option<Result<Dispatched<TransactionId>, DispatchError>> {
        let page_rows = self.page_rows();
        let syntax = synced_syntax(&self.syntax, &self.model);
        let language = loaded_language(&self.syntax);
        let state = self.vim.as_mut()?;
        // Out of the global while the host holds `cx`, and back after.
        let mut registers = std::mem::take(&mut cx.default_global::<Vim>().registers);
        let mut host = ViewHost {
            cx,
            page_rows,
            syntax,
            language,
            vim: true,
        };
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut host,
            state,
            registers: &mut registers,
        };
        let result = registry.dispatch(&mut target, invocation);
        cx.default_global::<Vim>().registers = registers;
        Some(result)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod insert_tests;

#[cfg(test)]
mod visual_tests;

#[cfg(test)]
mod search_tests;

#[cfg(test)]
mod clipboard_tests;

#[cfg(test)]
mod scroll_tests;

#[cfg(test)]
mod viewport_resize_tests;

#[cfg(test)]
mod jump_setting_tests;

#[cfg(test)]
mod remap_setting_tests;

#[cfg(test)]
mod macro_tests;

#[cfg(test)]
mod remap_tests;

#[cfg(test)]
mod replace_tests;

#[cfg(test)]
mod auto_indent_tests;
