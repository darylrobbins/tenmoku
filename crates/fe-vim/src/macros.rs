//! Macros (`docs/decisions/vim-engine.md` §4 and §9 item 6): `q{a-z}`
//! records the keys typed into a register as text, and `@{a-z}` hands that
//! text back as keys for the host to feed through its own path.
//!
//! A jump (`gs{c1}{c2}`) records what it resolved to rather than the keys
//! typed: once it lands, directly, by autojump or by a label, the keys from
//! its trigger through the one that picked it (paging keys, and an `<Esc>`
//! after an autojump, included) are `{n}gs{c1}{c2}` (`gS` backward, and
//! after `s` or `S` too), `n` the target's count from where it started, so
//! the replay reaches the same match whatever the view shows. A key that
//! falls through after an autojump is recorded after them, as itself. A
//! cancelled jump leaves none of its command's keys.

use fe_buffer::TransactionId;
use fe_command::{Actor, CommandError, Dispatched, Effect};

use crate::{JumpMotion, PlayArgs, RecordArgs, VimKey, VimState, VimTarget};

/// The most keys one replay feeds, `@` inside it included: nvim plays until
/// an error or `<C-c>`.
pub(crate) const REPLAY_CAP: usize = 1_000_000;

/// The keys a register's text holds in vim notation, where nvim stores a
/// byte sequence that isn't text.
const NAMED: [(&str, VimKey); 5] = [
    ("<BS>", VimKey::Backspace),
    ("<Left>", VimKey::Left),
    ("<Right>", VimKey::Right),
    ("<Up>", VimKey::Up),
    ("<Down>", VimKey::Down),
];

type Handled = Result<Dispatched<TransactionId>, CommandError>;

/// The recording under way, the last register played, and the replay the
/// host is feeding.
#[derive(Clone, Debug, Default)]
pub(crate) struct Macros {
    recording: Option<Recording>,
    /// For `@@`.
    last_played: Option<char>,
    /// The last register recorded, lowercase, for `Q`.
    last_recorded: Option<char>,
    /// The last ex line submitted, for `@:`.
    pub(crate) last_command_line: Option<String>,
    /// Keys `@` asked for that the host hasn't taken yet, in order.
    queued: Vec<VimKey>,
    /// Keys the host took and hasn't fed back yet: the next that many are
    /// a replay's.
    left: usize,
    /// Keys the replay under way asked for, for [`REPLAY_CAP`].
    played: usize,
    /// The key being read is a replay's.
    replayed: bool,
    /// How many `:normal`s are running ([`Macros::hold`]): their keys
    /// aren't typed, so a recording doesn't get them.
    held: u32,
    /// Where the command being read began in the recording's text: a
    /// cancelled jump takes its keys back to here.
    command_at: usize,
    /// Where the jump being read began, at its trigger (`gs`, `gS`, `s` or
    /// `S`): once it lands, its keys from here are the counted jump.
    jump_at: Option<usize>,
}

/// A replay a `:normal` set aside while its keys run ([`Macros::hold`]).
#[derive(Debug)]
pub(crate) struct Held {
    queued: Vec<VimKey>,
    left: usize,
    played: usize,
    replayed: bool,
}

/// `q{a-z}` (or `q{A-Z}`, appending) under way: the keys so far, as the
/// register's text.
#[derive(Clone, Debug)]
struct Recording {
    register: char,
    text: String,
}

impl Macros {
    /// The register `@@` plays, `:` after `@:`.
    pub(crate) fn last_played(&self) -> Option<char> {
        self.last_played
    }

    /// The lowercase register being recorded into.
    pub(crate) fn recording(&self) -> Option<char> {
        let recording = self.recording.as_ref()?;
        Some(recording.register.to_ascii_lowercase())
    }

    /// Counts a key the host reads: a replay's while any it took are left,
    /// else a typed one, which starts a new replay's count. Returns whether
    /// it's a replay's.
    pub(crate) fn count(&mut self) -> bool {
        self.replayed = self.left > 0;
        match self.replayed {
            true => self.left -= 1,
            false => self.played = 0,
        }
        self.replayed
    }

    /// Whether the key being read was typed: not a replay's, nor a
    /// `:normal`'s.
    fn typed(&self) -> bool {
        !self.replayed && self.held == 0
    }

    /// Adds a typed key to the recording, if one is under way: a replay's
    /// keys aren't recorded, as nvim records the `@a` that played them.
    pub(crate) fn record(&mut self, key: VimKey) {
        let typed = self.typed();
        if let Some(recording) = self.recording.as_mut().filter(|_| typed) {
            encode(key, &mut recording.text);
        }
    }

    /// The key about to be recorded starts a command (nothing was
    /// pending).
    pub(crate) fn command_start(&mut self) {
        self.jump_at = None;
        if let Some(recording) = self.recording.as_ref().filter(|_| self.typed()) {
            self.command_at = recording.text.len();
        }
    }

    /// The last key recorded finished a jump's trigger, `trigger` keys long
    /// (`gs` or `s`).
    pub(crate) fn jump_start(&mut self, trigger: usize) {
        if let Some(recording) = self.recording.as_ref().filter(|_| self.typed()) {
            self.jump_at = Some(recording.text.len().saturating_sub(trigger));
        }
    }

    /// The jump being read landed with `count` (`None` for a counted one,
    /// whose count was typed before the trigger): its keys so far, from the
    /// trigger, become `{count}gs{c1}{c2}` (`gS` backward), which replays
    /// as that counted jump, showing no labels, from wherever it plays.
    /// Landing again (a label after an autojump) writes it again.
    pub(crate) fn jump_landed(&mut self, count: Option<u32>, jump: JumpMotion) {
        if !self.typed() {
            return;
        }
        let (Some(at), Some(recording)) = (self.jump_at, self.recording.as_mut()) else {
            return;
        };
        recording.text.truncate(at.min(recording.text.len()));
        if let Some(count) = count {
            recording.text.push_str(&count.to_string());
        }
        recording
            .text
            .push_str(if jump.forward { "gs" } else { "gS" });
        for ch in jump.chars.0 {
            encode(VimKey::Char(ch), &mut recording.text);
        }
    }

    /// The command being read, a jump, was cancelled: none of its keys
    /// stay recorded.
    pub(crate) fn jump_cancelled(&mut self) {
        let typed = self.typed();
        if let Some(recording) = self.recording.as_mut().filter(|_| typed) {
            recording
                .text
                .truncate(self.command_at.min(recording.text.len()));
        }
        self.jump_at = None;
    }

    /// An ex line was submitted, typed or replayed: `@:` runs it again.
    /// Lines a `:normal` runs aren't.
    pub(crate) fn command_line(&mut self, line: &str) {
        if self.held == 0 && !line.is_empty() {
            self.last_command_line = Some(line.to_string());
        }
    }

    /// The keys queued for the host, which counts them as a replay's. Once
    /// it has fed them all and there are no more, the replay is over: a
    /// `@` from then on starts another.
    pub(crate) fn take(&mut self) -> Vec<VimKey> {
        let keys = std::mem::take(&mut self.queued);
        self.left += keys.len();
        if self.left == 0 {
            self.stop();
        }
        keys
    }

    /// Sets the replay under way aside while a `:normal` runs keys that
    /// aren't typed: they're counted as a replay of their own (a `@` among
    /// them plays), and no recording gets them, but a recording or `@`
    /// they start stays.
    pub(crate) fn hold(&mut self) -> Held {
        self.held += 1;
        Held {
            queued: std::mem::take(&mut self.queued),
            left: std::mem::take(&mut self.left),
            played: std::mem::take(&mut self.played),
            replayed: std::mem::take(&mut self.replayed),
        }
    }

    /// Puts back the replay [`Macros::hold`] set aside.
    pub(crate) fn release(&mut self, held: Held) {
        self.held -= 1;
        self.queued = held.queued;
        self.left = held.left;
        self.played = held.played;
        self.replayed = held.replayed;
    }

    /// Ends the replay, whatever is left of it.
    pub(crate) fn stop(&mut self) {
        self.queued.clear();
        self.left = 0;
        self.played = 0;
        self.replayed = false;
    }
}

/// `vim.record`: starts recording the keys typed into `register`.
pub(crate) fn record(state: &mut VimState, args: RecordArgs) -> Handled {
    if !args.register.is_ascii_alphabetic() {
        return Err(CommandError::new(format!(
            "{:?} can't hold a recording",
            args.register
        )));
    }
    if state.macros.recording.is_some() {
        return Err(CommandError::new("already recording"));
    }
    state.macros.recording = Some(Recording {
        register: args.register,
        text: String::new(),
    });
    state.macros.command_at = 0;
    state.macros.jump_at = None;
    Ok(Dispatched::new(Effect::Changed))
}

/// `vim.record_stop`: ends the recording, writing its keys to the register
/// (appending to it for `q{A-Z}`). `"` still reads what it read.
pub(crate) fn stop(target: &mut VimTarget<'_>) -> Handled {
    let Some(recording) = target.state.macros.recording.take() else {
        return Err(CommandError::new("not recording"));
    };
    target.state.macros.last_recorded = Some(recording.register.to_ascii_lowercase());
    target.registers.record(recording.register, recording.text);
    Ok(Dispatched::new(Effect::Changed))
}

/// `vim.play`: leaves register `register`'s text (`@` for the last one
/// played), `count` times, as keys for [`VimState::take_replay`], ahead of
/// any the host hasn't taken. An empty register fails, and so does one
/// that would take the replay under way past [`REPLAY_CAP`] keys, with a
/// message.
pub(crate) fn play(target: &mut VimTarget<'_>, actor: &Actor, args: PlayArgs) -> Handled {
    let macros = &mut target.state.macros;
    let register = match (args.register, macros.last_played) {
        ('@', Some(':')) => ':',
        (register, _) => register,
    };
    if register == ':' {
        return repeat_line(target, actor, args.count);
    }
    let name = match (register, macros.last_played) {
        ('Q', _) => match macros.last_recorded {
            Some(last) => last,
            // No message: the host's own `Q` key shows none either.
            None => {
                target.state.failed = true;
                return Ok(Dispatched::new(Effect::Nothing));
            }
        },
        ('@', Some(last)) => last,
        ('@', None) => return Ok(target.state.fail("E748: No previously used register")),
        (name, _) if name.is_ascii_lowercase() => name,
        (name, _) => return Err(CommandError::new(format!("{name:?} can't be played"))),
    };
    macros.last_played = Some(name);
    // A typed `@` drops keys a host never took.
    if !macros.replayed {
        macros.queued.clear();
    }
    let text = target.registers.get(name).map_or("", |r| r.text.as_str());
    let count = args.count.unwrap_or(1).max(1) as usize;
    // Decoded only as far as the replay may still go.
    let Some(keys) = decode(text, (REPLAY_CAP - macros.played) / count) else {
        return Ok(target.state.fail(format!(
            "macro stopped: it would replay more than {REPLAY_CAP} keys"
        )));
    };
    if keys.is_empty() {
        target.state.failed = true;
        return Ok(Dispatched::new(Effect::Nothing));
    }
    macros.played += keys.len() * count;
    let mut queued = keys.repeat(count);
    queued.append(&mut macros.queued);
    macros.queued = queued;
    Ok(Dispatched::new(Effect::Notify))
}

/// `@:`: runs the last command line `count` times, straight through the ex
/// runner so the `:` history is untouched, stopping at the first failure.
fn repeat_line(target: &mut VimTarget<'_>, actor: &Actor, count: Option<u32>) -> Handled {
    target.state.macros.last_played = Some(':');
    let Some(line) = target.state.macros.last_command_line.clone() else {
        return Ok(target.state.fail("E30: No previous command line"));
    };
    let before = std::mem::take(&mut target.state.failed);
    let mut dispatched = Dispatched::new(Effect::Notify);
    for _ in 0..count.unwrap_or(1).max(1) {
        let ran = crate::ex::run(target, actor, &line)?;
        if ran.effect == Effect::Changed || dispatched.effect != Effect::Changed {
            dispatched = ran;
        }
        if target.state.failed {
            break;
        }
    }
    target.state.failed |= before;
    Ok(dispatched)
}

/// Writes `key` as nvim stores it in a register: a byte where nvim stores
/// one (`<Esc>` `\x1b`, `<CR>` `\r`, `<Tab>` `\t`, `<C-x>` the control
/// byte), else in vim notation (`<BS>`, `<Left>`).
pub(crate) fn encode(key: VimKey, text: &mut String) {
    let ch = match key {
        VimKey::Char(ch) => ch,
        VimKey::Ctrl(ch) if ('?'..='_').contains(&ch.to_ascii_uppercase()) => {
            char::from(ch.to_ascii_uppercase() as u8 ^ 0x40)
        }
        VimKey::Ctrl(ch) => {
            text.push_str("<C-");
            text.push(ch);
            text.push('>');
            return;
        }
        VimKey::Esc => '\x1b',
        VimKey::Enter => '\r',
        VimKey::Tab => '\t',
        _ => {
            let (name, _) = NAMED
                .iter()
                .find(|(_, named)| *named == key)
                .expect("named");
            text.push_str(name);
            return;
        }
    };
    text.push(ch);
}

/// A register's text as the keys it plays: [`encode`]'s bytes and names
/// back (a line break is `<C-j>`, nvim's `<NL>`), anything else as itself;
/// `None` if that's more than `most` keys.
pub(crate) fn decode(text: &str, most: usize) -> Option<Vec<VimKey>> {
    let mut keys = Vec::with_capacity(text.len().min(most));
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        if keys.len() == most {
            return None;
        }
        if let Some((name, key)) = NAMED.iter().find(|(name, _)| rest.starts_with(name)) {
            keys.push(*key);
            rest = &rest[name.len()..];
            continue;
        }
        keys.push(match ch {
            '\x1b' => VimKey::Esc,
            '\r' => VimKey::Enter,
            '\t' => VimKey::Tab,
            '\x01'..='\x1a' => VimKey::Ctrl(char::from(ch as u8 | 0x60)),
            _ => VimKey::Char(ch),
        });
        rest = &rest[ch.len_utf8()..];
    }
    Some(keys)
}
