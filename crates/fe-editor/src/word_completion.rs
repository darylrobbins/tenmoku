//! Words for the completion menu
//! (docs/decisions/mgacx379x8-completion-as-you-type.md): the identifiers
//! in a buffer's text, offered after what a language server offers, or
//! alone without one, so the menu has something in plain text and config
//! files too. GPUI-free; the host scans each open buffer of the language
//! ([`BufferWords::scan`], off the main thread) and joins the words to its
//! answer ([`word_items`], [`merge_words`]).
//!
//! - **A word** is a run of word chars (letters, digits and `_`, as the
//!   menu's typed word is: [`crate::completion`]) of at least
//!   [`MIN_CHARS`] chars, not starting with a digit, and no longer than
//!   [`MAX_CHARS`].
//! - **Bounds.** A buffer's scan reads at most [`SCAN_BYTES`] of its text,
//!   around the caret for the buffer typed in, and keeps at most
//!   [`MAX_WORDS`] distinct words; the items offered are capped at as many.
//! - **Order.** Word items rank after every other item
//!   ([`WORD_GROUP`]), the buffer's own words before other buffers' at an
//!   equal match. The word being typed isn't offered, nor a word a server's
//!   item already offers.

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use fe_buffer::{Bias, Text};

use crate::{
    CharClass,
    completion::{CompletionItem, CompletionList},
};

/// The fewest chars a word offered has.
pub const MIN_CHARS: usize = 3;
/// The most chars a word offered has: longer runs (an encoded blob, say)
/// aren't words anyone types.
pub const MAX_CHARS: usize = 100;
/// The most distinct words a scan keeps, and the most items offered.
pub const MAX_WORDS: usize = 10_000;
/// The most bytes of a buffer's text a scan reads.
pub const SCAN_BYTES: usize = 1 << 20;
/// The group word items rank in: after every other source's
/// ([`CompletionItem::group`]).
pub const WORD_GROUP: u8 = u8::MAX;

/// Whether `c` is a word char, as the menu's typed word counts them.
pub fn is_word_char(c: char) -> bool {
    CharClass::of(c) == CharClass::Word
}

/// The distinct words of a buffer's text, each with how often it occurs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BufferWords {
    counts: HashMap<String, u32>,
    /// The bytes of the text scanned, and the text's length then.
    window: Range<usize>,
    len: usize,
}

/// The bytes of a text `len` long that a scan centred on `around` reads:
/// all of it, or [`SCAN_BYTES`] around `around`, before char boundaries.
fn scan_window(len: usize, around: usize) -> Range<usize> {
    if len <= SCAN_BYTES {
        return 0..len;
    }
    let start = around.saturating_sub(SCAN_BYTES / 2).min(len - SCAN_BYTES);
    start..start + SCAN_BYTES
}

impl BufferWords {
    /// The words of `text`, reading at most [`SCAN_BYTES`] of it, centred
    /// on byte `around` (the caret) where it's longer. A word the window's
    /// edge cuts through isn't one.
    pub fn scan(text: &Text, around: usize) -> Self {
        let len = text.len();
        let window = scan_window(len, around);
        let range =
            text.clip_offset(window.start, Bias::Left)..text.clip_offset(window.end, Bias::Right);
        let mut words = text.slice_to_string(range.clone());
        // Cut at the end: the run up to the edge goes on past it.
        if text.char_at(range.end).is_some_and(is_word_char) {
            let kept = words.trim_end_matches(is_word_char).len();
            words.truncate(kept);
        }
        let mut words = words.as_str();
        // Cut at the start: the run from the edge began before it.
        if char_before(text, range.start).is_some_and(is_word_char) {
            words = words.trim_start_matches(is_word_char);
        }
        Self {
            window: range,
            len,
            ..Self::of_str(words)
        }
    }

    /// Whether this scan of a text, at the same version, serves one asked
    /// for around byte `around`: it read all of the text, or the caret is
    /// well inside what it read, a quarter of [`SCAN_BYTES`] from an edge
    /// that isn't the text's.
    pub fn covers(&self, around: usize) -> bool {
        let (window, len) = (&self.window, self.len);
        let margin = SCAN_BYTES / 4;
        let low = match window.start {
            0 => 0,
            start => start + margin,
        };
        let high = match window.end >= len {
            true => len,
            false => window.end.saturating_sub(margin),
        };
        (low..=high).contains(&around)
    }

    /// The words of `text`.
    pub fn of_str(text: &str) -> Self {
        let mut counts: HashMap<String, u32> = HashMap::new();
        let mut word = String::new();
        let mut chars = 0;
        let mut keep = |word: &mut String, chars: &mut usize| {
            let wanted = (MIN_CHARS..=MAX_CHARS).contains(chars)
                && !word.starts_with(|c: char| c.is_numeric());
            if wanted {
                if let Some(count) = counts.get_mut(word.as_str()) {
                    *count += 1;
                } else if counts.len() < MAX_WORDS {
                    counts.insert(word.clone(), 1);
                }
            }
            word.clear();
            *chars = 0;
        };
        for c in text.chars() {
            if is_word_char(c) {
                // Past the longest kept, only counted.
                if chars <= MAX_CHARS {
                    word.push(c);
                }
                chars += 1;
            } else if chars > 0 {
                keep(&mut word, &mut chars);
            }
        }
        if chars > 0 {
            keep(&mut word, &mut chars);
        }
        Self {
            counts,
            window: 0..text.len(),
            len: text.len(),
        }
    }

    /// How often `word` occurs.
    pub fn count(&self, word: &str) -> u32 {
        self.counts.get(word).copied().unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.counts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// The words, in no order.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.counts.keys().map(String::as_str)
    }
}

/// The most bytes [`word_at`] reads on either side: a run longer than
/// the longest word ([`MAX_CHARS`]) is cut there.
const WORD_BYTES: usize = 4 * (MAX_CHARS + 1);

/// The char that ends at `at`, if any.
pub fn char_before(text: &Text, at: usize) -> Option<char> {
    let start = text.clip_offset(at.saturating_sub(4), Bias::Left);
    text.slice_to_string(start..at).chars().next_back()
}

/// The word around byte `offset` of `text`: the run of word chars (which
/// a line break never is) that it's in or touches, reading no more than
/// [`WORD_BYTES`] on either side however long the row. Empty at `offset`
/// if there's none.
pub fn word_at(text: &Text, offset: usize) -> Range<usize> {
    let offset = text.clip_offset(offset, Bias::Left);
    let start = text.clip_offset(offset.saturating_sub(WORD_BYTES), Bias::Left);
    let end = text.clip_offset((offset + WORD_BYTES).min(text.len()), Bias::Right);
    let before = text.slice_to_string(start..offset);
    let after = text.slice_to_string(offset..end);
    let back: usize = (before.chars().rev())
        .take_while(|&c| is_word_char(c))
        .map(char::len_utf8)
        .sum();
    let ahead: usize = (after.chars())
        .take_while(|&c| is_word_char(c))
        .map(char::len_utf8)
        .sum();
    offset - back..offset + ahead
}

/// Another open buffer's words, for [`word_items`].
#[derive(Clone, Debug)]
pub struct OtherWords {
    pub words: Arc<BufferWords>,
    /// What an item of a word only there says it is, such as "Word in
    /// lib.rs".
    pub detail: Option<String>,
}

/// The items for the words of `current`, the buffer typed in, then of
/// `others`, each word once, but `typed`, the word being typed. Each
/// replaces `edit` (the typed word's start to the caret, in the version
/// asked at) with its word, its kind `kind`. At most [`MAX_WORDS`].
pub fn word_items(
    current: &BufferWords,
    typed: &str,
    others: &[OtherWords],
    edit: Range<usize>,
    kind: &str,
) -> Vec<CompletionItem> {
    let mut seen: HashSet<&str> = HashSet::from([typed]);
    let mut items = Vec::new();
    let item = |word: &str, tier: char, detail: Option<&String>| CompletionItem {
        label: word.to_owned(),
        kind: Some(kind.to_owned()),
        detail: detail.cloned(),
        // This buffer's words before others' at an equal match.
        sort_text: Some(format!("{tier}{word}")),
        group: WORD_GROUP,
        edit: (edit.clone(), word.to_owned()),
        ..CompletionItem::default()
    };
    let mut words: Vec<&str> = current.words().collect();
    words.sort_unstable();
    for word in words {
        if items.len() < MAX_WORDS && seen.insert(word) {
            items.push(item(word, '0', None));
        }
    }
    for other in others {
        let mut words: Vec<&str> = other.words.words().collect();
        words.sort_unstable();
        for word in words {
            if items.len() < MAX_WORDS && seen.insert(word) {
                items.push(item(word, '1', other.detail.as_ref()));
            }
        }
    }
    items
}

/// `list`, a provider's answer (`None` for none), with `words` after its
/// items, but those it offers already: by label, filter text or text put
/// in. `None` only with neither.
pub fn merge_words(
    list: Option<CompletionList>,
    words: Vec<CompletionItem>,
) -> Option<CompletionList> {
    if words.is_empty() {
        return list;
    }
    let mut list = list.unwrap_or_default();
    let offered: HashSet<String> = (list.items.iter())
        .flat_map(|item| {
            [
                Some(item.label.as_str()),
                item.filter_text.as_deref(),
                (!item.snippet).then_some(item.edit.1.as_str()),
            ]
        })
        .flatten()
        .map(|text| text.trim().to_owned())
        .collect();
    (list.items).extend(
        words
            .into_iter()
            .filter(|word| !offered.contains(&word.label)),
    );
    Some(list)
}
