//! Insert and cmdline maps the keymap hides: the keymap wins where it
//! matches (`docs/decisions/vim-remaps.md` §4 rule 1), so a map whose left
//! side, or a prefix of it, the keymap binds in the context that mode keys
//! are matched in never runs. The config problems list says so.

use fe_command::{ContextFrame, ContextStack};
use fe_keymap::{Key, KeySequence, Keymap, Keystroke, Modifiers};
use fe_vim::{Remap, RemapMode, RemapSet, VimKey, parse_keys};

/// A map the keymap hides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemapConflict {
    /// The map's index in the records given.
    pub index: usize,
    /// The mode the keymap binds it in: `Insert` or `CmdLine`.
    pub mode: RemapMode,
    /// The map's left side as written.
    pub from: String,
    /// The binding's keys, which are the left side or the start of it.
    pub keys: KeySequence,
    /// The binding's command.
    pub command: String,
}

/// Each of `remaps`' `Insert` and `CmdLine` modes whose left side (with
/// `<leader>` as `leader`, in vim notation) or a prefix of it `keymap`
/// binds where that mode's keys are matched: an editor with vim in insert
/// or replace mode, and vim's command line. A map that can't load has no
/// conflict (it has its own problem).
pub fn remap_keymap_conflicts(
    keymap: &Keymap,
    remaps: &[Remap],
    leader: &str,
) -> Vec<RemapConflict> {
    let mut conflicts = Vec::new();
    let Ok(leader_keys) = parse_keys(leader) else {
        return conflicts;
    };
    for (index, remap) in remaps.iter().enumerate() {
        // A record RemapSet can't load never runs, and has its own problem.
        if !RemapSet::new(std::slice::from_ref(remap), leader)
            .1
            .is_empty()
        {
            continue;
        }
        let Some(keys) = left_side(&remap.from, &leader_keys) else {
            continue;
        };
        let typed: Vec<Vec<Keystroke>> = keys.iter().map(keystrokes).collect();
        for &mode in &remap.modes {
            let stacks = match mode {
                RemapMode::Insert => vec![editor_stack("insert"), editor_stack("replace")],
                RemapMode::CmdLine => vec![cmdline_stack()],
                _ => continue,
            };
            let mut seen: Vec<&fe_keymap::Binding> = Vec::new();
            for binding in keymap.bindings() {
                let bound = binding.keys.keystrokes();
                let hides = bound.len() <= typed.len()
                    && typed.iter().zip(bound).all(|(t, b)| t.contains(b));
                if hides
                    && stacks.iter().any(|stack| binding.depth(stack).is_some())
                    && !seen.contains(&binding)
                {
                    seen.push(binding);
                    conflicts.push(RemapConflict {
                        index,
                        mode,
                        from: remap.from.clone(),
                        keys: binding.keys.clone(),
                        command: binding.command.to_string(),
                    });
                }
            }
        }
    }
    conflicts
}

/// The keys of a left side: each piece between `<leader>`s (any case)
/// read as vim notation alone, with the leader's keys between them, as
/// `RemapSet` reads it.
fn left_side(text: &str, leader: &[VimKey]) -> Option<Vec<VimKey>> {
    let lower = text.to_ascii_lowercase();
    let mut keys = Vec::new();
    let mut start = 0;
    loop {
        let end = lower[start..].find("<leader>");
        let piece = &text[start..end.map_or(text.len(), |at| start + at)];
        keys.extend(parse_keys(piece).ok()?);
        let Some(at) = end else { return Some(keys) };
        keys.extend_from_slice(leader);
        start += at + "<leader>".len();
    }
}

/// Each shifted symbol with its key on a US layout.
const SHIFTED: [(char, char); 21] = [
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
    ('~', '`'),
];

/// The keystrokes that can type `key`: a shifted symbol is also its US
/// layout base key with shift, as `shift-4` for `$`.
fn keystrokes(key: &VimKey) -> Vec<Keystroke> {
    let shifted = |c: char| {
        Keystroke::new(
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
            Key::Char(c),
        )
    };
    match *key {
        VimKey::Char(c) if c.is_ascii_uppercase() => vec![shifted(c.to_ascii_lowercase())],
        VimKey::Char(c) => {
            let mut found = Vec::new();
            found.extend(keystroke(key));
            let base = SHIFTED.iter().find(|(symbol, _)| *symbol == c);
            found.extend(base.map(|&(_, base)| shifted(base)));
            found
        }
        _ => keystroke(key).into_iter().collect(),
    }
}

/// The keystroke that types `key` alone, if the keymap can bind one.
fn keystroke(key: &VimKey) -> Option<Keystroke> {
    let plain = |key| Keystroke::new(Modifiers::default(), key);
    Some(match *key {
        VimKey::Char(' ') => plain(Key::Space),
        VimKey::Char(c) if c.is_ascii_uppercase() => return None,
        VimKey::Char(c) => plain(Key::Char(c)),
        VimKey::Ctrl(c) => Keystroke::new(
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            Key::Char(c),
        ),
        VimKey::Esc => plain(Key::Escape),
        VimKey::Enter => plain(Key::Enter),
        VimKey::Backspace => plain(Key::Backspace),
        VimKey::Tab => plain(Key::Tab),
        VimKey::Left => plain(Key::Left),
        VimKey::Right => plain(Key::Right),
        VimKey::Up => plain(Key::Up),
        VimKey::Down => plain(Key::Down),
    })
}

/// A focused editor with vim on, in `mode` (`vim.mode`'s value).
fn editor_stack(mode: &str) -> ContextStack {
    ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(
            ContextFrame::new(fe_editor::commands::CONTEXT)
                .flag(crate::vim::ENABLED)
                .set("vim.mode", mode),
        )
}

/// Vim's command line, with its input focused.
fn cmdline_stack() -> ContextStack {
    ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("vim_line"))
        .with(ContextFrame::new("text_input"))
}

#[cfg(test)]
#[path = "keymap_remap_conflicts_tests.rs"]
mod tests;
