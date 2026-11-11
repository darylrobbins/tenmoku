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
//! [`VimState::line_break`], and `ctrl-t` and `ctrl-d` shift the row's
//! indent ([`VimState::indent_in`], [`VimState::indent_out`]), in replace mode
//! too, and `ctrl-w` and `ctrl-u` delete the word and the typed text before
//! the caret ([`VimState::insert_key`], which any other ctrl key goes to,
//! to be dropped). `ctrl-o` there runs one normal-mode command ([`VimState::one_command`]):
//! the mode is `normal` with the flag `vim.insert_pending` until it's done,
//! the caret a block, and keys go to the reducer as in normal mode; once
//! `fe-vim`'s registry has run the command ([`fe_vim::dispatch`]), or the
//! reducer committed `vim.resume_insert` for an `escape` with nothing
//! pending, the session resumes. Any other editor command, and a mouse click
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
//! `<CR>`, `<BS>`, `<Esc>`, the arrows and the ctrl keys vim takes there, and the command line's text and
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
//! A map's `<Cmd>…<CR>` runs without the command line, in whatever mode the
//! keys before it left: an ex line through `vim.ex_line`, and
//! `<Cmd>fe:{command id}<CR>` as a keymap binding runs that command, from
//! the focused element once the keystroke's update is over, in its stack
//! with the editor's frame stamped, the map's keys after it held until then
//! ([`EditorView::run_remap_command_later`]). With no command of that ID it
//! says `E492: Not an editor command: fe:…`.
//!
//! A file or window command a map leaves (`:w<CR>`, or `do` and `dp`)
//! suspends the replay if keys follow it, as a `<Cmd>fe:…<CR>` command does:
//! the editor holds typed keys, and the host reports the outcome back
//! ([`EditorView::finish_vim_host`]), so `:w<CR>dd` writes, then deletes
//! the line, and a write that failed ends the map before the delete. The
//! host that runs a command on its own time claims the report
//! ([`EditorView::hold_vim_host`]); one that answers at once is settled
//! after it runs ([`EditorView::settle_vim_host`]), failed if it left a
//! message that wasn't a report ([`EditorView::report_vim_message`]). Text
//! edited meanwhile is what the keys after it run on. A click ends the
//! wait. With no host taking the command the map ends there, and the status
//! bar says "a map's keys after a file or window command aren't run".
//!
//! Two known limits, each a card of its own:
//!
//! - **Some host commands end the map.** One that finishes on its own
//!   time with no report back (`:e`, a git command) ends the map there
//!   ([`EditorView::end_vim_host`]), as does a quit (`:wq`), as it ends vim;
//!   a `<Cmd>fe:…<CR>` command that leaves a file or window command
//!   (`vim.diff_get`) does too, as its host takes the command before the
//!   replay could wait for it. A `:normal` stops at a `<Cmd>fe:…<CR>`
//!   among its keys, which it doesn't run: "fe: commands don't run inside
//!   :normal". (fe-vim can suspend it there instead, and the replay resumes
//!   it once the command has run, but the host doesn't turn that on yet:
//!   [`VimState::set_normal_resumes`].)
//! - **Every mouse press resolves a held prefix first.** The view resolves
//!   the held keys as a timeout would ([`EditorView::resolve_held_prefix`]);
//!   if that edited the text or left a map's `<Cmd>fe:…<CR>` replay waiting,
//!   the press runs again once the replay is done and a frame has laid out
//!   the new text, so its destination is measured on it. A plain click then
//!   drops what the keys leave pending ([`EditorView::cancel_vim`]).
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

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    ops::{Range, RangeInclusive},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use fe_buffer::{BufferEvent, Subscription as BufferSubscription, Text, TransactionId, Version};
use fe_command::{CommandId, CommandMeta, Value};
use fe_command::{ContextStack, DispatchError, Dispatched, Effect, Invocation, Registry, Source};
use fe_config::VimJumpStyle;
use fe_config::settings::VimQuickScope;
use fe_editor::decorations::{Decoration, DecorationKind, DecorationSet, ProviderId, Tint};
use fe_editor::{EditorModel, commands::EditorHost};
use fe_keymap::{CatalogError, CommandCatalog};
use fe_vim::{
    CommandLineKind, Registers, Remap, RemapCommand, RemapInsert, RemapIssue, RemapSet, Step,
    VimCommands, VimKey, VimState, VimTarget, WhichKeyGroupName, jump::JumpStyle, search::Pattern,
};
use gpui::{
    AnyWindowHandle, App, Context, Entity, FocusHandle, Global, KeyContext, KeyDownEvent, Pixels,
    Point, SharedString, Subscription, Task, WeakEntity, WeakFocusHandle, Window,
};
use kiln_l10n::tr;

use crate::{
    EditorView,
    commands::{ViewHost, loaded_language},
    keymap::CHORD_TIMEOUT,
    view::synced_syntax,
};

pub(crate) mod which_key;
mod which_key_icons;
mod which_key_layout;

pub use which_key::{set_host_commands, set_vim_which_key};

/// How long a yank's text stays highlighted: nvim's `vim.hl.on_yank` default.
const YANK_FLASH: Duration = Duration::from_millis(150);

/// Sets whether a vim yank flashes the text it took and for how long
/// (`vim.highlight_yank`, `vim.highlight_yank_ms`). A flash under way keeps
/// its timer.
pub fn set_vim_highlight_yank(cx: &mut App, on: bool, duration: Duration) {
    cx.default_global::<Vim>().highlight_yank = Some((on, duration));
}

/// Sets when the caret row's `f` and `F` targets are underlined
/// (`vim.quick_scope`), in every window at once.
pub fn set_vim_quick_scope(cx: &mut App, mode: VimQuickScope) {
    let vim = cx.default_global::<Vim>();
    if std::mem::replace(&mut vim.quick_scope, mode) != mode {
        cx.refresh_windows();
    }
}

/// When the caret row's `f` and `F` targets are underlined.
pub(crate) fn quick_scope_mode(cx: &App) -> VimQuickScope {
    cx.try_global::<Vim>()
        .map_or_else(VimQuickScope::default, |vim| vim.quick_scope)
}

/// A yank's text highlighted for [`YANK_FLASH`], or until the buffer next
/// changes. Dropped, its timer ends; [`EditorView::end_yank_flash`] also
/// takes its provider off the model.
pub(crate) struct YankFlash {
    provider: ProviderId,
    /// The buffer's version the ranges are of.
    version: Version,
    _timer: Task<()>,
}

/// An open `/` or `?` line's preview ('incsearch'), from the line opening
/// until it closes: see [`EditorView::preview_search`]; or a `:` line's
/// ('inccommand'), from its first preview: see
/// [`EditorView::preview_substitute`].
pub(crate) struct SearchPreview {
    /// Where the view was before the line opened.
    place: ViewPlace,
    /// The folds hiding the shown match, opened to show it, and the text
    /// their rows are of, with the buffer's edits since: they move the
    /// folds ([`follow_fold_edits`]).
    opened: Vec<RangeInclusive<u32>>,
    opened_in: Option<(Text, BufferSubscription)>,
    /// The match shown, in the text at its version, and the line's
    /// pattern, whose matches paint in place of any other: `None` while the
    /// line previews nothing.
    shown: Option<(Range<usize>, Pattern, Version)>,
    /// Where the next frame scrolls to, as it does to a caret: the shown
    /// match's start.
    reveal: Option<usize>,
    /// A `:s` line's matches and rows as they would read, painted by this
    /// provider on the model, of the buffer at this version.
    substitute: Option<(ProviderId, Version)>,
}

/// `folds`, row ranges of the text `before`, followed through the batches
/// of edits `events` received since, as the display's own folds follow each
/// batch: a fold whose hidden rows an edit touched, or whose header it split
/// or joined, is dropped, and one after it shifts.
fn follow_fold_edits(
    mut folds: Vec<RangeInclusive<u32>>,
    before: &Text,
    events: &BufferSubscription,
) -> Vec<RangeInclusive<u32>> {
    let mut before = before.clone();
    for event in events.drain() {
        let BufferEvent::Edited { edits, text, .. } = event else {
            continue;
        };
        folds = follow_row_edits(folds, &before, &text, &edits);
        before = text;
    }
    folds
}

/// [`follow_fold_edits`] through one batch, from the text `old` to `new`.
fn follow_row_edits(
    mut folds: Vec<RangeInclusive<u32>>,
    old: &Text,
    new: &Text,
    batch: &[fe_buffer::Edit],
) -> Vec<RangeInclusive<u32>> {
    for edit in batch {
        let (old_start, old_end) = (
            old.offset_to_point(edit.old.start),
            old.offset_to_point(edit.old.end),
        );
        let (new_start, new_end) = (
            new.offset_to_point(edit.new.start),
            new.offset_to_point(edit.new.end),
        );
        // Rows an edit only ends at the start of are untouched.
        let whole_rows = [old_start, old_end, new_start, new_end]
            .iter()
            .all(|point| point.column == 0);
        let touched = u32::from(!whole_rows);
        let start = new_start.row;
        let old_rows = old_end.row - old_start.row + touched;
        let new_rows = new_end.row - new_start.row + touched;
        let old_end = start + old_rows;
        let delta = i64::from(new_rows) - i64::from(old_rows);
        folds.retain_mut(|fold| {
            let (first, last) = (*fold.start(), *fold.end());
            let touches_hidden = start <= last && old_end.max(start + 1) > first + 1;
            let touches_header = start <= first && old_end > first;
            if touches_hidden || (touches_header && delta != 0) {
                return false;
            }
            if first >= old_end {
                let shift = |row: u32| (i64::from(row) + delta) as u32;
                *fold = shift(first)..=shift(last);
            }
            true
        });
    }
    folds
}

/// Where a view is scrolled: its scroll, the buffer and display rows first
/// in view (`None` before a frame), and whether the next frame reveals the
/// caret.
#[derive(Clone, Copy)]
pub(crate) struct ViewPlace {
    scroll: Point<Pixels>,
    top: Option<(u32, u32)>,
    autoscroll: bool,
    /// [`EditorView::band_spares_caret`] and [`EditorView::clear_band`].
    band: (bool, bool),
}

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

/// What the status bar says of a map's keys after a file or window command,
/// which the host runs once the keys' update is over: they aren't run.
fn handed_host_command() -> SharedString {
    tr!("gpui-vim-map-keys-after-host-command")
}

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
    /// `vim.jump_style`: how the jump reads its target.
    jump_style: JumpStyle,
    /// `vim.replace_with_register`: whether `gr` is ReplaceWithRegister's.
    replace_with_register: bool,
    /// `vim.subword_motions`: whether `w b e ge` step by subwords.
    subword_motions: bool,
    /// `vim.remap` and `vim.leader`, built.
    remaps: Arc<RemapSet>,
    /// The names which-key titles groups of maps by: a vimrc's `:fegroup`s
    /// and `vim.which_key_group`.
    which_key_groups: Arc<Vec<WhichKeyGroupName>>,
    /// `vim.which_key` and `vim.which_key_delay_ms`.
    which_key: which_key::WhichKeySettings,
    /// `vim.highlight_yank` and `vim.highlight_yank_ms`; unset, on for [`YANK_FLASH`].
    highlight_yank: Option<(bool, Duration)>,
    /// `vim.quick_scope`.
    quick_scope: VimQuickScope,
    /// The editor holding keys for a longer map: one at a time, as only the
    /// focused editor takes keys.
    remap_wait: Option<RemapWait>,
    /// Where each editor's last keystroke came in, for a replay that starts
    /// some other way, such as a `@` the keymap dispatched.
    origins: Vec<(gpui::EntityId, ReplayOrigin)>,
    /// The keystrokes each editor took while its replay held for a map's
    /// `<Cmd>fe:…<CR>` command, in order, to go on once the map is over.
    held_keys: Vec<(gpui::EntityId, gpui::Keystroke)>,
    /// The replay each editor suspended at a map's file or window command,
    /// until the host reports it ([`EditorView::finish_vim_host`]).
    host_waits: HashMap<gpui::EntityId, HostWait>,
    /// The last [`VimHostHold`] handed out.
    host_token: u64,
    /// Each editor whose keys after a map's `<Cmd>` go on from the host's
    /// open line ([`EditorView::replay_line_rest`]), and how far they are.
    live_lines: HashMap<gpui::EntityId, LiveLine>,
    /// Drops a pending `<C-r>` when its editor loses focus; one at a time,
    /// as only the focused editor takes keys.
    register_blur: Option<gpui::Subscription>,
}

/// Where the keys after a map's `<Cmd>` on the host's open line are
/// ([`EditorView::replay_line_rest`]).
#[derive(Clone, Debug, PartialEq, Eq)]
enum LiveLine {
    /// They run, or wait for a command they ran; `closed` once they've
    /// closed the host's line (any line they open after is theirs).
    Running { closed: bool },
    /// They're over, the line still open: what they left it.
    Done(LineContinued),
}

/// What the keys after a map's `<Cmd>` did to the host's open line, once
/// they've run ([`EditorView::take_line_text`],
/// [`EditorView::take_line_ended`]).
#[derive(Clone, Debug, PartialEq, Eq)]
enum LineContinued {
    /// The line is still open, holding this text.
    Shows(String),
    /// They ended the line, and may have opened another
    /// ([`VimState::command_line_prefill`] its text).
    Ended,
}

/// What the `c` maps make of keys typed on the command line
/// ([`EditorView::remap_line_key`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LineKeys {
    /// The keys to type now, in order, each with whether a map's right
    /// side made it.
    pub typed: Vec<(VimKey, bool)>,
    /// The keys after one that ends the line (`<CR>`, `<Esc>`, `<C-c>`), or
    /// from a `<Cmd>` on: the editor's to feed
    /// ([`EditorView::replay_line_rest`]), matched in the mode they find.
    pub rest: Vec<VimKey>,
    /// Matching stopped at the last key typed, a `<BS>`, for the line it
    /// leaves: the keys after it, not yet matched. The host takes the
    /// `<BS>` out as its line does, then hands these back with the text left
    /// ([`EditorView::remap_line_more`]); or, if it cancelled the line (on
    /// an empty one), feeds them with `rest`, in the mode that leaves.
    pub then: Option<Vec<VimKey>>,
}

/// A map's replay, suspended while the host runs the file or window command
/// the map left: the keys after it, and what the loop held.
struct HostWait {
    token: u64,
    /// The vim switch's generation it was suspended in: a reset of vim's
    /// state ends it ([`EditorView::end_stale_host_wait`]).
    generation: u64,
    queue: VecDeque<VimKey>,
    line: Option<String>,
    origin: Option<ReplayOrigin>,
    /// The host's command runs on, past its own update (a write): it
    /// reports back itself ([`EditorView::hold_vim_host`]).
    claimed: bool,
    /// The host answered with a report, not an error
    /// ([`EditorView::report_vim_message`]).
    reported: bool,
}

/// The host's claim on the completion of the file or window command it
/// took from an editor whose map waits for it: report the outcome with
/// [`EditorView::finish_vim_host`].
#[derive(Clone, Copy, Debug)]
pub struct VimHostHold(u64);

/// What ends an editor's wait for the rest of a map: [`CHORD_TIMEOUT`]
/// with no key, as nvim's 'timeoutlen', or its focus going. Dropped, it
/// ends neither.
struct RemapWait {
    editor: WeakEntity<EditorView>,
    /// Where the keys held came in, for what they resolve to.
    origin: ReplayOrigin,
    _timer: Task<()>,
    _blur: Subscription,
}

/// Where the keys a replay feeds came in: the window, and the stack of the
/// element they went to there, the editor focused in it. A map's
/// `<Cmd>fe:…<CR>` runs there, whatever has focus by then.
#[derive(Clone)]
struct ReplayOrigin {
    window: AnyWindowHandle,
    stack: ContextStack,
}

impl ReplayOrigin {
    /// A keystroke's in `window`, as the keymap matched it.
    fn of(window: &Window) -> ReplayOrigin {
        ReplayOrigin {
            window: window.window_handle(),
            stack: crate::keymap::context_stack(&window.context_stack()),
        }
    }
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

/// Sets 'ignorecase' and 'smartcase' on the registers every editor's vim
/// shares (`vim.ignorecase`, `vim.smartcase`): open editors' next search
/// follows.
pub fn set_vim_search_case(cx: &mut App, ignorecase: bool, smartcase: bool) {
    cx.default_global::<Vim>()
        .registers
        .set_search_case(ignorecase, smartcase);
}

/// Sets 'incsearch' on the shared registers (`vim.incsearch`).
pub fn set_vim_incsearch(cx: &mut App, on: bool) {
    cx.default_global::<Vim>().registers.set_incsearch(on);
}

/// Sets 'inccommand' on the shared registers (`vim.inccommand`): with it
/// off, a `:s` line previews nothing ([`EditorView::preview_substitute`]).
pub fn set_vim_inccommand(cx: &mut App, on: bool) {
    let vim = cx.default_global::<Vim>();
    vim.registers.set_inccommand(on);
    if on {
        return;
    }
    // A preview shown comes down now, not at the line's next edit.
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    for editor in editors {
        editor
            .update(cx, |view, cx| view.drop_substitute_preview(cx))
            .ok();
    }
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

/// Sets how the jump reads its target in every editor, and in any opened or
/// switched on later (`vim.jump_style`): two chars then labels (leap), or
/// one char then labels on every match in view, both ways (flash).
pub fn set_vim_jump_style(cx: &mut App, style: VimJumpStyle) {
    let style = match style {
        VimJumpStyle::Leap => JumpStyle::Leap,
        VimJumpStyle::Flash => JumpStyle::Flash,
    };
    let vim = cx.default_global::<Vim>();
    vim.jump_style = style;
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    for editor in editors {
        editor
            .update(cx, |view, _| {
                if let Some(state) = &mut view.vim {
                    state.set_jump_style(style);
                }
            })
            .ok();
    }
}

/// Gives `gr{motion}`, `grr` and visual `gr` to ReplaceWithRegister in every
/// editor, and in any opened or switched on later
/// (`vim.replace_with_register`).
pub fn set_vim_replace_with_register(cx: &mut App, on: bool) {
    let vim = cx.default_global::<Vim>();
    vim.replace_with_register = on;
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    for editor in editors {
        editor
            .update(cx, |view, _| {
                if let Some(state) = &mut view.vim {
                    state.set_replace_with_register(on);
                }
            })
            .ok();
    }
}

/// Makes `w`, `b`, `e` and `ge` step by the parts of camelCase and
/// snake_case names in every editor, and in any opened or switched on later
/// (`vim.subword_motions`).
pub fn set_vim_subword_motions(cx: &mut App, on: bool) {
    let vim = cx.default_global::<Vim>();
    vim.subword_motions = on;
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    for editor in editors {
        editor
            .update(cx, |view, _| {
                if let Some(state) = &mut view.vim {
                    state.set_subword_motions(on);
                }
            })
            .ok();
    }
}

/// Gives every editor the set `remaps` build with `leader` (in vim
/// notation), and any opened or switched on later, replacing the set
/// before: keys an editor holds for a longer map go on at once, unmapped.
/// A later record for the same keys in the same mode replaces an earlier
/// one. A record that can't load is left out alone, with its issue.
pub fn set_vim_remaps(cx: &mut App, remaps: &[Remap], leader: &str) -> Vec<RemapIssue> {
    let (set, issues) = RemapSet::new(remaps, leader);
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
}

/// Names groups of maps for every editor's which-key popup
/// ([`fe_vim::WhichKeyLabel::Group`]): for the same mode and keys, a later
/// name wins.
pub fn set_vim_which_key_groups(cx: &mut App, groups: Vec<WhichKeyGroupName>) {
    let groups = Arc::new(groups);
    let vim = cx.default_global::<Vim>();
    if vim.which_key_groups == groups {
        return;
    }
    vim.which_key_groups = Arc::clone(&groups);
    let editors: Vec<_> = vim.editors.iter().map(|(_, view)| view.clone()).collect();
    for editor in editors {
        editor
            .update(cx, |view, cx| {
                if let Some(state) = &mut view.vim {
                    state.set_which_key_groups(Arc::clone(&groups));
                }
                cx.notify();
            })
            .ok();
    }
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

/// The host's side of a map's file or window command: see the module docs.
impl EditorView {
    /// Claims the report of the command the host took from this editor, whose
    /// map waits for it, for a host that runs it past its own update: a
    /// write. Then [`EditorView::finish_vim_host`] reports it. `None` if no
    /// map waits for it, or another claim holds it.
    pub fn hold_vim_host(&mut self, cx: &mut Context<Self>) -> Option<VimHostHold> {
        if self.vim.as_ref().is_none_or(VimState::has_host_command) {
            return None;
        }
        let id = cx.entity_id();
        let wait = cx.default_global::<Vim>().host_waits.get_mut(&id)?;
        if std::mem::replace(&mut wait.claimed, true) {
            return None;
        }
        Some(VimHostHold(wait.token))
    }

    /// Reports the outcome of the command `hold` claimed, `ok` if it
    /// worked: the map goes on, or, if it failed, ends. Nothing if the map
    /// no longer waits, such as after a click.
    pub fn finish_vim_host(&mut self, hold: VimHostHold, ok: bool, cx: &mut Context<Self>) {
        if self.end_stale_host_wait(cx) {
            return;
        }
        let Some(wait) = self.take_host_wait(hold.0, cx) else {
            return;
        };
        let HostWait {
            queue,
            line,
            origin,
            ..
        } = wait;
        self.resume_replay(ok, false, queue, line, origin, cx);
    }

    /// Reports the outcome of the command the host just ran, if it claimed
    /// no report ([`EditorView::hold_vim_host`]) and a map waits for it: it
    /// failed if it left a message, which a report
    /// ([`EditorView::report_vim_message`]) isn't.
    pub fn settle_vim_host(&mut self, cx: &mut Context<Self>) {
        if self.vim.as_ref().is_none_or(VimState::has_host_command) {
            return;
        }
        let id = cx.entity_id();
        let Some(wait) = cx.default_global::<Vim>().host_waits.get(&id) else {
            return;
        };
        if wait.claimed {
            return;
        }
        let (token, reported) = (wait.token, wait.reported);
        let failed = self
            .vim
            .as_ref()
            .is_some_and(|state| state.message().is_some());
        self.finish_vim_host(VimHostHold(token), reported || !failed, cx);
    }

    /// Ends the map waiting for the command the host just ran, which
    /// finishes on its own time with no report (`what`, such as `:e`): the
    /// keys after it aren't run, and the status bar says so, unless the
    /// command left a message of its own.
    pub fn end_vim_host(&mut self, what: &str, cx: &mut Context<Self>) {
        if self.vim.as_ref().is_none_or(VimState::has_host_command) {
            return;
        }
        let id = cx.entity_id();
        let Some(wait) = cx.default_global::<Vim>().host_waits.get(&id) else {
            return;
        };
        if wait.claimed {
            return;
        }
        let token = wait.token;
        let unsaid = self
            .vim
            .as_ref()
            .is_some_and(|state| state.message().is_none());
        self.finish_vim_host(VimHostHold(token), false, cx);
        if unsaid && let Some(state) = &mut self.vim {
            state.set_host_message(tr!("gpui-vim-map-keys-not-run", what = what).to_string());
        }
    }

    /// Shows `message` as vim's, as [`EditorView::set_vim_message`] does,
    /// as a report of what the command did, not an error: a map goes on.
    pub fn report_vim_message(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        let id = cx.entity_id();
        if let Some(wait) = cx.default_global::<Vim>().host_waits.get_mut(&id) {
            wait.reported = true;
        }
        self.set_vim_message(message, cx);
    }
}

/// Lists `view`'s editor, focused by `focus`, until it's released.
pub(crate) fn register(focus: &FocusHandle, cx: &mut Context<EditorView>) {
    let view = cx.weak_entity();
    let id = view.entity_id();
    cx.default_global::<Vim>()
        .editors
        .push((focus.downgrade(), view));
    cx.on_release(move |_, cx| {
        let vim = cx.default_global::<Vim>();
        vim.editors.retain(|(_, view)| view.entity_id() != id);
        vim.origins.retain(|(editor, _)| *editor != id);
        vim.host_waits.remove(&id);
        vim.live_lines.remove(&id);
    })
    .detach();
}

/// Notes `origin` as where the editor `id`'s last keystroke came in.
fn note_origin(id: gpui::EntityId, origin: ReplayOrigin, cx: &mut App) {
    let origins = &mut cx.default_global::<Vim>().origins;
    origins.retain(|(editor, _)| *editor != id);
    origins.push((id, origin));
}

/// Where the editor `id`'s last keystroke came in, if one did.
fn last_origin(id: gpui::EntityId, cx: &App) -> Option<ReplayOrigin> {
    let vim = cx.try_global::<Vim>()?;
    let (_, origin) = vim.origins.iter().find(|(editor, _)| *editor == id)?;
    Some(origin.clone())
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

/// What the editor knows of `id` if it runs it itself when a key
/// dispatches it, as its `RunCommand` handler does
/// ([`EditorView::run_command`]): one of its own, vim's or the proposals'.
fn editor_command(id: &CommandId) -> Option<CommandMeta> {
    if let Some(meta) = registry().get(id) {
        return Some(meta.clone());
    }
    if let Some(meta) = crate::view::proposal_registry().get(id) {
        return Some(meta.clone());
    }
    if crate::diff_view::registry().get(id).is_some() {
        return None;
    }
    (crate::commands::command_list().into_iter()).find(|meta| meta.id == *id)
}

thread_local! {
    /// The host's full command catalog, for [`known_command`].
    static HOST_CATALOG: RefCell<Option<Box<dyn CommandCatalog>>> = const { RefCell::new(None) };
}

/// Hands fe-gpui the host's full command catalog, which knows the commands
/// that live above it (the workbench's panels), so a map's `fe:` command
/// of one isn't E492. Call after [`init`].
pub fn set_host_catalog(catalog: Box<dyn CommandCatalog>) {
    HOST_CATALOG.with_borrow_mut(|host| *host = Some(catalog));
}

/// Whether some command has `id`, where a key in an editor reaches: the
/// editor's, the diff view's, the app's, or a typed command (the
/// workbench's).
fn known_command(id: &CommandId, cx: &App) -> bool {
    let catalog = crate::commands::command_catalog();
    !matches!(
        catalog.check(id, &Value::Null),
        Err(CatalogError::UnknownCommand)
    ) || HOST_CATALOG.with_borrow(|host| {
        host.as_ref().is_some_and(|host| {
            !matches!(
                host.check(id, &Value::Null),
                Err(CatalogError::UnknownCommand)
            )
        })
    }) || kiln_platform::command_actions::typed_command(id, cx).is_some()
        || kiln_platform::commands::app_command_list(cx)
            .iter()
            .any(|meta| meta.id == *id)
}

/// Why the command `id` can't run with no args, if it can't: it needs some.
fn needs_args(id: &CommandId, cx: &App) -> Option<String> {
    match crate::commands::command_catalog().check(id, &Value::Null) {
        Ok(()) => None,
        Err(CatalogError::BadArgs(message)) => Some(message),
        Err(CatalogError::UnknownCommand) => {
            let typed = kiln_platform::command_actions::typed_command(id, cx)?;
            typed.build(&Value::Null).err()
        }
    }
}

/// The window an editor with `focus` is focused in, else one it was drawn
/// in last.
fn editor_window(focus: &FocusHandle, cx: &mut App) -> Option<AnyWindowHandle> {
    let windows = cx.windows();
    let focused = windows.iter().copied().find(|handle| {
        (handle.update(cx, |_, window, _| focus.is_focused(window))).unwrap_or(false)
    });
    focused.or_else(|| {
        windows.into_iter().find(|handle| {
            let drawn = |_, window: &mut Window, cx: &mut App| {
                kiln_platform::focus::is_drawn(focus, window, cx)
            };
            handle.update(cx, drawn).unwrap_or(false)
        })
    })
}

/// Runs `command`, a map's `<Cmd>fe:…<CR>` command, in `window` as the
/// keymap runs a binding's: if `view` is focused there, from the focused
/// element in its stack, stamped live as a keystroke's is. Otherwise in the
/// stack the replay's keys came in (`origin`), `view` running one of its
/// own itself and any other going to the window's focused element. It's
/// checked first ([`EditorView::check_remap_command`]). Returns whether it
/// was dispatched; why not is left as vim's message.
fn run_remap_command(
    view: &Entity<EditorView>,
    command: RemapCommand,
    origin: Option<&ReplayOrigin>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let focused = view.read(cx).focus.is_focused(window);
    let stack = match (focused, origin) {
        (true, _) => crate::keymap::context_stack(&window.context_stack()),
        (false, Some(origin)) => origin.stack.clone(),
        (false, None) => fe_editor::commands::context(),
    };
    // A macro's command runs as the replay's (no shell command from it).
    view.update(cx, |view, _| {
        if let Some(state) = &mut view.vim {
            state.running_remap_command(&command);
        }
    });
    let checked = view.update(cx, |view, cx| view.check_remap_command(command, stack, cx));
    let Some(invocation) = checked else {
        return false;
    };
    if !focused && editor_command(&invocation.command).is_some() {
        return view.update(cx, |view, cx| {
            view.run_command(invocation, Some(window), cx)
        });
    }
    view.update(cx, |view, _| view.command_failed = false);
    let id = invocation.command.clone();
    match kiln_platform::command_actions::dispatch_invocation(invocation, window, cx) {
        Ok(()) => true,
        Err(error) => {
            view.update(cx, |view, cx| {
                if let Some(state) = &mut view.vim {
                    let command = format!("fe:{id}");
                    let message = tr!(
                        "gpui-vim-command-failed",
                        command = command,
                        error = error.to_string()
                    );
                    // An error, which a screen reader is told at once.
                    view.announced.host_message(message.to_string(), true);
                    state.set_host_message(message.to_string());
                }
                cx.notify();
            });
            false
        }
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
    let origin = ReplayOrigin {
        window: window.window_handle(),
        stack: stack.clone(),
    };
    note_origin(editor.entity_id(), origin, cx);
    editor.update(cx, |view, cx| {
        view.sync_vim(cx);
        view.end_stale_host_wait(cx);
        // A key ends vim's message, whether it's bound or fed to the reducer.
        if let Some(state) = view.vim.as_mut().filter(|state| state.message().is_some()) {
            state.clear_message();
            cx.notify();
        }
        // So `cmd-c` copies the visual extent even if no frame was drawn
        // since another view's edit.
        view.sync_visual();
        // A replay held for a map's command owns the keys too, until it's
        // over ([`EditorView::feed_vim`]).
        let pending = view.vim_replaying
            || (view.vim.as_ref()).is_some_and(|state| {
                state.pending().kind().is_some()
                    || state.remap_waiting()
                    || state.insert_register_waiting()
            });
        (view.live_frame(stack, cx), pending)
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
    let ((), transactions) = model.run_as(origin, |model| {
        state.line_break_in(model, host);
        state.restore_indent(model);
    });
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
                Some('\n') => {
                    state.line_break_in(model, host);
                    state.restore_indent(model);
                }
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

/// [`EditorView::key_context_frames`], built.
fn key_context_frames() -> Vec<fe_command::ContextFrame> {
    let typing = [
        crate::completion_menu::KEY_CONTEXT,
        crate::view::ghost_text::KEY_CONTEXT,
    ];
    // `frame` with each subset of `flags`.
    let with_each = |frame: fe_command::ContextFrame, flags: &[&str]| {
        (0..1usize << flags.len())
            .map(|set| {
                (flags.iter().enumerate())
                    .filter(|(bit, _)| set & (1 << bit) != 0)
                    .fold(frame.clone(), |frame, (_, flag)| frame.flag(*flag))
            })
            .collect::<Vec<_>>()
    };
    let editor = fe_command::ContextFrame::new(crate::KEY_CONTEXT);
    let mut frames = with_each(editor.clone(), &[&typing[..], &["proposal"]].concat());
    for editor in with_each(editor.flag(ENABLED), &["proposal"]) {
        for frame in fe_vim::frames(&editor) {
            match frame.get("vim.mode") {
                Some("insert" | "replace") => frames.extend(with_each(frame, &typing)),
                _ => frames.push(frame),
            }
        }
    }
    frames
}

impl EditorView {
    /// The keyword completion vim has under way in this view, for the
    /// completion menu to show.
    pub(crate) fn keyword_completion(&self) -> Option<fe_vim::KeywordCompletion> {
        self.vim.as_ref()?.completion(&self.model)
    }

    /// `stack` with the editor's frame stamped as a keystroke is matched
    /// against it: its vim state, the completion menu and the suggestion as
    /// they are now, not as the last frame drew them, so a key after the
    /// menu closed, or `tab` after the suggestion went, is the editor's.
    fn live_frame(&mut self, stack: ContextStack, cx: &mut Context<Self>) -> ContextStack {
        self.drop_stale_completion();
        self.drop_stale_ghost_text(cx);
        let stack = stamp(stack, self.vim.as_ref());
        let stack = crate::completion_menu::stamp(stack, self.completion_shown());
        crate::view::ghost_text::stamp(stack, self.ghost_text_shown())
    }

    /// Every frame of an editor's a keystroke can be matched in
    /// ([`Self::live_frame`]), for the Keyboard Shortcuts tab to find the
    /// bindings that clash where they both apply: with vim off, with or
    /// without the completion menu, a suggestion and an open hunk's
    /// proposal; with vim on, each frame vim stamps ([`fe_vim::frames`])
    /// with `vim.enabled`, with or without a proposal, and with or without
    /// the menu and a suggestion in insert and replace mode, the only modes
    /// that keep them. Built once: vim's keys don't change while the app
    /// runs.
    pub fn key_context_frames() -> &'static [fe_command::ContextFrame] {
        static FRAMES: std::sync::LazyLock<Vec<fe_command::ContextFrame>> =
            std::sync::LazyLock::new(key_context_frames);
        &FRAMES
    }

    /// The named key contexts fe-gpui's own views seat an editor under, for
    /// the Keyboard Shortcuts tab to find the bindings that clash there: the
    /// diff view. The file viewer and the panes set none, so their editors
    /// sit in the workbench's frame alone; hosts outside fe-gpui (the chat
    /// composer) add theirs.
    pub fn host_key_contexts() -> &'static [&'static str] {
        &[crate::diff_view::KEY_CONTEXT]
    }

    /// Creates the vim state when vim is on and drops it when it's off.
    /// Returns whether that changed anything.
    pub(crate) fn sync_vim(&mut self, cx: &App) -> bool {
        let (enabled, generation) = switch(cx);
        if self.vim_generation == generation {
            return false;
        }
        self.vim_generation = generation;
        let enabled = enabled && !self.vim_off;
        let was_on = self.vim.is_some();
        // A new state's yank serials start again, and its line is closed.
        self.yank_seen = 0;
        self.end_yank_flash();
        self.end_search_preview(true);
        self.vim = enabled.then(|| {
            let mut state = VimState::new();
            if let Some(vim) = cx.try_global::<Vim>() {
                state.set_jump_on_s(vim.jump_on_s);
                state.set_jump_style(vim.jump_style);
                state.set_replace_with_register(vim.replace_with_register);
                state.set_subword_motions(vim.subword_motions);
                state.set_remaps(Arc::clone(&vim.remaps));
                state.set_which_key_groups(Arc::clone(&vim.which_key_groups));
            }
            state
        });
        was_on || enabled
    }

    /// Keeps vim off in this editor whatever the switch says, as an input
    /// such as a chat's composer wants, or has it follow the switch again.
    pub fn set_vim_off(&mut self, off: bool, cx: &mut Context<Self>) {
        if self.vim_off == off {
            return;
        }
        self.vim_off = off;
        // Synced again, as a turn of the switch would.
        self.vim_generation = self.vim_generation.wrapping_sub(1);
        if self.sync_vim(cx) {
            cx.notify();
        }
    }

    /// Whether vim is kept off here ([`EditorView::set_vim_off`]).
    pub fn is_vim_off(&self) -> bool {
        self.vim_off
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

    /// Runs `f` on the vim state and the model, as a host handing back a
    /// command's answer does ([`VimState::apply_filter`]), then shows what
    /// it changed; `None` with vim off.
    pub fn update_vim<R>(
        &mut self,
        f: impl FnOnce(&mut VimState, &mut EditorModel) -> R,
        cx: &mut Context<Self>,
    ) -> Option<R> {
        let state = self.vim.as_mut()?;
        let answer = f(state, &mut self.model);
        self.changed(cx);
        Some(answer)
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

    /// Ends the yank flash if the buffer changed since it began.
    pub(crate) fn sync_yank_flash(&mut self) {
        let version = self.model.snapshot().version();
        if self
            .yank_flash
            .as_ref()
            .is_some_and(|flash| flash.version != version)
        {
            self.end_yank_flash();
        }
    }

    fn end_yank_flash(&mut self) {
        if let Some(flash) = self.yank_flash.take() {
            self.model.remove_decoration_provider(flash.provider);
        }
    }

    /// If vim yanked since the last flash, highlights the yanked ranges,
    /// replacing any flash under way, and ends it after [`YANK_FLASH`]. The
    /// ranges are of the text the command yanked from: one that also edited
    /// (`:normal yiwx`), leaving the buffer other than at `before`, flashes
    /// nothing, as its ranges may no longer be the text yanked.
    fn flash_new_yank(&mut self, before: Version, cx: &mut Context<Self>) {
        let Some((serial, ranges)) = self.vim.as_ref().and_then(VimState::last_yank) else {
            return;
        };
        if serial == self.yank_seen {
            return;
        }
        self.yank_seen = serial;
        let (on, duration) = cx
            .try_global::<Vim>()
            .and_then(|vim| vim.highlight_yank)
            .unwrap_or((true, YANK_FLASH));
        let ranges = ranges.to_vec();
        self.end_yank_flash();
        let snapshot = self.model.snapshot().clone();
        let version = snapshot.version();
        if !on || ranges.is_empty() || version != before {
            return;
        }
        let items = ranges
            .into_iter()
            .map(|range| Decoration::new(range, DecorationKind::Highlight, Tint::Yank))
            .collect();
        let mut set = DecorationSet::new();
        set.apply(&snapshot, version, items);
        let provider = self
            .model
            .add_decoration_provider(Rc::new(RefCell::new(set)));
        let timer = cx.spawn(async move |view, cx| {
            cx.background_executor().timer(duration).await;
            view.update(cx, |view, cx| {
                view.end_yank_flash();
                cx.notify();
            })
            .ok();
        });
        self.yank_flash = Some(YankFlash {
            provider,
            version,
            _timer: timer,
        });
        cx.notify();
    }

    /// While vim has a `/` or `?` line open, shows what submitting it as
    /// `line` would land on now ([`VimState::search_preview`], 'incsearch'):
    /// the match as the current one, painted as the find bar's is, with the
    /// line's pattern's matches in place of any other highlight; a fold
    /// hiding it opened; and from where the view was before the line
    /// opened, scrolled as a caret there would be, leaving the caret where
    /// it is. A line that previews nothing (empty, invalid, not found) puts
    /// back that view, the folds and the highlight. The host's line calls it
    /// after each edit. Cancelling the line puts them back too; submitting
    /// leaves them, the caret landing on the match in view.
    pub fn preview_search(&mut self, line: &str, cx: &mut Context<Self>) {
        self.sync_vim(cx);
        if !matches!(
            self.vim.as_ref().map(VimState::mode),
            Some(fe_vim::Mode::CmdLine(
                CommandLineKind::SearchForward | CommandLineKind::SearchBackward
            ))
        ) {
            return;
        }
        if self.search_preview.is_none() {
            self.search_preview = Some(SearchPreview::from(self.place()));
        }
        self.undo_search_preview();
        self.model.sync_display();
        let Some(state) = &self.vim else {
            return;
        };
        // The pattern the preview found its match by, case and all.
        let unset = Registers::default();
        let registers = vim_registers(cx).unwrap_or(&unset);
        let shown = state
            .search_preview(&self.model, registers, line)
            .and_then(|found| Some((found, state.search_preview_pattern(registers, line)?)));
        if let Some((found, pattern)) = shown {
            let before = self.model.text().clone();
            let events = self.model.buffer_mut().subscribe();
            let opened = self.open_folds_at(found.start);
            let version = self.model.snapshot().version();
            if let Some(preview) = &mut self.search_preview {
                preview.opened = opened;
                preview.opened_in = Some((before, events));
                preview.reveal = Some(found.start);
                preview.shown = Some((found, pattern, version));
                // The frame scrolls to it as it would to the caret, clear of
                // sticky scroll's band.
                self.autoscroll = true;
                self.band_spares_caret = false;
            }
        }
        cx.notify();
    }

    /// Opens the folds hiding `offset`, returning them.
    fn open_folds_at(&mut self, offset: usize) -> Vec<RangeInclusive<u32>> {
        let row = self.model.text().offset_to_point(offset).row;
        if self.model.display().hidden_rows(row).is_none() {
            return Vec::new();
        }
        let folds = |model: &EditorModel| model.display().folds().to_vec();
        let before = folds(&self.model);
        self.model.unfold_rows(row..=row);
        let after = folds(&self.model);
        before
            .into_iter()
            .filter(|fold| !after.contains(fold))
            .map(|fold| fold.start..=fold.end)
            .collect()
    }

    /// The match an open search line's preview shows
    /// ([`EditorView::preview_search`]), in the text as it is: `None` with
    /// none shown, or once the text has changed.
    pub fn search_preview(&self) -> Option<Range<usize>> {
        let (found, _, version) = self.search_preview.as_ref()?.shown.as_ref()?;
        (*version == self.model.snapshot().version()).then(|| found.clone())
    }

    /// The pattern whose matches the view paints: an open search line's
    /// previewed one ([`EditorView::preview_search`]), else the find bar's
    /// ([`EditorView::set_find`]), else the last vim search's ('hlsearch',
    /// [`VimState::highlight`]). `None` with none of them, as after `:noh`.
    pub fn match_pattern(&self) -> Option<&Pattern> {
        self.previewed_pattern()
            .or_else(|| self.find_pattern())
            .or_else(|| self.vim.as_ref()?.highlight())
    }

    /// The pattern of the match [`EditorView::search_preview`] shows.
    pub(crate) fn previewed_pattern(&self) -> Option<&Pattern> {
        let (_, pattern, version) = self.search_preview.as_ref()?.shown.as_ref()?;
        (*version == self.model.snapshot().version()).then_some(pattern)
    }

    /// Where the next frame scrolls to in place of the caret: a preview's
    /// match not yet revealed.
    pub(crate) fn preview_reveal(&self) -> Option<usize> {
        self.search_preview.as_ref()?.reveal
    }

    /// A frame was laid out, revealing the preview's match if it had one to.
    pub(crate) fn preview_revealed(&mut self) {
        if let Some(preview) = &mut self.search_preview {
            preview.reveal = None;
        }
    }

    fn place(&self) -> ViewPlace {
        ViewPlace {
            scroll: self.scroll,
            top: self.top_buffer_row().zip(self.top_display_row()),
            autoscroll: self.autoscroll,
            band: (self.band_spares_caret, self.clear_band),
        }
    }

    /// Whether vim's command line is open.
    fn vim_line_open(&self) -> bool {
        self.vim
            .as_ref()
            .is_some_and(|state| matches!(state.mode(), fe_vim::Mode::CmdLine(_)))
    }

    /// After a vim command ran: starts a search preview if it opened a `/`
    /// or `?` line, from `place`, where the view was before it; and ends
    /// one if it closed the line, a submit leaving what the preview showed
    /// and anything else putting the view and folds back.
    fn settle_search_preview(&mut self, was_open: bool, place: ViewPlace, invocation: &Invocation) {
        let searching = matches!(
            self.vim.as_ref().map(VimState::mode),
            Some(fe_vim::Mode::CmdLine(
                CommandLineKind::SearchForward | CommandLineKind::SearchBackward
            ))
        );
        if searching && !was_open {
            self.search_preview = Some(SearchPreview::from(place));
        } else if self.search_preview.is_some() && !self.vim_line_open() {
            let submitted = invocation.command.as_str() == "vim.command_line_submit";
            self.end_search_preview(!submitted);
        }
    }

    /// Ends the search preview, if any, with `restore` putting back the
    /// view and folds as they were before the line opened.
    fn end_search_preview(&mut self, restore: bool) {
        if restore {
            self.undo_search_preview();
        }
        self.end_substitute_preview();
        self.search_preview = None;
    }

    /// Puts back the folds the search preview opened and the view as it
    /// was before the line opened, showing no match.
    fn undo_search_preview(&mut self) {
        let Some(preview) = &mut self.search_preview else {
            return;
        };
        preview.shown = None;
        preview.reveal = None;
        let substitute = preview.substitute.take();
        let opened = std::mem::take(&mut preview.opened);
        let opened_in = preview.opened_in.take();
        let place = preview.place;
        if let Some((provider, _)) = substitute {
            self.model.remove_decoration_provider(provider);
        }
        // The buffer may have changed since the folds were opened.
        self.model.sync_display();
        let opened = match opened_in {
            Some((before, events)) => follow_fold_edits(opened, &before, &events),
            None => opened,
        };
        for fold in opened {
            self.model.fold_rows(fold);
        }
        self.scroll = place.scroll;
        self.scrolled_top = place.top.map(|(row, _)| row);
        self.scrolled_display_top = place.top.map(|(_, row)| row);
        self.autoscroll = place.autoscroll;
        (self.band_spares_caret, self.clear_band) = place.band;
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

    /// Resolves keys held for a longer map as a timeout would, before a
    /// mouse press measures where it landed: they can edit the text.
    pub(crate) fn resolve_held_prefix(&mut self, cx: &mut Context<Self>) {
        if self.vim.as_ref().is_some_and(VimState::remap_waiting) {
            self.sync_vim(cx);
            self.end_host_wait(cx);
            self.resolve_remap(cx);
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
        self.end_host_wait(cx);
        if self.vim.as_ref().is_some_and(VimState::remap_waiting) {
            self.resolve_remap(cx);
        }
        let Some(state) = &mut self.vim else {
            return;
        };
        if matches!(state.mode(), fe_vim::Mode::Insert | fe_vim::Mode::Replace) {
            self.cancel_register(cx);
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
    ///
    /// A line that took keys through the `c` maps ([`EditorView::remap_line_key`])
    /// has recorded them as typed, not as they expanded. What they typed
    /// there is the line a replay builds too, so only the difference to
    /// `text` is recorded (edits the maps never saw, a `<BS>`, a paste),
    /// then `end`, but for one a map's right side typed.
    pub fn record_command_line(&mut self, text: &str, end: VimKey) {
        let echo = self.vim_line_echo.take();
        let expanded = std::mem::take(&mut self.vim_line_end_expanded);
        let Some(state) = &self.vim else {
            return;
        };
        if !matches!(state.mode(), fe_vim::Mode::CmdLine(_)) {
            return;
        }
        if let Some(echo) = echo {
            let (taken, kept) = grapheme_diff(&echo, text);
            let typed: Vec<VimKey> = (0..taken)
                .map(|_| VimKey::Backspace)
                .chain(literal_keys(text[kept..].chars()))
                .chain((!expanded).then_some(end))
                .collect();
            for key in typed {
                self.record_vim_key(key);
            }
            return;
        }
        let prefill = state.command_line_prefill();
        let (taken, kept) = grapheme_diff(prefill, text);
        let typed: Vec<VimKey> = (0..taken)
            .map(|_| VimKey::Backspace)
            .chain(text_keys(text[kept..].chars()))
            .chain([end])
            .collect();
        for key in typed {
            self.record_vim_key(key);
        }
    }

    /// The line's text changed to `text` by an edit the `c` maps never saw
    /// (a `<BS>`, a paste, an IME commit, a history recall or a completion):
    /// recorded now, its text as literal input ([`LITERAL_NEXT`] before each
    /// character, which a replay reads unmapped), so it comes before the
    /// keys the maps see next. Only once a line goes through the maps; until
    /// then [`EditorView::record_command_line`] records the line whole.
    pub fn sync_line_text(&mut self, text: &str) {
        let Some(state) = &self.vim else {
            return;
        };
        if !matches!(state.mode(), fe_vim::Mode::CmdLine(_))
            || !(self.vim_line_echo.is_some()
                || state.remap_waiting()
                || !state.remaps().is_empty())
        {
            return;
        }
        let echo = self
            .vim_line_echo
            .get_or_insert_with(|| state.command_line_prefill().to_owned());
        let (taken, kept) = grapheme_diff(echo, text);
        let typed: Vec<VimKey> = (0..taken)
            .map(|_| VimKey::Backspace)
            .chain(literal_keys(text[kept..].chars()))
            .collect();
        *echo = text.to_owned();
        for key in typed {
            self.record_vim_key(key);
        }
    }

    /// A new command line opens: what the last one took through the `c`
    /// maps is forgotten.
    pub fn begin_command_line(&mut self) {
        self.vim_line_echo = None;
        self.vim_line_end_expanded = false;
        self.vim_line_literal = false;
        // Keys a continuation left held go on to the line it opened.
        self.vim_line_holds &= self.vim.as_ref().is_some_and(VimState::remap_waiting);
        self.vim_line_selected = false;
        self.vim_line_platform.clear();
    }

    /// A character typed on the command line, asked of the `c` maps
    /// ([`VimState::remap_insert`]). `None`: no map is in play, and it
    /// types as it came. Otherwise what the maps give ([`LineKeys`]): no
    /// keys while the character starts or runs a map, or is held for a
    /// longer one ([`EditorView::remap_line_waiting`]). A map's right side
    /// is matched again, so a recursive map expands. Each key is recorded
    /// as the Insert path records it, so a recording has what was typed.
    pub fn remap_line_key(&mut self, ch: char) -> Option<LineKeys> {
        let state = self.vim.as_ref()?;
        if !matches!(state.mode(), fe_vim::Mode::CmdLine(_))
            || !(state.remap_waiting() || !state.remaps().is_empty())
        {
            return None;
        }
        Some(self.remapped_line_keys(VecDeque::from([VimKey::Char(ch)])))
    }

    /// Whether keys typed on the command line are held for a longer map.
    pub fn remap_line_waiting(&self) -> bool {
        self.vim.as_ref().is_some_and(VimState::remap_waiting)
    }

    /// Ends the wait for the rest of a map on the command line, as
    /// [`CHORD_TIMEOUT`] does ([`VimState::remap_timeout`]): what the held
    /// keys come to, as [`EditorView::remap_line_key`] gives it.
    pub fn remap_line_timeout(&mut self) -> LineKeys {
        let Some(state) = self.vim.as_mut().filter(|state| state.remap_waiting()) else {
            return LineKeys::default();
        };
        state.remap_timeout();
        let rest = state.take_replay();
        self.vim_line_holds = false;
        self.remapped_line_keys(rest.into())
    }

    /// Takes the last grapheme out of `text`, a command line's, as the
    /// host's line's `<BS>` does with the caret at its end, or all of it if
    /// it's `selected` (which the edit ends): whether there was anything.
    pub fn line_backspace(text: &mut String, selected: &mut bool) -> bool {
        use unicode_segmentation::UnicodeSegmentation as _;
        if std::mem::take(selected) && !text.is_empty() {
            text.clear();
            return true;
        }
        let Some((at, _)) = text.grapheme_indices(true).next_back() else {
            return false;
        };
        text.truncate(at);
        true
    }

    /// Goes on matching `keys`, which [`LineKeys::then`] left after a
    /// `<BS>` the host's line took out, against the `c` maps, the line now
    /// holding `text`; as [`EditorView::remap_line_key`] gives them.
    pub fn remap_line_more(&mut self, keys: Vec<VimKey>, text: &str) -> LineKeys {
        self.vim_line_echo = Some(text.to_owned());
        // The `<BS>` didn't end the line.
        self.vim_line_end_expanded = false;
        self.remapped_line_keys(keys.into())
    }

    /// Feeds the keys a map left after the command line ended to the
    /// editor, as the mode it left takes them, or after a `<Cmd>`, with
    /// `line` the text the host's line, still open, shows: they type into
    /// it, and the text they leave it with, once they've run (after the
    /// fastedit or host commands they wait for too), is the host's line's
    /// to show ([`EditorView::take_line_text`]). `handed`: the line's submit
    /// left the host a file or window command (`:w`), which they wait for
    /// as a replay's keys do ([`EditorView::finish_vim_host`]).
    pub fn replay_line_rest(
        &mut self,
        rest: Vec<VimKey>,
        line: Option<String>,
        handed: bool,
        cx: &mut Context<Self>,
    ) {
        if rest.is_empty() {
            return;
        }
        if line.is_some() {
            let id = cx.entity_id();
            let running = LiveLine::Running { closed: false };
            (cx.default_global::<Vim>().live_lines).insert(id, running);
        }
        if handed {
            self.vim_replaying = true;
            self.suspend_for_host(rest.into(), line, None, cx);
            return;
        }
        self.vim_held_marker = None;
        self.replay_from(rest.into(), line, None, cx);
    }

    /// `text`, platform text the host's line is putting in, goes in once
    /// the keys [`EditorView::replay_line_rest`] is given, and every command
    /// they wait for, have run: unmapped, into the line if it's still open,
    /// else as keys of the editor's.
    pub fn line_platform_text(&mut self, text: &str) {
        self.vim_line_platform = text.to_owned();
    }

    /// Whether the text of the host's open line, which the keys after a
    /// map's `<Cmd>` ([`EditorView::replay_line_rest`]) go on from, is
    /// `selected` in its input: their `<BS>` takes it out, and a character replaces it.
    pub fn select_line_text(&mut self, selected: bool) {
        self.vim_line_selected = selected;
    }

    /// The text the keys after a map's `<Cmd>` left the host's open line
    /// with, once they've run and if it's still open
    /// ([`EditorView::replay_line_rest`]), and then `None`: for the host's
    /// line to show. Its echo is that text already, so a recording doesn't
    /// take it as typed.
    pub fn take_line_text(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let id = cx.entity_id();
        let lines = &mut cx.default_global::<Vim>().live_lines;
        let Some(LiveLine::Done(LineContinued::Shows(_))) = lines.get(&id) else {
            return None;
        };
        match lines.remove(&id) {
            Some(LiveLine::Done(LineContinued::Shows(text))) => Some(text),
            _ => None,
        }
    }

    /// Whether the keys after a map's `<Cmd>` ended the host's open line,
    /// once they've run, and then `false`: the host's line is over, and it
    /// follows the line they may have opened since
    /// ([`VimState::command_line_prefill`] its text).
    pub fn take_line_ended(&mut self, cx: &mut Context<Self>) -> bool {
        let id = cx.entity_id();
        let lines = &mut cx.default_global::<Vim>().live_lines;
        let ended = lines.get(&id) == Some(&LiveLine::Done(LineContinued::Ended));
        if ended {
            lines.remove(&id);
        }
        ended
    }

    /// Whether the keys after a map's `<Cmd>` on the host's open line wait
    /// for a command they ran: keys typed there wait for them.
    pub fn line_continuing(&self, cx: &Context<Self>) -> bool {
        (cx.try_global::<Vim>()).is_some_and(|vim| {
            matches!(
                vim.live_lines.get(&cx.entity_id()),
                Some(LiveLine::Running { .. })
            )
        })
    }

    /// A replay that went on from the host's open line is over, or ends,
    /// leaving it `line` if it's still open: what it did is the host's to
    /// show; if they closed it and opened another, `line` is that one's,
    /// which its prefill holds for the host's line to open with, whether
    /// they're over or failed. Returns whether the replay went on from the
    /// host's line.
    fn end_live_line(&mut self, line: Option<&str>, cx: &mut Context<Self>) -> bool {
        let id = cx.entity_id();
        let open =
            (self.vim.as_ref()).is_some_and(|s| matches!(s.mode(), fe_vim::Mode::CmdLine(_)));
        let lines = &mut cx.default_global::<Vim>().live_lines;
        match lines.get(&id) {
            Some(LiveLine::Running { closed: false }) => {}
            Some(LiveLine::Running { closed: true }) => {
                lines.insert(id, LiveLine::Done(LineContinued::Ended));
                if let (Some(text), Some(state)) = (line.filter(|_| open), &mut self.vim) {
                    state.set_command_line_prefill(text);
                    // Keys the tail left held are the new line's to resolve.
                    self.vim_line_holds = state.remap_waiting();
                }
                return true;
            }
            _ => return false,
        }
        let done = match line.filter(|_| open) {
            Some(text) => {
                self.vim_line_echo = Some(text.to_owned());
                // Keys the tail left held are the host's line's to resolve,
                // against the input: they're typed there.
                self.vim_line_holds = (self.vim.as_ref()).is_some_and(VimState::remap_waiting);
                LineContinued::Shows(text.to_owned())
            }
            None => LineContinued::Ended,
        };
        lines.insert(id, LiveLine::Done(done));
        true
    }

    /// The command line a replay's keys typed into is closed: if it was the
    /// host's open line, a line they open next is one of their own. The
    /// host's line hears of it once they're over, with that line's text.
    fn live_line_closed(&mut self, cx: &mut Context<Self>) {
        let id = cx.entity_id();
        if let Some(LiveLine::Running { closed }) =
            cx.default_global::<Vim>().live_lines.get_mut(&id)
        {
            *closed = true;
        }
    }

    /// Feeds `queue` through the `c` maps, and what a map or a held prefix
    /// leaves after it, until nothing is left to match or a key ends the
    /// line: the keys after it, and a `<Cmd>`, are the line's `rest`.
    fn remapped_line_keys(&mut self, mut queue: VecDeque<VimKey>) -> LineKeys {
        let mut line = LineKeys::default();
        let Some(state) = &mut self.vim else {
            return line;
        };
        let echo = self
            .vim_line_echo
            .get_or_insert_with(|| state.command_line_prefill().to_owned());
        'keys: while let Some(key) = queue.pop_front() {
            if state.remap_command_next() {
                queue.push_front(key);
                break;
            }
            let keys = match state.remap_insert(key) {
                RemapInsert::Wait => Vec::new(),
                RemapInsert::Keys(keys) => keys,
            };
            let mut keys = VecDeque::from(keys);
            while let Some(key) = keys.pop_front() {
                state.record_key(key);
                let expanded = state.expanding();
                line.typed.push((key, expanded));
                match key {
                    VimKey::Char(ch) => echo.push(ch),
                    // What it takes out, or whether it cancels, is the
                    // line's to say: the keys after it wait for that.
                    VimKey::Backspace => {
                        Self::line_backspace(echo, &mut false);
                        self.vim_line_end_expanded |= expanded;
                        keys.extend(state.take_replay());
                        keys.extend(queue.drain(..));
                        line.then = Some(keys.into());
                        return line;
                    }
                    VimKey::Enter | VimKey::Ctrl('j' | 'c') | VimKey::Esc => {
                        self.vim_line_end_expanded |= expanded;
                        // The rest is the editor's, in the mode this leaves.
                        keys.extend(state.take_replay());
                        keys.extend(queue.drain(..));
                        line.rest = keys.into();
                        break 'keys;
                    }
                    _ => {}
                }
            }
            // After the keys that came out, as the Insert path feeds them.
            let mut rest: VecDeque<VimKey> = state.take_replay().into();
            rest.extend(queue.drain(..));
            queue = rest;
            if state.failed() {
                state.stop_replay();
                queue.clear();
                break;
            }
        }
        line.rest.extend(queue);
        line
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
    /// resolves to run before it returns, up to a `<Cmd>fe:…<CR>`, whose
    /// command and the keys after it run once the keystroke is over
    /// ([`EditorView::run_remap_command_later`]), and if it leaves keys held
    /// for a longer map, they wait [`CHORD_TIMEOUT`] for the next. While a
    /// replay holds for such a command, a keystroke waits, in any mode, to
    /// go on as typed once the map is over
    /// ([`EditorView::release_held_keys`]): it can't come between the map's
    /// keys, nor see the context the command runs in change. A prefix it
    /// leaves waiting shows the which-key popup after a delay
    /// ([`EditorView::which_key_after_key`]).
    fn feed_vim(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let taken = self.feed_vim_key(keystroke, window, cx);
        self.which_key_after_key(window, cx);
        taken
    }

    /// [`EditorView::feed_vim`] but for the which-key popup.
    fn feed_vim_key(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.vim_replaying && !self.end_stale_host_wait(cx) {
            let held = (cx.entity_id(), keystroke.clone());
            cx.default_global::<Vim>().held_keys.push(held);
            return true;
        }
        self.sync_vim(cx);
        self.drop_remap_wait(cx);
        if self.accepts_text() {
            // The key after `<C-r>` names the register, whatever it is; one
            // vim has no name for cancels the prefix and goes on.
            if self.register_waiting() {
                return match vim_key(keystroke) {
                    Some(key) => {
                        self.record_vim_key(key);
                        self.register_typing(Some(key), cx)
                    }
                    None => {
                        // A key vim can't name cancels it as `<Esc>` does,
                        // which a macro records, so replay cancels too.
                        self.record_vim_key(VimKey::Esc);
                        self.cancel_register(cx);
                        false
                    }
                };
            }
            if let Some(taken) = self.remap_typed(keystroke, window, cx) {
                return taken;
            }
            // `ctrl-t`, `ctrl-d` and `ctrl-o` are vim's in insert and
            // replace mode, and so are the other ctrl keys it takes.
            return match vim_key(keystroke) {
                Some(key @ VimKey::Ctrl('r')) if self.typing_mode().is_some() => {
                    self.record_vim_key(key);
                    self.register_typing(Some(key), cx)
                }
                Some(key @ VimKey::Ctrl('a')) if self.typing_mode() == Some(false) => {
                    self.record_vim_key(key);
                    self.register_typing(Some(key), cx)
                }
                Some(key @ VimKey::Ctrl('t' | 'd')) if self.typing_mode().is_some() => {
                    self.record_vim_key(key);
                    self.indent_typing(key, cx)
                }
                Some(key @ VimKey::Ctrl('o')) if self.typing_mode().is_some() => {
                    self.record_vim_key(key);
                    self.one_command_typing(cx)
                }
                // `<C-e>` and `<C-y>` are vim's only while completing.
                Some(key @ VimKey::Ctrl('n' | 'p')) | Some(key @ VimKey::Ctrl('e' | 'y'))
                    if self.typing_mode().is_some()
                        && (matches!(key, VimKey::Ctrl('n' | 'p'))
                            || self.keyword_completion().is_some()) =>
                {
                    self.record_vim_key(key);
                    self.complete_typing(key, cx)
                }
                Some(key @ VimKey::Ctrl(_)) => self.insert_key_typing(key, cx),
                _ => false,
            };
        }
        let Some(state) = &mut self.vim else {
            return false;
        };
        let taken = match vim_key(keystroke) {
            // A key vim has no name for cancels a pending command, or keys
            // held for a map, and otherwise goes on, to do nothing.
            None if state.pending().kind().is_none() && !state.remap_waiting() => false,
            // After `ctrl-o`, cancelling it ends the command, and insert
            // mode resumes.
            None => {
                if let Step::Commit(invocation) = state.feed(VimKey::Esc) {
                    let stack = crate::keymap::context_stack(&window.context_stack());
                    let invocation = invocation.context(stamp(stack, self.vim.as_ref()));
                    self.run_vim(&invocation, cx);
                }
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
                    // What it replays (a `@`) comes in here, below.
                    self.run_vim_once(&invocation, cx);
                    cx.notify();
                    true
                }
            },
        };
        let origin = ReplayOrigin::of(window);
        note_origin(cx.entity_id(), origin.clone(), cx);
        self.replay_vim_from(Some(origin), cx);
        self.wait_for_remap(window, cx);
        taken
    }

    /// A keystroke typed in insert or replace mode, asked of the `i` maps
    /// ([`VimState::remap_insert`]) when it's a plain character or a ctrl
    /// key, or any key while keys are held for a longer map. `None`: it
    /// goes on as typed, to the IME path or the ctrl key's own, which
    /// record it. Otherwise `Some(true)`, the editor having taken it: it
    /// starts or completes a map, whose keys are fed once the keystroke is
    /// over ([`EditorView::replay_vim_from`]), or it broke a held prefix,
    /// whose first key is typed here and the rest fed. A composition in
    /// progress is never matched, nor is a multi-character commit, which
    /// comes in by the IME and not as a keystroke.
    fn remap_typed(
        &mut self,
        keystroke: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<bool> {
        let waiting = self.vim.as_ref().is_some_and(VimState::remap_waiting);
        if self.typing_mode().is_none() || self.composition().is_some() {
            return None;
        }
        let key = match vim_key(keystroke) {
            Some(key @ (VimKey::Char(_) | VimKey::Ctrl(_))) => key,
            Some(key) if waiting => key,
            // A key vim has no name for breaks the match: what's held goes
            // on as typed.
            None if waiting => {
                self.resolve_remap(cx);
                return None;
            }
            _ => return None,
        };
        let state = self.vim.as_mut()?;
        let origin = ReplayOrigin::of(window);
        note_origin(cx.entity_id(), origin.clone(), cx);
        match state.remap_insert(key) {
            RemapInsert::Wait => {}
            RemapInsert::Keys(keys) => {
                // With nothing held the key goes on as typed. The tag of
                // each key typed here is read before the rest is taken,
                // which puts its own tags in front.
                if !waiting && keys == [key] {
                    return None;
                }
                self.vim_replaying = true;
                for key in keys {
                    self.type_replayed_key(key, cx);
                }
                self.vim_replaying = false;
                let rest = self.vim.as_mut()?.take_replay();
                if !rest.is_empty() {
                    self.replay_from(rest.into(), None, Some(origin.clone()), cx);
                }
            }
        }
        self.replay_vim_from(Some(origin), cx);
        self.wait_for_remap(window, cx);
        cx.notify();
        Some(true)
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
            .map(|wait| (wait.editor.clone(), wait.origin.clone()));
        if let Some((other, origin)) = other {
            (other.update(cx, |view, cx| view.resolve_remap_from(Some(origin), cx))).ok();
        }
        let timer = cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(CHORD_TIMEOUT).await;
            view.update_in(cx, |view, window, cx| view.remap_timed_out(window, cx))
                .ok();
        });
        let blur = cx.on_blur(&self.focus, window, Self::remap_timed_out);
        let wait = RemapWait {
            editor: cx.weak_entity(),
            origin: ReplayOrigin::of(window),
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
    /// or a click. With no window focusing it, what it holds resolves, as
    /// keys that came in at `origin`.
    fn wait_for_remap_later(&self, origin: Option<ReplayOrigin>, cx: &mut Context<Self>) {
        // A prefix the replay left waits for its which-key popup too.
        self.which_key_later(cx);
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
            // Keys held on the host's line wait there.
            let on_line = editor.vim_line_holds && editor.vim_in_line();
            if waits || on_line || !editor.vim.as_ref().is_some_and(VimState::remap_waiting) {
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
                view.update(cx, |view, cx| view.resolve_remap_from(origin, cx));
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
    /// ([`VimState::remap_timeout`]) and runs what they resolve to, from
    /// where the keys came in: after a blur too.
    fn resolve_remap(&mut self, cx: &mut Context<Self>) {
        let origin = self.drop_remap_wait(cx);
        self.resolve_remap_from(origin, cx);
    }

    /// [`EditorView::resolve_remap`] of keys that came in at `origin`, if
    /// known, its wait already dropped.
    fn resolve_remap_from(&mut self, origin: Option<ReplayOrigin>, cx: &mut Context<Self>) {
        let dropped = self.drop_remap_wait(cx);
        let Some(state) = self.vim.as_mut().filter(|state| state.remap_waiting()) else {
            return;
        };
        state.remap_timeout();
        self.replay_vim_from(origin.or(dropped), cx);
        self.settle_which_key();
        self.which_key_later(cx);
        cx.notify();
    }

    /// Drops this editor's wait for the rest of a map, if it has one,
    /// returning where its keys came in.
    fn drop_remap_wait(&self, cx: &mut Context<Self>) -> Option<ReplayOrigin> {
        let id = cx.entity_id();
        let vim = cx.try_global::<Vim>()?;
        if !(vim.remap_wait.as_ref()).is_some_and(|wait| wait.editor.entity_id() == id) {
            return None;
        }
        let dropped = cx.global_mut::<Vim>().remap_wait.take()?;
        Some(dropped.origin.clone())
    }

    /// `<C-t>` or `<C-d>` (`key`) in insert or replace mode: the carets'
    /// rows' indent shifts ([`VimState::indent_in`], [`VimState::indent_out`]),
    /// in the session. Returns whether it ran.
    fn indent_typing(&mut self, key: VimKey, cx: &mut Context<Self>) -> bool {
        if self.typing_mode().is_none() {
            return false;
        }
        if self.read_only {
            self.refuse_edit(cx);
            return true;
        }
        let invocation = Invocation::of(KEY)
            .expect("a valid command ID")
            .source(Source::Keyboard);
        let origin = fe_editor::commands::origin(&invocation.actor);
        let Some(state) = &self.vim else {
            return false;
        };
        let ((), transactions) = self.model.run_as(origin, |model| match key {
            VimKey::Ctrl('t') => state.indent_in(model),
            _ => state.indent_out(model),
        });
        let result: Result<_, DispatchError> =
            Ok(Dispatched::new(Effect::Changed).with_transactions(transactions));
        self.dispatched(result, &invocation, true, cx).is_ok()
    }

    /// `<C-n>`, `<C-p>`, `<C-e>` or `<C-y>` (`key`) in insert mode: the
    /// keyword before the caret completes, or the completion under way
    /// goes back or is kept ([`VimState::complete_next`] and the others).
    /// Returns whether it ran.
    fn complete_typing(&mut self, key: VimKey, cx: &mut Context<Self>) -> bool {
        if self.typing_mode().is_none() {
            return false;
        }
        if self.read_only {
            self.refuse_edit(cx);
            return true;
        }
        let invocation = Invocation::of(KEY)
            .expect("a valid command ID")
            .source(Source::Keyboard);
        let origin = fe_editor::commands::origin(&invocation.actor);
        let Some(state) = &mut self.vim else {
            return false;
        };
        let (ran, transactions) = self.model.run_as(origin, |model| match key {
            VimKey::Ctrl('n') => {
                state.complete_next(model);
                true
            }
            VimKey::Ctrl('p') => {
                state.complete_previous(model);
                true
            }
            VimKey::Ctrl('e') => state.complete_cancel(model),
            _ => state.complete_accept(model),
        });
        if !ran {
            return false;
        }
        let result: Result<_, DispatchError> =
            Ok(Dispatched::new(Effect::Changed).with_transactions(transactions));
        self.dispatched(result, &invocation, true, cx).is_ok()
    }

    /// Adds the caret to vim's jumplist and sets the jump mark (`''`) there,
    /// as `m'` does, before a jump the view makes itself (a click on a
    /// sticky scroll row); in insert or replace mode through `<C-o>`, so the
    /// session goes on. Nothing with vim off.
    pub(crate) fn record_vim_jump(&mut self, cx: &mut Context<Self>) {
        self.sync_vim(cx);
        if self.vim.is_none() || (self.typing_mode().is_some() && !self.one_command_typing(cx)) {
            return;
        }
        let args = fe_command::Value::from_iter([("name", "'")]);
        let invocation = Invocation::of("vim.set_mark")
            .expect("a valid command ID")
            .args(args)
            .source(Source::Mouse);
        self.run_command(invocation, None, cx);
    }

    /// `<C-o>` in insert or replace mode: normal mode for one command
    /// ([`VimState::one_command`]), the caret a block meanwhile, then back
    /// to the mode it came from once `fe-vim`'s registry runs the command
    /// ([`fe_vim::dispatch`]). Returns whether it ran.
    fn one_command_typing(&mut self, cx: &mut Context<Self>) -> bool {
        if self.typing_mode().is_none() {
            return false;
        }
        let invocation = Invocation::of(KEY)
            .expect("a valid command ID")
            .source(Source::Keyboard);
        let origin = fe_editor::commands::origin(&invocation.actor);
        let Some(state) = &mut self.vim else {
            return false;
        };
        let ((), transactions) = self.model.run_as(origin, |model| state.one_command(model));
        let result: Result<_, DispatchError> =
            Ok(Dispatched::new(Effect::Changed).with_transactions(transactions));
        self.dispatched(result, &invocation, true, cx).is_ok()
    }

    /// Whether insert mode waits for the register name after `<C-r>`.
    fn register_waiting(&self) -> bool {
        self.vim
            .as_ref()
            .is_some_and(VimState::insert_register_waiting)
    }

    /// Drops a pending `<C-r>` once its editor loses focus, however the
    /// prefix began (typed, or replayed by a macro or map), or at once if
    /// the editor isn't focused. One editor at a time, as only the focused
    /// one takes keys.
    fn cancel_register_on_blur(&self, cx: &mut Context<Self>) {
        let view = cx.entity();
        cx.defer(move |cx| {
            let focus = view.read(cx).focus.clone();
            let Some(window) = editor_window(&focus, cx) else {
                view.update(cx, |view, cx| view.cancel_register(cx));
                return;
            };
            window
                .update(cx, |_, window, cx| {
                    if !focus.is_focused(window) {
                        view.update(cx, |view, cx| view.cancel_register(cx));
                        return;
                    }
                    let blur = view.update(cx, |view, cx| {
                        cx.on_blur(&view.focus, window, |view, _, cx| view.cancel_register(cx))
                    });
                    cx.default_global::<Vim>().register_blur = Some(blur);
                })
                .ok();
        });
    }

    /// Drops a pending `<C-r>`, as an `<Esc>` after it does.
    pub(crate) fn cancel_register(&mut self, cx: &mut Context<Self>) {
        if self.register_waiting() {
            self.register_typing(Some(VimKey::Esc), cx);
        }
    }

    /// `<C-r>` or `<C-a>` in insert or replace mode, or the key after
    /// `<C-r>`: to fe-vim's register insert ([`VimState::insert_register_start`],
    /// [`VimState::insert_register_key`], [`VimState::insert_last_inserted`]),
    /// in the session. Returns whether it ran.
    fn register_typing(&mut self, key: Option<VimKey>, cx: &mut Context<Self>) -> bool {
        if self.typing_mode().is_none() {
            return false;
        }
        let waiting = self.register_waiting();
        if self.read_only && !waiting {
            self.refuse_edit(cx);
            return true;
        }
        if !waiting && key == Some(VimKey::Ctrl('r')) {
            if let Some(state) = &mut self.vim {
                state.insert_register_start();
            }
            self.cancel_register_on_blur(cx);
            return true;
        }
        let invocation = Invocation::of(KEY)
            .expect("a valid command ID")
            .source(Source::Keyboard);
        let origin = fe_editor::commands::origin(&invocation.actor);
        let page_rows = self.page_rows();
        let syntax = synced_syntax(&self.syntax, &self.model);
        let language = loaded_language(&self.syntax);
        let server_folds = self.server_fold_ranges();
        let Some(state) = &mut self.vim else {
            return false;
        };
        // Out of the global while the host holds `cx`, and back after.
        let mut registers = std::mem::take(&mut cx.default_global::<Vim>().registers);
        let mut host = ViewHost {
            cx,
            page_rows,
            syntax,
            server_folds,
            language,
            vim: true,
        };
        let ((), transactions) = self.model.run_as(origin, |model| match key {
            Some(key) if waiting => {
                state.insert_register_key(model, &mut host, &mut registers, key)
            }
            _ => state.insert_last_inserted(model, &mut host, &mut registers),
        });
        cx.default_global::<Vim>().registers = registers;
        let result: Result<_, DispatchError> =
            Ok(Dispatched::new(Effect::Changed).with_transactions(transactions));
        self.dispatched(result, &invocation, true, cx).is_ok()
    }

    /// A ctrl key in insert or replace mode, one the keymap left or bound to
    /// `vim.key`: to [`VimState::insert_key`] (`<C-w>`, `<C-u>`), in the
    /// session, and recorded. Returns whether vim took it; it drops any
    /// other.
    fn insert_key_typing(&mut self, key: VimKey, cx: &mut Context<Self>) -> bool {
        let takes = |state: &VimState| state.takes_insert_key(key);
        if self.typing_mode().is_none() || !self.vim.as_ref().is_some_and(takes) {
            return false;
        }
        self.record_vim_key(key);
        if self.read_only {
            self.refuse_edit(cx);
            return true;
        }
        let invocation = Invocation::of(KEY)
            .expect("a valid command ID")
            .source(Source::Keyboard);
        let origin = fe_editor::commands::origin(&invocation.actor);
        let page_rows = self.page_rows();
        let syntax = synced_syntax(&self.syntax, &self.model);
        let language = loaded_language(&self.syntax);
        let server_folds = self.server_fold_ranges();
        let Some(state) = &self.vim else {
            return false;
        };
        let host = ViewHost {
            cx,
            page_rows,
            syntax,
            server_folds,
            language,
            vim: true,
        };
        let (_, transactions) = self
            .model
            .run_as(origin, |model| state.insert_key(model, &host, key));
        let result: Result<_, DispatchError> =
            Ok(Dispatched::new(Effect::Changed).with_transactions(transactions));
        self.dispatched(result, &invocation, true, cx).is_ok()
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
    /// `<Right>` in it isn't followed, and the caret starts at the end. The
    /// keys after a map's `<Cmd>` on the host's open line go on from its
    /// text, and the host's line shows what they leave
    /// ([`EditorView::replay_line_rest`]).
    ///
    /// A fastedit command a key leaves (a map's `<Cmd>fe:…<CR>`, or one a
    /// `:normal` ran) holds the replay there, and it goes on once the
    /// command has run, or stops if it can't
    /// ([`EditorView::run_remap_command_later`]).
    fn replay_vim(&mut self, cx: &mut Context<Self>) {
        self.replay_vim_from(None, cx);
    }

    /// [`EditorView::replay_vim`] of keys that came in at `origin`: a
    /// keystroke's, or the keys a wait held; or, not known, where the
    /// editor's last keystroke came in.
    fn replay_vim_from(&mut self, origin: Option<ReplayOrigin>, cx: &mut Context<Self>) {
        if self.vim_replaying {
            // A key of the replay under way ran this, or the command it
            // holds for: the loop takes what it left.
            return;
        }
        let Some(state) = &mut self.vim else {
            return;
        };
        let queue: VecDeque<VimKey> = state.take_replay().into();
        if queue.is_empty() && !state.has_remap_command() {
            return;
        }
        let origin = origin.or_else(|| last_origin(cx.entity_id(), cx));
        self.vim_held_marker = None;
        self.replay_from(queue, None, origin, cx);
    }

    /// [`EditorView::replay_vim`]'s loop, from `queue`, with `line` the
    /// hidden command line's text, the keys from `origin`.
    fn replay_from(
        &mut self,
        mut queue: VecDeque<VimKey>,
        mut line: Option<String>,
        origin: Option<ReplayOrigin>,
        cx: &mut Context<Self>,
    ) {
        self.vim_replaying = true;
        let mut handed = None;
        // A key of the line's own that held keys resolved before, and how
        // many keys come after it: it comes once what they resolved to has
        // run, if the line is still open, as the host's line drops it if
        // they closed it.
        let mut held_back: Option<(VimKey, usize)> = None;
        // The line it was typed on closed since: a line open after is
        // another's, and a suspension (which queues the key) keeps this.
        let mut line_gone = false;
        if let Some((after, gone)) = self.vim_held_marker.take()
            && let Some(at) = queue.len().checked_sub(after + 1)
            && let Some(key) = queue.remove(at)
        {
            held_back = Some((key, after));
            line_gone = gone;
        }
        let mut drained = false;
        loop {
            if held_back.is_some()
                && !self
                    .vim
                    .as_ref()
                    .is_some_and(|s| matches!(s.mode(), fe_vim::Mode::CmdLine(_)))
            {
                line_gone = true;
            }
            if let Some((key, after)) = held_back
                && queue.len() <= after
            {
                held_back = None;
                let Some(state) = &mut self.vim else {
                    break;
                };
                match state.mode() {
                    fe_vim::Mode::CmdLine(_) if !line_gone => queue.push_front(key),
                    // Read but not run: its place in the replay goes with
                    // it, so the next key isn't taken for the replay's.
                    _ => state.record_key(key),
                }
            }
            if let Some(command) = self.vim.as_mut().and_then(VimState::take_remap_command) {
                self.mark_held_back(&mut queue, held_back, line_gone);
                return self.run_remap_command_later(command, queue, line, origin, cx);
            }
            let Some(key) = queue.pop_front() else {
                drained = true;
                break;
            };
            if self.unmapped_after_held(key) {
                // The held keys resolve as a timeout resolves them, then
                // the key comes, which no map takes.
                let Some(state) = &mut self.vim else {
                    break;
                };
                state.remap_timeout();
                if state.failed() {
                    // Their maps recursed (E223): the replay stops there,
                    // as at a key that fails, the message kept.
                    self.stop_map(true, None);
                    break;
                }
                held_back = Some((key, queue.len()));
                line_gone = false;
                for key in state.take_replay().into_iter().rev() {
                    queue.push_front(key);
                }
                continue;
            }
            let ok = self.replay_key(key, &mut line, &mut handed, cx);
            if line.is_none() {
                self.live_line_closed(cx);
            }
            if !ok {
                if held_back.is_some() && !self.vim_in_line() {
                    line_gone = true;
                }
                put_back(&mut queue, held_back);
                // The host runs a file or window command after this update:
                // what the map does after it waits for the host's report.
                if handed.is_some() {
                    let mark = held_back.map(|(_, after)| (after, line_gone));
                    if self.wait_for_host(&mut queue, &mut line, &origin, mark, cx) {
                        return;
                    }
                }
                self.stop_map(!queue.is_empty(), handed);
                break;
            }
            let Some(state) = &mut self.vim else {
                break;
            };
            for key in state.take_replay().into_iter().rev() {
                queue.push_front(key);
            }
        }
        // The host's platform text goes in after all the keys. A map that
        // failed or stopped drops it.
        let platform = std::mem::take(&mut self.vim_line_platform);
        if drained && !platform.is_empty() {
            let open = (self.vim.as_ref())
                .is_some_and(|state| matches!(state.mode(), fe_vim::Mode::CmdLine(_)));
            match line.as_mut().filter(|_| open) {
                Some(text) => {
                    if std::mem::take(&mut self.vim_line_selected) {
                        text.clear();
                    }
                    text.push_str(&platform);
                }
                None => {
                    let keys = platform.chars().map(VimKey::Char).collect();
                    return self.replay_from(keys, line, origin, cx);
                }
            }
        }
        self.vim_replaying = false;
        self.release_held_keys(cx);
        self.wait_for_remap_later(origin, cx);
        if self.end_live_line(line.as_deref(), cx) {
            return cx.notify();
        }
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

    /// Whether the editor is in a command line.
    fn vim_in_line(&self) -> bool {
        (self.vim.as_ref()).is_some_and(|s| matches!(s.mode(), fe_vim::Mode::CmdLine(_)))
    }

    /// The replay suspends with `held_back` in `queue`, back where it was:
    /// kept to be held again once it goes on ([`EditorView::replay_from`]),
    /// with whether its line has closed since.
    fn mark_held_back(
        &mut self,
        queue: &mut VecDeque<VimKey>,
        held_back: Option<(VimKey, usize)>,
        line_gone: bool,
    ) {
        put_back(queue, held_back);
        let gone = line_gone || !self.vim_in_line();
        self.vim_held_marker = held_back.map(|(_, after)| (after, gone));
    }

    /// Suspends the replay at the file or window command the last key left
    /// for the host, if keys follow it: they wait in `queue` and `line`
    /// until the host reports ([`EditorView::finish_vim_host`]), which
    /// resumes them. Returns whether it did; if not, the replay ends. The
    /// editor notifies for the host to take the command; one still there
    /// once the update is over was taken by no one, and the map ends.
    fn wait_for_host(
        &mut self,
        queue: &mut VecDeque<VimKey>,
        line: &mut Option<String>,
        origin: &Option<ReplayOrigin>,
        held: Option<(usize, bool)>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = self.vim.as_mut().filter(|state| state.has_host_command()) else {
            return false;
        };
        for key in state.take_replay().into_iter().rev() {
            queue.push_front(key);
        }
        if queue.is_empty() && !state.has_remap_command() {
            return false;
        }
        self.vim_held_marker = held;
        let token = self.suspend_for_host(std::mem::take(queue), line.take(), origin.clone(), cx);
        let view = cx.weak_entity();
        cx.defer(move |cx| {
            (view.update(cx, |view, cx| view.host_untaken(token, cx))).ok();
        });
        true
    }

    /// Suspends the replay, `queue` and `line` left of it, until the host
    /// reports on the file or window command it has or takes
    /// ([`EditorView::finish_vim_host`]). Returns the wait's token.
    fn suspend_for_host(
        &mut self,
        queue: VecDeque<VimKey>,
        line: Option<String>,
        origin: Option<ReplayOrigin>,
        cx: &mut Context<Self>,
    ) -> u64 {
        // The host's answer is what comes after.
        if let Some(state) = &mut self.vim {
            state.clear_message();
        }
        let id = cx.entity_id();
        let generation = switch(cx).1;
        let vim = cx.default_global::<Vim>();
        vim.host_token += 1;
        let token = vim.host_token;
        let wait = HostWait {
            token,
            generation,
            queue,
            line,
            origin,
            claimed: false,
            reported: false,
        };
        vim.host_waits.insert(id, wait);
        cx.notify();
        token
    }

    /// The replay suspended as `token` ends: its host command is still the
    /// editor's, so no host took it.
    fn host_untaken(&mut self, token: u64, cx: &mut Context<Self>) {
        let untaken = self.vim.as_ref().is_some_and(VimState::has_host_command);
        if !untaken {
            return;
        }
        let Some(wait) = self.take_host_wait(token, cx) else {
            return;
        };
        self.vim_replaying = false;
        self.end_live_line(wait.line.as_deref(), cx);
        self.stop_map(true, Some(handed_host_command()));
        self.release_held_keys(cx);
        self.wait_for_remap_later(wait.origin, cx);
        cx.notify();
    }

    /// This editor's suspended replay, if it's `token`'s.
    fn take_host_wait(&self, token: u64, cx: &mut Context<Self>) -> Option<HostWait> {
        let id = cx.entity_id();
        let waits = &mut cx.default_global::<Vim>().host_waits;
        match waits.get(&id) {
            Some(wait) if wait.token == token => waits.remove(&id),
            _ => None,
        }
    }

    /// Ends this editor's suspended replay, if it has one, dropping the
    /// rest of the map: a click, say, which the map's keys can't come
    /// before.
    fn end_host_wait(&mut self, cx: &mut Context<Self>) {
        let id = cx.entity_id();
        let ended = cx.default_global::<Vim>().host_waits.remove(&id);
        let Some(wait) = ended else {
            return;
        };
        self.vim_replaying = false;
        self.end_live_line(wait.line.as_deref(), cx);
        self.stop_map(true, None);
        self.release_held_keys(cx);
        self.wait_for_remap_later(wait.origin, cx);
        cx.notify();
    }

    /// Ends this editor's suspended replay if vim's state was reset since,
    /// by being switched off and on or the like: what it holds is of a
    /// state that's gone. Returns whether it did.
    pub(crate) fn end_stale_host_wait(&mut self, cx: &mut Context<Self>) -> bool {
        let id = cx.entity_id();
        let generation = switch(cx).1;
        let stale = (cx.default_global::<Vim>().host_waits.get(&id))
            .is_some_and(|wait| wait.generation != generation);
        if stale {
            self.end_host_wait(cx);
        }
        stale
    }

    /// Drops the rest of a replay, `rest` whether keys were left of it, and
    /// says `handed` of them if any were.
    fn stop_map(&mut self, rest: bool, handed: Option<SharedString>) {
        self.vim_held_marker = None;
        let Some(state) = &mut self.vim else {
            return;
        };
        let rest = rest || !state.take_replay().is_empty() || state.has_remap_command();
        state.stop_replay();
        if let Some(note) = handed
            && rest
        {
            log::warn!("{note}");
            state.set_host_message(note.to_string());
        }
    }

    /// `command`, a map's `<Cmd>fe:…<CR>` command, from the keyboard in
    /// `stack` with the editor's frame stamped live
    /// ([`EditorView::live_frame`]), if it can run there; if not, `None`,
    /// and vim's message says why: `E492: Not an editor command: fe:…`, the
    /// args it needs, a context that doesn't hold, or read-only's E21.
    fn check_remap_command(
        &mut self,
        command: RemapCommand,
        stack: ContextStack,
        cx: &mut Context<Self>,
    ) -> Option<Invocation> {
        let RemapCommand(invocation) = command;
        let context = self.live_frame(stack, cx);
        let invocation = invocation.source(Source::Keyboard).context(context);
        let id = invocation.command.clone();
        let meta = editor_command(&id);
        let message = if !known_command(&id, cx) {
            fe_vim::messages::not_an_editor_command(&format!("fe:{id}"))
        } else if let Some(message) = needs_args(&id, cx) {
            let command = format!("fe:{id}");
            tr!(
                "gpui-vim-command-failed",
                command = command,
                error = message
            )
            .to_string()
        } else if (meta.as_ref().and_then(|meta| meta.context.as_ref()))
            .is_some_and(|context| !context.eval(&invocation.context))
        {
            let command = format!("fe:{id}");
            tr!("gpui-vim-command-not-here", command = command).to_string()
        } else {
            let refused = match (&meta, registry().get(&id)) {
                (_, Some(_)) => self.refuses_vim(&invocation),
                (Some(meta), None) => self.read_only && meta.mutates_buffer,
                (None, None) => false,
            };
            if !refused {
                return Some(invocation);
            }
            self.refuse_edit(cx);
            return None;
        };
        if let Some(state) = &mut self.vim {
            // An error, which a screen reader is told at once.
            self.announced.host_message(message.clone(), true);
            state.set_host_message(message);
        }
        cx.notify();
        None
    }

    /// Runs `command`, a map's `<Cmd>fe:…<CR>` command, once this update is
    /// over, as the keymap runs a binding's ([`run_remap_command`]): in the
    /// window the replay's keys came in (`origin`), or with none known, the
    /// one the editor is focused or shown in. So the editor's own commands
    /// run as their keys do (`editor.save` saves off the main thread, and
    /// asks where for an untitled buffer), and the workbench's and the
    /// app's too. Then the replay goes on from `queue` and `line`
    /// ([`EditorView::resume_replay`]), after the command even if the
    /// window dispatches it later itself. It holds meanwhile: no key comes
    /// between, as the update ends first, and what the command replays
    /// itself waits for it.
    fn run_remap_command_later(
        &mut self,
        command: RemapCommand,
        queue: VecDeque<VimKey>,
        line: Option<String>,
        origin: Option<ReplayOrigin>,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let focus = self.focus.clone();
        cx.defer(move |cx| {
            let window = match &origin {
                Some(origin) => Some(origin.window),
                None => editor_window(&focus, cx),
            };
            let target = view.clone();
            let ran = window.and_then(|handle| {
                let run = move |_, window: &mut Window, cx: &mut App| {
                    let handing = |cx: &App| {
                        (target.read(cx).vim.as_ref()).is_some_and(VimState::has_host_command)
                    };
                    let before = handing(cx);
                    let ok = run_remap_command(&target, command, origin.as_ref(), window, cx);
                    // Noted now: the workbench takes the command as the
                    // editor notifies, before the replay resumes.
                    let handed = !before && handing(cx);
                    // Deferred, so it follows a dispatch the window deferred.
                    cx.defer(move |cx| {
                        target.update(cx, |view, cx| {
                            // A command the editor ran as it was dispatched
                            // has failed by now, if it did.
                            let ok = ok && !view.command_failed;
                            view.resume_replay(ok, handed, queue, line, origin, cx)
                        })
                    });
                };
                handle.update(cx, run).ok()
            });
            if ran.is_none() {
                view.update(cx, |view, cx| {
                    view.vim_replaying = false;
                    view.stop_map(true, None);
                    view.release_held_keys(cx);
                    cx.notify();
                });
            }
        });
    }

    /// Goes on with a replay held for a map's `<Cmd>fe:…<CR>` command, now
    /// run, `ok` if it was dispatched: the keys it replayed itself first,
    /// then `queue`. A command that failed, left a file or window command
    /// for the host (`vim.diff_get`; `handed` if it did as it ran, which the
    /// host may have taken since), or switched vim, ends the map there
    /// instead.
    fn resume_replay(
        &mut self,
        ok: bool,
        handed: bool,
        mut queue: VecDeque<VimKey>,
        line: Option<String>,
        origin: Option<ReplayOrigin>,
        cx: &mut Context<Self>,
    ) {
        self.vim_replaying = false;
        // A command that switched vim off, or on anew, ends what it held.
        if self.sync_vim(cx) || self.vim.is_none() {
            self.end_live_line(None, cx);
            self.release_held_keys(cx);
            return cx.notify();
        }
        let Some(state) = &mut self.vim else {
            return;
        };
        let rest = !queue.is_empty();
        if !ok || state.failed() {
            self.stop_map(rest, None);
        } else if handed || state.has_host_command() {
            self.stop_map(rest, Some(handed_host_command()));
        } else {
            // A `:normal` suspended at the command goes on first, with the
            // rest of its keys and rows: they come before the map's keys.
            if state.has_normal_suspended() {
                let resume = Invocation::of("vim.normal_resume")
                    .expect("a valid command ID")
                    .source(Source::Keyboard);
                let context = stamp(fe_editor::commands::context(), self.vim.as_ref());
                if !self.run_vim_once(&resume.context(context), cx) {
                    self.end_live_line(line.as_deref(), cx);
                    self.stop_map(rest, None);
                    self.release_held_keys(cx);
                    self.wait_for_remap_later(origin, cx);
                    return cx.notify();
                }
            }
            let Some(state) = &mut self.vim else {
                return;
            };
            for key in state.take_replay().into_iter().rev() {
                queue.push_front(key);
            }
            return self.replay_from(queue, line, origin, cx);
        }
        self.end_live_line(line.as_deref(), cx);
        self.release_held_keys(cx);
        self.wait_for_remap_later(origin, cx);
        cx.notify();
    }

    /// The keystrokes this editor held while its replay waited for a map's
    /// command ([`EditorView::feed_vim`]), now the map is over: dispatched
    /// again, in order, once this update is over, in the window the editor
    /// is focused or shown in, as if typed now.
    fn release_held_keys(&self, cx: &mut Context<Self>) {
        let id = cx.entity_id();
        let vim = cx.default_global::<Vim>();
        let (mine, others): (Vec<_>, Vec<_>) = std::mem::take(&mut vim.held_keys)
            .into_iter()
            .partition(|(editor, _)| *editor == id);
        vim.held_keys = others;
        if mine.is_empty() {
            return;
        }
        let focus = self.focus.clone();
        cx.defer(move |cx| {
            let Some(window) = editor_window(&focus, cx) else {
                return;
            };
            let typed = |_, window: &mut Window, cx: &mut App| {
                for (_, keystroke) in mine {
                    window.dispatch_keystroke(keystroke, cx);
                }
            };
            window.update(cx, typed).ok();
        });
    }

    /// Feeds one replayed `key`, `line` holding the command line's text
    /// while it's open. Returns whether it worked, and whether the replay
    /// goes on: an ex line a map ran that left a file or window command
    /// (`:w`) ends it, `handed` set to what to say of the keys after it, as
    /// the host runs the command only once this update is over. A map's
    /// `<Cmd>…<CR>` goes to the reducer in every mode
    /// ([`VimState::remap_command_next`]).
    fn replay_key(
        &mut self,
        key: VimKey,
        line: &mut Option<String>,
        handed: &mut Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.sync_visual();
        let Some(state) = &mut self.vim else {
            return false;
        };
        if state.remap_command_next() {
            return self.replay_fed(key, handed, cx);
        }
        let mode = state.mode();
        if !matches!(mode, fe_vim::Mode::CmdLine(_)) {
            *line = None;
        }
        match mode {
            fe_vim::Mode::Insert | fe_vim::Mode::Replace => {
                // The key after `<C-r>` names a register: never matched.
                if state.insert_register_waiting() {
                    return self.type_replayed_key(key, cx);
                }
                match state.remap_insert(key) {
                    RemapInsert::Wait => !state.failed(),
                    RemapInsert::Keys(keys) => {
                        keys.into_iter().all(|key| self.type_replayed_key(key, cx))
                    }
                }
            }
            fe_vim::Mode::CmdLine(_) => {
                // As on the host's line: a character, and any key of a map's
                // right side (a recursive map's `<C-x>` too), goes through
                // the `c` maps; the line's other keys are its own, read
                // with none held ([`EditorView::unmapped_after_held`]). A
                // replay's [`LITERAL_NEXT`] is read before the maps too: the
                // character after it is literal text, unmapped. A map's is
                // a character.
                let literal = std::mem::take(&mut self.vim_line_literal);
                let expansion = state.remap_expansion_next();
                if !literal && key == VimKey::Char(LITERAL_NEXT) && !expansion {
                    state.record_key(key);
                    self.vim_line_literal = true;
                    return true;
                }
                if !literal && (matches!(key, VimKey::Char(_)) || expansion) {
                    return match state.remap_insert(key) {
                        RemapInsert::Wait => !state.failed(),
                        RemapInsert::Keys(keys) => keys
                            .into_iter()
                            .all(|key| self.replay_line_key(key, line, handed, cx)),
                    };
                }
                self.replay_line_key(key, line, handed, cx)
            }
            // The arrows too, so a map's are matched against the maps: with
            // nothing pending the reducer reads them as the keymap's
            // `vim.move`s do.
            _ => self.replay_fed(key, handed, cx),
        }
    }

    /// Whether `key`, the next a replay feeds on the command line while keys
    /// are held for a longer `c` map, is one the host's line never asks the
    /// maps about: a replay's [`LITERAL_NEXT`], or a key that's no
    /// character (`<CR>`, `<BS>`, `<C-c>`) and no map's. The held keys
    /// resolve first, as the host's line resolves them before its own keys
    /// (on a timeout), so no map takes it.
    fn unmapped_after_held(&self, key: VimKey) -> bool {
        let Some(state) = &self.vim else {
            return false;
        };
        let unmapped = match key {
            VimKey::Char(ch) => ch == LITERAL_NEXT && !self.vim_line_literal,
            _ => true,
        };
        unmapped
            && matches!(state.mode(), fe_vim::Mode::CmdLine(_))
            && state.remap_waiting()
            && !state.remap_expansion_next()
            && !state.remap_command_next()
    }

    /// One `key` of a replay or of a map's keys, typed on the command line.
    fn replay_line_key(
        &mut self,
        key: VimKey,
        line: &mut Option<String>,
        handed: &mut Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = &mut self.vim else {
            return false;
        };
        state.record_key(key);
        // Read before the line runs, which may read keys of its own.
        let mapped = state.expanding();
        if line.is_none() {
            self.vim_line_selected = false;
        }
        let text = line.get_or_insert_with(|| state.command_line_prefill().to_owned());
        let invocation = match key {
            // [`LITERAL_NEXT`] too, read as the marker before this.
            VimKey::Char(ch) => {
                if std::mem::take(&mut self.vim_line_selected) {
                    text.clear();
                }
                text.push(ch);
                return true;
            }
            VimKey::Backspace if Self::line_backspace(text, &mut self.vim_line_selected) => {
                return true;
            }
            // `<BS>` on an empty line, and `<C-c>`, cancel it. A
            // replay's `<Esc>` runs it, as in nvim (`:help c_<Esc>`).
            VimKey::Backspace | VimKey::Ctrl('c') => {
                Invocation::of("vim.command_line_cancel").expect("a valid command ID")
            }
            // A map's `<Esc>` cancels, as on the live line.
            VimKey::Esc if mapped => {
                Invocation::of("vim.command_line_cancel").expect("a valid command ID")
            }
            VimKey::Enter | VimKey::Ctrl('j') | VimKey::Esc => {
                Invocation::of("vim.command_line_submit")
                    .expect("a valid command ID")
                    .args(fe_command::Value::from_iter([("text", text.as_str())]))
            }
            // Keys the line has no use for are dropped, as the live line
            // drops them when a map types them.
            _ => return true,
        };
        *line = None;
        let context = stamp(fe_editor::commands::context(), self.vim.as_ref());
        let invocation = invocation.source(Source::Keyboard).context(context);
        let ok = self.run_vim_once(&invocation, cx);
        // A file or window command (`:w`) is the host's line's to
        // run, not a macro replay's; but a map's is left for the
        // host, which takes it from the editor, and ends the map.
        if mapped && self.vim.as_ref().is_some_and(VimState::has_host_command) {
            *handed = Some(handed_host_command());
            return false;
        }
        if let Some(command) = self.take_vim_host_command() {
            log::warn!("a macro replay doesn't run {command:?}");
        }
        ok && self.vim.as_ref().is_some_and(|s| !s.failed())
    }

    /// One `key` of a replay, or of a live key a map resolved, typed in
    /// insert or replace mode as the mode is now. Records it
    /// ([`VimState::record_key`]), which counts a typed key once and not a
    /// map's.
    fn type_replayed_key(&mut self, key: VimKey, cx: &mut Context<Self>) -> bool {
        let Some(state) = &mut self.vim else {
            return false;
        };
        let mode = state.mode();
        state.record_key(key);
        if state.insert_register_waiting()
            || key == VimKey::Ctrl('r')
            || (mode == fe_vim::Mode::Insert && key == VimKey::Ctrl('a'))
        {
            return self.register_typing(Some(key), cx);
        }
        let command = match key {
            VimKey::Char(ch) => {
                self.type_text(ch.encode_utf8(&mut [0; 4]), cx);
                return true;
            }
            // `<C-j>` (`<NL>`) is `<CR>` here.
            VimKey::Enter | VimKey::Ctrl('j') => NEWLINE,
            VimKey::Backspace => BACKSPACE,
            VimKey::Ctrl('t' | 'd') => return self.indent_typing(key, cx),
            VimKey::Ctrl('o') => return self.one_command_typing(cx),
            VimKey::Ctrl('n' | 'p') => return self.complete_typing(key, cx),
            // Vim's only while completing, as live.
            VimKey::Ctrl('e' | 'y') if self.keyword_completion().is_some() => {
                return self.complete_typing(key, cx);
            }
            VimKey::Ctrl(_) => return self.insert_key_typing(key, cx),
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

    /// [`EditorView::replay_key`] for a key the reducer reads: in normal or
    /// a visual mode, or a map's `<Cmd>…<CR>` in any. What it commits runs
    /// in the editor's context.
    fn replay_fed(
        &mut self,
        key: VimKey,
        handed: &mut Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = &mut self.vim else {
            return false;
        };
        match (single_caret_key(state, key), state.feed(key)) {
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
                // A map's `do`, `dp` or `<Cmd>w<CR>` hands the host a command
                // too, and ends the map, as an ex line's does.
                if mapped && self.vim.as_ref().is_some_and(VimState::has_host_command) {
                    *handed = Some(handed_host_command());
                    return false;
                }
                // A macro replay's `.` after a filter runs no shell command,
                // as its command line's filter doesn't.
                let shell = self.vim.as_ref().and_then(VimState::host_command);
                if !mapped && shell.is_some_and(fe_vim::HostCommand::runs_shell) {
                    let command = self.take_vim_host_command();
                    log::warn!("a macro replay doesn't run {command:?}");
                }
                ok && self.vim.as_ref().is_some_and(|s| !s.failed())
            }
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
        if self.settle_which_key() {
            cx.notify();
        }
        self.which_key_later(cx);
        ok
    }

    /// [`EditorView::run_vim`] without the replay: for the replay's own keys.
    pub(crate) fn run_vim_once(&mut self, invocation: &Invocation, cx: &mut Context<Self>) -> bool {
        // `<Esc>` after `<C-r>` only drops the prefix: insert mode goes on.
        if self.register_waiting()
            && matches!(
                invocation.command.as_str(),
                NORMAL_MODE | REPLACE_NORMAL_MODE
            )
        {
            self.cancel_register(cx);
            return true;
        }
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
        // Caught up first, so only the command's own edits count.
        self.model.sync_display();
        let before = self.model.snapshot().version();
        let (was_open, place) = (self.vim_line_open(), self.place());
        let Some(result) = self.dispatch_vim(&registry, invocation, cx) else {
            log::warn!("{} needs vim on", invocation.command);
            return false;
        };
        let mut ok = result.is_ok();
        self.sync_yank_flash();
        self.sync_substitute_preview();
        self.flash_new_yank(before, cx);
        let mutates = registry
            .get(&invocation.command)
            .is_some_and(|meta| meta.mutates_buffer);
        if let Err(error) = self.dispatched(result, invocation, mutates, cx) {
            log::warn!("{error}");
            ok = false;
        }
        if ok {
            self.emit_pasted(invocation, before, None, cx);
        }
        self.settle_search_preview(was_open, place, invocation);
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
        let server_folds = self.server_fold_ranges();
        let state = self.vim.as_mut()?;
        // Out of the global while the host holds `cx`, and back after.
        let mut registers = std::mem::take(&mut cx.default_global::<Vim>().registers);
        let mut host = ViewHost {
            cx,
            page_rows,
            syntax,
            server_folds,
            language,
            vim: true,
        };
        let mut target = VimTarget {
            model: &mut self.model,
            host: &mut host,
            state,
            registers: &mut registers,
        };
        let result = fe_vim::dispatch(registry, &mut target, invocation);
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
pub(crate) mod incsearch_tests;

mod substitute_preview;

#[cfg(test)]
mod substitute_preview_tests;

#[cfg(test)]
mod incsearch_fold_tests;

#[cfg(test)]
mod yank_flash_reset_tests;

#[cfg(test)]
mod yank_flash_setting_tests;

#[cfg(test)]
mod clipboard_tests;

#[cfg(test)]
mod scroll_tests;

#[cfg(test)]
mod viewport_resize_tests;

#[cfg(test)]
mod jump_setting_tests;

#[cfg(test)]
mod subword_setting_tests;

#[cfg(test)]
mod jump_flash_tests;

#[cfg(test)]
mod remap_setting_tests;

#[cfg(test)]
mod macro_tests;

#[cfg(test)]
mod remap_insert_tests;

#[cfg(test)]
mod remap_line_tests;
#[cfg(test)]
mod remap_tests;

#[cfg(test)]
mod which_key_tests;

#[cfg(test)]
mod which_key_chord_tests;

#[cfg(test)]
mod which_key_layout_tests;

#[cfg(test)]
mod which_key_icons_tests;

#[cfg(test)]
mod remap_cmd_tests;

#[cfg(test)]
mod remap_catalog_tests;

#[cfg(test)]
mod replace_tests;

#[cfg(test)]
mod auto_indent_tests;

impl From<ViewPlace> for SearchPreview {
    fn from(place: ViewPlace) -> Self {
        Self {
            opened_in: None,
            place,
            opened: Vec::new(),
            shown: None,
            reveal: None,
            substitute: None,
        }
    }
}

#[cfg(test)]
#[path = "vim_indent_keys_tests.rs"]
mod vim_indent_keys_tests;

/// The key before a character that no typed key made, in a recording: the
/// character after it is literal text. A private-use character, read as
/// this only when a replay feeds it on the command line, before the `c`
/// maps and once any keys held for a longer one resolve
/// ([`EditorView::unmapped_after_held`]), so no map's left side takes it; a
/// map's right side's is a character. One in a line recorded whole is
/// itself written after one ([`text_keys`]).
const LITERAL_NEXT: char = '\u{F8FF}';

/// Puts `held_back`, a key [`EditorView::replay_from`] holds until the keys
/// before it have run, back in `queue` where it was, for a replay that
/// stops or waits there.
fn put_back(queue: &mut VecDeque<VimKey>, held_back: Option<(VimKey, usize)>) {
    if let Some((key, after)) = held_back {
        queue.insert(queue.len().saturating_sub(after), key);
    }
}

/// What it takes to turn `old`, a command line's text, into `new` as a
/// replay does, which deletes a grapheme per `<BS>`: how many `<BS>`s, and
/// the byte offset in `new` the graphemes the two share end at.
fn grapheme_diff(old: &str, new: &str) -> (usize, usize) {
    use unicode_segmentation::UnicodeSegmentation as _;
    let mut kept = 0;
    let mut shared = 0;
    for (a, b) in old.graphemes(true).zip(new.graphemes(true)) {
        if a != b {
            break;
        }
        kept += b.len();
        shared += 1;
    }
    (old.graphemes(true).count() - shared, kept)
}

/// `chars` as the keys a recording holds for a line's text: each itself,
/// but a [`LITERAL_NEXT`] after one, so a replay reads it as text.
fn text_keys(chars: impl Iterator<Item = char>) -> impl Iterator<Item = VimKey> {
    chars.flat_map(|ch| {
        let marker = (ch == LITERAL_NEXT).then_some(VimKey::Char(LITERAL_NEXT));
        marker.into_iter().chain([VimKey::Char(ch)])
    })
}

/// `chars` as the keys a recording holds for text no typed key made: each
/// after a [`LITERAL_NEXT`], so a replay reads it as literal text and no `c` map
/// matches it.
fn literal_keys(chars: impl Iterator<Item = char>) -> impl Iterator<Item = VimKey> {
    chars.flat_map(|ch| [VimKey::Char(LITERAL_NEXT), VimKey::Char(ch)])
}
