//! Vi copy mode in a [`TerminalView`]: ctrl-shift-space (alacritty's key)
//! freezes the view under a block cursor that moves with vim's motions,
//! selects with v, V and ctrl-v, yanks with y and searches with a `/` or
//! `?` prompt drawn at the bottom of the view. No key reaches the pty while
//! it's on. The model is `fe-terminal`'s; this is the keys.

use fe_terminal::{SelectionKind, TerminalQuery, ViMove};
use gpui::{ClipboardItem, Context, Keystroke};

use crate::TerminalView;
use crate::input::{KeyInput, OPTION_COMPOSES, clipboard_key};

/// The largest count a motion takes: more only takes longer.
const MAX_COUNT: usize = 10_000;

/// A key as vi mode reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ViKey {
    /// A character typed without ctrl.
    Char(char),
    /// Ctrl and a letter.
    Ctrl(char),
    Escape,
    Enter,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    /// Anything else: does nothing.
    Other,
}

impl ViKey {
    pub(crate) fn of(keystroke: &Keystroke) -> ViKey {
        let m = &keystroke.modifiers;
        if m.platform || m.function {
            return ViKey::Other;
        }
        let key = keystroke.key.as_str();
        if m.control {
            let mut chars = key.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) if !m.alt && c.is_ascii_alphabetic() => {
                    ViKey::Ctrl(c.to_ascii_lowercase())
                }
                _ => ViKey::Other,
            };
        }
        if m.alt {
            // Alt isn't part of any vi key: alt-q doesn't leave, alt-y
            // doesn't yank. On macOS option types characters, which are
            // taken as typed.
            let composed = keystroke.key_char.as_deref().filter(|typed| *typed != key);
            let mut chars = composed.filter(|_| OPTION_COMPOSES).unwrap_or("").chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) if !c.is_control() => ViKey::Char(c),
                _ => ViKey::Other,
            };
        }
        match key {
            "escape" => return ViKey::Escape,
            "enter" => return ViKey::Enter,
            "backspace" => return ViKey::Backspace,
            "up" => return ViKey::Up,
            "down" => return ViKey::Down,
            "left" => return ViKey::Left,
            "right" => return ViKey::Right,
            "home" => return ViKey::Home,
            "end" => return ViKey::End,
            "pageup" => return ViKey::PageUp,
            "pagedown" => return ViKey::PageDown,
            "space" => return ViKey::Char(' '),
            _ => {}
        }
        // What the platform says the key types, shift and all.
        let typed = keystroke.key_char.as_deref().unwrap_or(key);
        let mut chars = typed.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if m.shift && keystroke.key_char.is_none() => {
                ViKey::Char(c.to_uppercase().next().unwrap_or(c))
            }
            (Some(c), None) if !c.is_control() => ViKey::Char(c),
            _ => ViKey::Other,
        }
    }
}

/// What a key asks of the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViCommand {
    /// Moves the vi cursor this many times.
    Move(ViMove, usize),
    /// [`fe_terminal::Terminal::vi_scroll`] by this many lines.
    Scroll(i32),
    /// [`fe_terminal::Terminal::vi_scroll_page`] by this many pages.
    Page(i32),
    /// [`fe_terminal::Terminal::vi_scroll_view`] by this many lines.
    ScrollView(i32),
    Select(SelectionKind),
    /// Copies the selection and leaves.
    Yank,
    /// Goes to the previous or next prompt mark this many times.
    Prompt {
        forward: bool,
        times: usize,
    },
    /// Runs a search typed at the prompt; an empty one repeats the last.
    Search {
        query: String,
        forward: bool,
    },
    /// Repeats the last search, the other way when `reverse`.
    Repeat {
        reverse: bool,
        times: usize,
    },
    /// Clears the selection, or leaves with none.
    Escape,
    Leave,
}

/// The search prompt `/` and `?` open.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Prompt {
    forward: bool,
    query: String,
}

/// Vi mode's key state: a count and the first key of a two-key command
/// typed so far, and the search prompt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ViKeys {
    count: Option<usize>,
    /// `g`, `[` or `]`, waiting for its second key.
    pending: Option<char>,
    prompt: Option<Prompt>,
    /// The last `/` or `?` went back, so `n` does, until another query
    /// replaces it ([`TerminalView::set_find`]), which goes forward.
    pub(crate) backward: bool,
}

impl ViKeys {
    /// Forgets what was typed so far, keeping the last search.
    pub(crate) fn reset(&mut self) {
        *self = ViKeys {
            backward: self.backward,
            ..ViKeys::default()
        };
    }

    /// The search prompt as drawn: `/` or `?`, then the query.
    pub(crate) fn prompt(&self) -> Option<String> {
        let prompt = self.prompt.as_ref()?;
        let leader = if prompt.forward { '/' } else { '?' };
        Some(format!("{leader}{}", prompt.query))
    }

    /// Takes `key` on a view `rows` high: the command it completes, if any.
    pub(crate) fn key(&mut self, key: ViKey, rows: usize) -> Option<ViCommand> {
        if let Some(prompt) = &mut self.prompt {
            match key {
                ViKey::Escape | ViKey::Ctrl('c') => self.prompt = None,
                ViKey::Backspace => {
                    if prompt.query.pop().is_none() {
                        self.prompt = None;
                    }
                }
                ViKey::Enter => {
                    let Prompt { forward, query } = self.prompt.take()?;
                    return Some(ViCommand::Search { query, forward });
                }
                ViKey::Char(c) => prompt.query.push(c),
                _ => {}
            }
            return None;
        }

        let pending = self.pending.take();
        let count = self.count.take();
        let times = count.unwrap_or(1);
        let lines = |n: usize| i32::try_from(n.saturating_mul(times)).unwrap_or(i32::MAX);
        let half = (rows / 2).max(1);
        let motion = |to: ViMove| Some(ViCommand::Move(to, times));
        match (pending, key) {
            (Some('g'), ViKey::Char('g')) => return Some(ViCommand::Move(ViMove::Top, 1)),
            (Some('['), ViKey::Char('[')) => {
                return Some(ViCommand::Prompt {
                    forward: false,
                    times,
                });
            }
            (Some(']'), ViKey::Char(']')) => {
                return Some(ViCommand::Prompt {
                    forward: true,
                    times,
                });
            }
            // Anything else drops the first key and is taken on its own,
            // so Esc and the keys that leave always work.
            _ => {}
        }
        match key {
            ViKey::Char(c @ '0'..='9') if c != '0' || count.is_some() => {
                let digit = c as usize - '0' as usize;
                let count = count.unwrap_or(0).saturating_mul(10) + digit;
                self.count = Some(count.min(MAX_COUNT));
                None
            }
            ViKey::Char(c @ ('g' | '[' | ']')) => {
                self.pending = Some(c);
                self.count = count;
                None
            }
            ViKey::Char('h') | ViKey::Left => motion(ViMove::Left),
            ViKey::Char('j') | ViKey::Down => motion(ViMove::Down),
            ViKey::Char('k') | ViKey::Up => motion(ViMove::Up),
            ViKey::Char('l') | ViKey::Right => motion(ViMove::Right),
            ViKey::Char('w') => motion(ViMove::WordNext),
            ViKey::Char('b') => motion(ViMove::WordPrevious),
            ViKey::Char('e') => motion(ViMove::WordEnd),
            ViKey::Char('W') => motion(ViMove::BigWordNext),
            ViKey::Char('B') => motion(ViMove::BigWordPrevious),
            ViKey::Char('E') => motion(ViMove::BigWordEnd),
            ViKey::Char('0') | ViKey::Home => motion(ViMove::LineStart),
            ViKey::Char('^') => motion(ViMove::FirstNonBlank),
            ViKey::Char('$') | ViKey::End => motion(ViMove::LineEnd),
            ViKey::Char('H') => motion(ViMove::High),
            ViKey::Char('M') => motion(ViMove::Middle),
            ViKey::Char('L') => motion(ViMove::Low),
            ViKey::Char('%') => motion(ViMove::Bracket),
            ViKey::Char('{') => motion(ViMove::ParagraphUp),
            ViKey::Char('}') => motion(ViMove::ParagraphDown),
            ViKey::Char('G') => Some(ViCommand::Move(ViMove::Bottom, 1)),
            ViKey::Ctrl('e') => Some(ViCommand::ScrollView(lines(1))),
            ViKey::Ctrl('y') => Some(ViCommand::ScrollView(-lines(1))),
            ViKey::Ctrl('d') => Some(ViCommand::Scroll(lines(half))),
            ViKey::Ctrl('u') => Some(ViCommand::Scroll(-lines(half))),
            ViKey::Ctrl('f') | ViKey::PageDown => Some(ViCommand::Page(lines(1))),
            ViKey::Ctrl('b') | ViKey::PageUp => Some(ViCommand::Page(-lines(1))),
            ViKey::Char('v') => Some(ViCommand::Select(SelectionKind::Simple)),
            ViKey::Char('V') => Some(ViCommand::Select(SelectionKind::Line)),
            ViKey::Ctrl('v') => Some(ViCommand::Select(SelectionKind::Block)),
            ViKey::Char('y') => Some(ViCommand::Yank),
            ViKey::Char(c @ ('/' | '?')) => {
                self.prompt = Some(Prompt {
                    forward: c == '/',
                    query: String::new(),
                });
                None
            }
            ViKey::Char('n') => Some(ViCommand::Repeat {
                reverse: false,
                times,
            }),
            ViKey::Char('N') => Some(ViCommand::Repeat {
                reverse: true,
                times,
            }),
            ViKey::Escape => Some(ViCommand::Escape),
            ViKey::Char('q' | 'i') | ViKey::Ctrl('c') => Some(ViCommand::Leave),
            _ => None,
        }
    }
}

/// Whether `keystroke` turns vi mode on or off: ctrl-shift-space on every
/// platform.
pub(crate) fn is_toggle(keystroke: &Keystroke) -> bool {
    let m = &keystroke.modifiers;
    keystroke.key == "space" && m.control && m.shift && !m.alt && !m.platform && !m.function
}

impl TerminalView {
    /// Whether the session is in vi copy mode.
    pub fn is_vi_mode(&self) -> bool {
        self.terminal.as_ref().is_some_and(|t| t.is_vi_mode())
    }

    /// Turns vi copy mode on or off, as ctrl-shift-space does. Nothing
    /// before the session starts.
    pub fn toggle_vi_mode(&mut self, cx: &mut Context<Self>) {
        let on = !self.is_vi_mode();
        self.set_vi_mode(on, cx);
    }

    /// The search prompt drawn at the bottom of the view, as shown: `/` or
    /// `?` and the query.
    pub fn vi_prompt(&self) -> Option<String> {
        self.vi.prompt()
    }

    pub(crate) fn set_vi_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        terminal.set_vi_mode(on);
        self.vi.reset();
        // A composition doesn't carry over: vi mode drops composed text.
        self.marked = None;
        cx.notify();
    }

    /// Takes a keystroke in vi mode. None reaches the pty.
    pub(crate) fn vi_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        // The clipboard keys work in the search prompt too.
        match clipboard_key(keystroke) {
            Some(KeyInput::Paste) => {
                self.set_vi_mode(false, cx);
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    self.paste_text(&text, cx);
                }
                return;
            }
            Some(KeyInput::Copy) => {
                if let Some(text) = terminal.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                return;
            }
            _ => {}
        }
        let rows = terminal.snapshot().rows.len();
        let command = self.vi.key(ViKey::of(keystroke), rows);
        if let Some(command) = command {
            self.vi_command(command, cx);
        }
        cx.notify();
    }

    fn vi_command(&mut self, command: ViCommand, cx: &mut Context<Self>) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        match command {
            ViCommand::Move(to, times) => {
                for _ in 0..times {
                    terminal.vi_motion(to);
                }
            }
            ViCommand::Scroll(lines) => terminal.vi_scroll(lines),
            ViCommand::Page(pages) => terminal.vi_scroll_page(pages),
            ViCommand::ScrollView(lines) => terminal.vi_scroll_view(lines),
            ViCommand::Select(kind) => terminal.vi_select(Some(kind)),
            ViCommand::Yank => {
                if let Some(text) = terminal.selected_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                    self.set_vi_mode(false, cx);
                }
            }
            ViCommand::Prompt { forward, times } => {
                for _ in 0..times {
                    if !terminal.vi_prompt(forward) {
                        break;
                    }
                }
            }
            ViCommand::Search { query, forward } => {
                let query = if query.is_empty() {
                    match &self.find {
                        Some(query) => query.clone(),
                        None => return,
                    }
                } else {
                    TerminalQuery::regex(query)
                };
                // A query that doesn't compile changes nothing.
                if terminal.vi_search(&query, forward).is_ok() {
                    let _ = self.set_find(Some(query), cx);
                    self.vi.backward = !forward;
                }
            }
            ViCommand::Repeat { reverse, times } => {
                let Some(query) = self.find.clone() else {
                    return;
                };
                let forward = self.vi.backward == reverse;
                for _ in 0..times {
                    if !matches!(terminal.vi_search(&query, forward), Ok(Some(_))) {
                        break;
                    }
                }
            }
            ViCommand::Escape => {
                if terminal.has_selection() {
                    terminal.vi_select(None);
                } else {
                    self.set_vi_mode(false, cx);
                }
            }
            ViCommand::Leave => self.set_vi_mode(false, cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(source: &str) -> ViKey {
        ViKey::of(&Keystroke::parse(source).unwrap().with_simulated_ime())
    }

    /// The commands `keys` complete, one per key, on a 10-row view.
    fn typed(keys: &mut ViKeys, sources: &str) -> Vec<Option<ViCommand>> {
        (sources.split(' '))
            .map(|source| keys.key(key(source), 10))
            .collect()
    }

    #[test]
    fn keys_as_vi_reads_them() {
        assert_eq!(key("j"), ViKey::Char('j'));
        assert_eq!(key("shift-w"), ViKey::Char('W'));
        assert_eq!(key("$"), ViKey::Char('$'));
        assert_eq!(key("ctrl-d"), ViKey::Ctrl('d'));
        assert_eq!(key("ctrl-shift-v"), ViKey::Ctrl('v'));
        assert_eq!(key("escape"), ViKey::Escape);
        assert_eq!(key("cmd-j"), ViKey::Other);
        // Alt is no vi key's.
        for alt in [
            "alt-q",
            "alt-i",
            "alt-y",
            "alt-j",
            "alt-left",
            "alt-escape",
            "alt-shift-g",
        ] {
            assert_eq!(key(alt), ViKey::Other, "{alt}");
        }
        // But on macOS option types characters.
        let mut composed = Keystroke::parse("alt-8").unwrap();
        composed.key_char = Some("[".into());
        let expected = if OPTION_COMPOSES {
            ViKey::Char('[')
        } else {
            ViKey::Other
        };
        assert_eq!(ViKey::of(&composed), expected);
        assert!(is_toggle(&Keystroke::parse("ctrl-shift-space").unwrap()));
        assert!(!is_toggle(&Keystroke::parse("ctrl-space").unwrap()));
    }

    #[test]
    fn counts_and_two_key_commands() {
        use ViCommand::*;
        let mut keys = ViKeys::default();
        assert_eq!(
            typed(&mut keys, "5 j 1 0 w 0"),
            [
                None,
                Some(Move(ViMove::Down, 5)),
                None,
                None,
                Some(Move(ViMove::WordNext, 10)),
                Some(Move(ViMove::LineStart, 1)),
            ]
        );
        assert_eq!(
            typed(&mut keys, "g g g x G"),
            [
                None,
                Some(Move(ViMove::Top, 1)),
                None,
                None,
                Some(Move(ViMove::Bottom, 1))
            ]
        );
        assert_eq!(
            typed(&mut keys, "2 [ [ ] ]"),
            [
                None,
                None,
                Some(Prompt {
                    forward: false,
                    times: 2
                }),
                None,
                Some(Prompt {
                    forward: true,
                    times: 1
                }),
            ]
        );
        assert_eq!(
            typed(&mut keys, "ctrl-d 2 ctrl-u ctrl-f ctrl-b ctrl-e ctrl-y"),
            [
                Some(Scroll(5)),
                None,
                Some(Scroll(-10)),
                Some(Page(1)),
                Some(Page(-1)),
                Some(ScrollView(1)),
                Some(ScrollView(-1)),
            ]
        );
        assert_eq!(keys.key(key("z"), 10), None);
    }

    #[test]
    fn a_pending_key_gives_way_to_the_next() {
        for first in ["g", "[", "]"] {
            for (second, command) in [
                ("escape", ViCommand::Escape),
                ("q", ViCommand::Leave),
                ("i", ViCommand::Leave),
                ("ctrl-c", ViCommand::Leave),
                ("j", ViCommand::Move(ViMove::Down, 1)),
            ] {
                let mut keys = ViKeys::default();
                typed(&mut keys, first);
                assert_eq!(keys.key(key(second), 10), Some(command), "{first} {second}");
                assert_eq!(keys, ViKeys::default(), "{first} {second}");
            }
        }
        // A count before it carries on to the key taken instead.
        let mut keys = ViKeys::default();
        assert_eq!(
            typed(&mut keys, "2 g j"),
            [None, None, Some(ViCommand::Move(ViMove::Down, 2))]
        );
    }

    #[test]
    fn the_search_prompt() {
        let mut keys = ViKeys::default();
        assert_eq!(typed(&mut keys, "? a b"), [None, None, None]);
        assert_eq!(keys.prompt().as_deref(), Some("?ab"));
        // Keys are the query's while it's open.
        assert_eq!(typed(&mut keys, "j backspace"), [None, None]);
        assert_eq!(keys.prompt().as_deref(), Some("?ab"));
        assert_eq!(
            keys.key(ViKey::Enter, 10),
            Some(ViCommand::Search {
                query: "ab".into(),
                forward: false
            })
        );
        assert_eq!(keys.prompt(), None);
        // n goes on the way the search went; N the other.
        assert_eq!(
            typed(&mut keys, "n 3 N"),
            [
                Some(ViCommand::Repeat {
                    reverse: false,
                    times: 1
                }),
                None,
                Some(ViCommand::Repeat {
                    reverse: true,
                    times: 3
                }),
            ]
        );

        // Backspace on an empty query, or escape, cancels.
        typed(&mut keys, "/ x backspace backspace");
        assert_eq!(keys.prompt(), None);
        typed(&mut keys, "/ x escape");
        assert_eq!(keys.prompt(), None);
    }
}
