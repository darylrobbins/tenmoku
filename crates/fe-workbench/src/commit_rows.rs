//! What the History and Git Log panels' commit lists share: a commit row's
//! texts, `<short>  <summary>` then `<author>, <time ago>` (shown muted),
//! and the lists' keys. Up and down move the selection and Enter opens;
//! with vim on (the panel's key context then has the `vim` flag), `j` and
//! `k` move, `g g` and `G` go to the first and last row, `o` opens and `q`
//! hides the dock, focusing the active editor ([`leave`]). With vim off
//! the letters do nothing.

use fe_git::CommitInfo;
use gpui::{App, Context, Focusable as _, KeyContext, KeyDownEvent, WeakEntity, Window};

use crate::{Workbench, dock::DockOp};

/// The flag a list's key context has while vim is on.
const VIM_FLAG: &str = "vim";

/// Seconds since the epoch.
pub(super) fn now() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |since| since.as_secs() as i64)
}

/// How long ago `seconds` was: `just now`, `5 minutes ago`, `1 day ago`,
/// as the branch picker says it.
fn ago(seconds: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let seconds = seconds.max(0);
    let (count, unit) = if seconds < MINUTE {
        return "just now".to_owned();
    } else if seconds < HOUR {
        (seconds / MINUTE, "minute")
    } else if seconds < DAY {
        (seconds / HOUR, "hour")
    } else if seconds < 14 * DAY {
        (seconds / DAY, "day")
    } else if seconds < 60 * DAY {
        (seconds / (7 * DAY), "week")
    } else if seconds < 365 * DAY {
        (seconds / (30 * DAY), "month")
    } else {
        (seconds / (365 * DAY), "year")
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural} ago")
}

/// `commit`'s row texts, at `now`: `<short>  <summary>` and `<author>,
/// <time ago>`.
pub(super) fn texts(commit: &CommitInfo, now: i64) -> [String; 2] {
    [
        format!("{}  {}", commit.short, commit.summary),
        format!("{}, {}", commit.author, ago(now - commit.time)),
    ]
}

/// What a key in a list does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Key {
    Up,
    Down,
    First,
    Last,
    Open,
    Quit,
}

impl Key {
    /// The row it selects, from `selected`, in a list of `len` rows; none
    /// for an empty list, Open or Quit.
    pub(super) fn moved(self, selected: Option<usize>, len: usize) -> Option<usize> {
        let last = len.checked_sub(1)?;
        Some(match self {
            Key::Up => selected.map_or(0, |ix| ix.saturating_sub(1)),
            Key::Down => selected.map_or(0, |ix| (ix + 1).min(last)),
            Key::First => 0,
            Key::Last => last,
            Key::Open | Key::Quit => return None,
        })
    }
}

/// A list's keys: whether `g` was pressed, with vim on, waiting for a
/// second.
#[derive(Default)]
pub(super) struct ListKeys {
    pending_g: bool,
}

impl ListKeys {
    /// The list key `event` is, if any: see the module docs. Stops the
    /// propagation of those it takes, the first `g` among them.
    pub(super) fn key<T>(&mut self, event: &KeyDownEvent, cx: &mut Context<T>) -> Option<Key> {
        let keystroke = &event.keystroke;
        // Anything but a second g drops the prefix.
        let pending_g = std::mem::take(&mut self.pending_g);
        let m = keystroke.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return None;
        }
        let vim = fe_gpui::vim_enabled(cx);
        let key = match (keystroke.key.as_str(), m.shift) {
            ("up", false) => Key::Up,
            ("down", false) => Key::Down,
            ("enter", false) => Key::Open,
            _ if !vim => return None,
            ("j", false) => Key::Down,
            ("k", false) => Key::Up,
            ("g", false) if pending_g => Key::First,
            ("g", false) => {
                self.pending_g = true;
                cx.stop_propagation();
                return None;
            }
            ("g", true) => Key::Last,
            ("o", false) => Key::Open,
            ("q", false) => Key::Quit,
            _ => return None,
        };
        cx.stop_propagation();
        Some(key)
    }
}

/// A list's key context: `context`, with the `vim` flag while vim is on.
pub(super) fn key_context(context: &str, cx: &App) -> KeyContext {
    let mut key_context = KeyContext::default();
    key_context.add(context);
    if fe_gpui::vim_enabled(cx) {
        key_context.add(VIM_FLAG);
    }
    key_context
}

/// `q` in a list: hides the dock and focuses the active editor, once the
/// panel's update is over, as hiding the dock looks at which of its panels
/// has focus.
pub(super) fn leave(workbench: WeakEntity<Workbench>, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        let leave = |workbench: &mut Workbench, cx: &mut Context<Workbench>| {
            if workbench.dock.read(cx).is_visible() {
                workbench.run_dock_op(DockOp::Toggle, window, cx);
            }
            window.focus(&workbench.focus_handle(cx), cx);
        };
        workbench.update(cx, leave).ok();
    });
}
