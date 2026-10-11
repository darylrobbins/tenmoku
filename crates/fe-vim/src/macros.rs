//! Macros (`docs/decisions/vim-engine.md` §4 and §9 item 6): `q{a-z}`
//! records the keys typed into a register as text, and `@{a-z}` hands that
//! text back as keys for the host to feed through its own path.

use fe_buffer::TransactionId;
use fe_command::{CommandError, Dispatched, Effect};

use crate::{PlayArgs, RecordArgs, VimKey, VimState, VimTarget};

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
    /// Keys `@` asked for that the host hasn't taken yet, in order.
    queued: Vec<VimKey>,
    /// Keys the host took and hasn't fed back yet: the next that many are
    /// a replay's.
    left: usize,
    /// Keys the replay under way asked for, for [`REPLAY_CAP`].
    played: usize,
    /// The key being read is a replay's.
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

    /// Adds a typed key to the recording, if one is under way: a replay's
    /// keys aren't recorded, as nvim records the `@a` that played them.
    pub(crate) fn record(&mut self, key: VimKey) {
        if let Some(recording) = self.recording.as_mut().filter(|_| !self.replayed) {
            encode(key, &mut recording.text);
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
    Ok(Dispatched::new(Effect::Changed))
}

/// `vim.record_stop`: ends the recording, writing its keys to the register
/// (appending to it for `q{A-Z}`). `"` still reads what it read.
pub(crate) fn stop(target: &mut VimTarget<'_>) -> Handled {
    let Some(recording) = target.state.macros.recording.take() else {
        return Err(CommandError::new("not recording"));
    };
    target.registers.record(recording.register, recording.text);
    Ok(Dispatched::new(Effect::Changed))
}

/// `vim.play`: leaves register `register`'s text (`@` for the last one
/// played), `count` times, as keys for [`VimState::take_replay`], ahead of
/// any the host hasn't taken. An empty register fails, and so does one
/// that would take the replay under way past [`REPLAY_CAP`] keys, with a
/// message.
pub(crate) fn play(target: &mut VimTarget<'_>, args: PlayArgs) -> Handled {
    let macros = &mut target.state.macros;
    let name = match (args.register, macros.last_played) {
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
