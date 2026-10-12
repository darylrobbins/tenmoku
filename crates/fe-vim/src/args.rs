//! The args of `vim.move` and `vim.operate`, in the shape of
//! `docs/decisions/command-model.md`'s "Vim composition" table, and of the
//! command line's commands.

use std::{
    collections::BTreeSet,
    sync::{Mutex, PoisonError},
};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{line_motions, prose};

/// `d y c > <`, and over a visual selection only, `~ u U J r{char}`, and
/// over a block `I` and `A`. `rot13`, `format` and `format_keep` (`g?`, `gq`
/// and `gw`) belong to `g_operators.rs`, which also owns `gu gU g~`
/// (`lowercase`, `uppercase` and `toggle_case`) over a motion or object.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Delete,
    Yank,
    Change,
    Indent,
    Outdent,
    ToggleCase,
    Lowercase,
    Uppercase,
    Join,
    /// `r{char}`: every grapheme but line breaks becomes the char.
    Replace(char),
    /// `I` over a block: an insert session at its left edge, the typed text
    /// then going in on every row of it that reaches that edge.
    Insert,
    /// `A` over a block: an insert session after its right edge (or each
    /// row's end, after `$`), the typed text then going in on every row of
    /// it, short rows padded with spaces.
    Append,
    /// `g?`.
    Rot13,
    /// `gq`.
    Format,
    /// `gw`.
    FormatKeep,
}

/// A motion, by its keys: `h l k j` are `left right up down`; `w b e` are
/// `word_forward word_backward word_end`, and `W B E` the `big_word_` ones;
/// `0 ^ $` are `line_start first_non_blank line_end`; `gg G` are
/// `document_start document_end`; and `f F t T` are `find_forward
/// find_backward till_forward till_backward`, carrying the char, and `; ,`
/// are `repeat_find repeat_find_reverse`; `%` is `match_pair` (with a count,
/// the row `count` percent down the text). `line` is the target of a doubled
/// operator such as `dd`: `count` lines from the caret's, like Vim's `_`.
/// `search` is what submitting `/` or `?` runs: `{"search": {"pattern":
/// "foo", "forward": true}}`. `search_next` and `search_previous` are `n`
/// and `N` over the last search and its direction, and `search_word_forward`
/// and `search_word_backward` are `*` and `#`: they resolve to a `search`
/// before they run, so `vim.move` and `vim.operate` take them but nothing
/// else sees them. `mark` is `'{name}` and `` `{name} `` (`exact`): `{"mark":
/// {"name": "a", "exact": true}}`. `jump` is `gs{c1}{c2}` and `gS{c1}{c2}`:
/// `{"jump": {"chars": "ab", "forward": true}}`. `paragraph_forward`
/// `paragraph_backward` `sentence_forward` `sentence_backward` are `} { ) (`
/// (`prose.rs`); `word_end_backward` `big_word_end_backward`
/// `next_line_first_non_blank` `prev_line_first_non_blank` `column`
/// `window_top` `window_middle` `window_bottom` `display_down` `display_up`
/// are `ge gE + - | H M L gj gk` (`line_motions.rs`; `_` is `line`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    BigWordForward,
    BigWordBackward,
    BigWordEnd,
    LineStart,
    FirstNonBlank,
    LineEnd,
    DocumentStart,
    DocumentEnd,
    FindForward(char),
    FindBackward(char),
    TillForward(char),
    TillBackward(char),
    RepeatFind,
    RepeatFindReverse,
    MatchPair,
    Line,
    Search(Search),
    /// `*` and `#` once resolved: `search` from the start of the keyword
    /// under or after the caret.
    SearchWord(Search),
    SearchNext,
    SearchPrevious,
    SearchWordForward,
    SearchWordBackward,
    Mark(MarkMotion),
    Jump(JumpMotion),
    /// A `mark` once resolved: the mark's offset in the text it was
    /// resolved in. Only `vim.move` and `vim.operate` make one, as they
    /// run; it's never serialized.
    #[serde(skip)]
    MarkAt {
        offset: usize,
        exact: bool,
    },
    ParagraphForward,
    ParagraphBackward,
    SentenceForward,
    SentenceBackward,
    WordEndBackward,
    BigWordEndBackward,
    NextLineFirstNonBlank,
    PrevLineFirstNonBlank,
    Column,
    WindowTop,
    WindowMiddle,
    WindowBottom,
    DisplayDown,
    DisplayUp,
}

/// `'{name}`, or `` `{name} `` if `exact`, as a motion: to the first
/// non-blank of the mark's row, linewise, or to the mark itself, exclusive
/// and charwise. The names are `a` to `z`, `'` and `` ` `` (both the
/// position before the last jump), and `<` and `>`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct MarkMotion {
    pub name: char,
    #[serde(default)]
    pub exact: bool,
}

/// `gs{c1}{c2}`, or `gS{c1}{c2}` if not `forward`, as a motion: the
/// `count`th match of the two chars after the caret, or before it, over the
/// whole text with no wrap, landing on `{c1}` ([`crate::jump::label::targets`]
/// says what matches).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct JumpMotion {
    #[schemars(with = "String")]
    pub chars: JumpChars,
    pub forward: bool,
}

/// The two chars a jump matches, typed as they are. It serializes as a
/// two-char string.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct JumpChars(pub [char; 2]);

impl Serialize for JumpChars {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0.iter().collect::<String>())
    }
}

impl<'de> Deserialize<'de> for JumpChars {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let mut chars = text.chars();
        match (chars.next(), chars.next(), chars.next()) {
            (Some(c1), Some(c2), None) => Ok(Self([c1, c2])),
            _ => Err(serde::de::Error::custom(format!(
                "a jump takes two chars, not {text:?}"
            ))),
        }
    }
}

/// `/` and `?` as a motion: the `count`th match of `pattern` (Vim's syntax,
/// [`crate::search::Pattern`]) after the caret, or before it if not
/// `forward`, wrapping round the text. A match at a row's end, on a row
/// with text, is its last character, as in nvim.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Search {
    #[schemars(with = "String")]
    pub pattern: SearchPattern,
    pub forward: bool,
}

/// A search pattern's text, interned so that [`Motion`] (and the args
/// holding one) stay `Copy`: each distinct pattern is kept for the life of
/// the process. It serializes as the text.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct SearchPattern(&'static str);

impl SearchPattern {
    pub fn new(pattern: &str) -> Self {
        static INTERNED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
        let mut interned = INTERNED.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(&kept) = interned.get(pattern) {
            return Self(kept);
        }
        let kept: &'static str = Box::leak(pattern.into());
        interned.insert(kept);
        Self(kept)
    }

    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Debug for SearchPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for SearchPattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for SearchPattern {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|pattern| Self::new(&pattern))
    }
}

/// How an operator takes a motion's range (`:help exclusive`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RangeClass {
    /// Up to the target, not including it: `h l w b 0 ^ F T / ?` and
    /// `` ` ``.
    Exclusive,
    /// Up to and including the target: `e $ f t %`.
    Inclusive,
    /// Whole rows: `j k gg G '` and a doubled operator.
    Linewise,
}

impl Motion {
    /// Its class as an operator's target. `;` and `,` take the class of the
    /// find they repeat, which only the last find knows: alone they say
    /// [`RangeClass::Inclusive`], as `f` and `t` are. `%` with a count is
    /// linewise.
    pub fn class(self) -> RangeClass {
        match self {
            Self::Left
            | Self::Right
            | Self::WordForward
            | Self::WordBackward
            | Self::BigWordForward
            | Self::BigWordBackward
            | Self::LineStart
            | Self::FirstNonBlank
            | Self::FindBackward(_)
            | Self::TillBackward(_)
            | Self::Jump(JumpMotion { forward: false, .. })
            | Self::Search(_)
            | Self::SearchWord(_)
            | Self::SearchNext
            | Self::SearchPrevious
            | Self::SearchWordForward
            | Self::SearchWordBackward => RangeClass::Exclusive,
            Self::WordEnd
            | Self::BigWordEnd
            | Self::LineEnd
            | Self::FindForward(_)
            | Self::TillForward(_)
            | Self::Jump(JumpMotion { forward: true, .. })
            | Self::RepeatFind
            | Self::RepeatFindReverse
            | Self::MatchPair => RangeClass::Inclusive,
            Self::Up | Self::Down | Self::DocumentStart | Self::DocumentEnd | Self::Line => {
                RangeClass::Linewise
            }
            Self::Mark(MarkMotion { exact, .. }) | Self::MarkAt { exact, .. } => match exact {
                true => RangeClass::Exclusive,
                false => RangeClass::Linewise,
            },
            Self::ParagraphForward
            | Self::ParagraphBackward
            | Self::SentenceForward
            | Self::SentenceBackward => prose::class(self),
            Self::WordEndBackward
            | Self::BigWordEndBackward
            | Self::NextLineFirstNonBlank
            | Self::PrevLineFirstNonBlank
            | Self::Column
            | Self::WindowTop
            | Self::WindowMiddle
            | Self::WindowBottom
            | Self::DisplayDown
            | Self::DisplayUp => line_motions::class(self),
        }
    }

    /// Whether it's a jump (`:help jump-motions`), which sets the jump mark
    /// (`''`) to where it left from: `gg G % / ? n N * #`, `'` and `` ` ``,
    /// and `gs` and `gS`.
    pub fn is_jump(self) -> bool {
        if self.is_prose() {
            return prose::is_jump(self);
        }
        if self.is_line_motion() {
            return line_motions::is_jump(self);
        }
        matches!(
            self,
            Self::DocumentStart
                | Self::DocumentEnd
                | Self::MatchPair
                | Self::Search(_)
                | Self::SearchWord(_)
                | Self::SearchNext
                | Self::SearchPrevious
                | Self::SearchWordForward
                | Self::SearchWordBackward
                | Self::Mark(_)
                | Self::MarkAt { .. }
                | Self::Jump(_)
        )
    }

    /// Whether `prose.rs` owns it.
    fn is_prose(self) -> bool {
        matches!(
            self,
            Self::ParagraphForward
                | Self::ParagraphBackward
                | Self::SentenceForward
                | Self::SentenceBackward
        )
    }

    /// Whether `line_motions.rs` owns it.
    fn is_line_motion(self) -> bool {
        matches!(
            self,
            Self::WordEndBackward
                | Self::BigWordEndBackward
                | Self::NextLineFirstNonBlank
                | Self::PrevLineFirstNonBlank
                | Self::Column
                | Self::WindowTop
                | Self::WindowMiddle
                | Self::WindowBottom
                | Self::DisplayDown
                | Self::DisplayUp
        )
    }
}

/// `iw aw iW aW i( a( i{ a{ i" a" i' a'` (`ib` and `iB` are `i(` and `i{`).
/// `inner_paragraph` `a_paragraph` `inner_sentence` `a_sentence` are `ip ap
/// is as` (`prose.rs`); `inner_bracket` `a_bracket` `inner_angle`
/// `a_angle` `inner_tag` `a_tag` `inner_backtick` `a_backtick` are `` i[ a[
/// i< a< it at i` a` `` (`delimiters.rs`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextObject {
    InnerWord,
    AWord,
    InnerBigWord,
    ABigWord,
    InnerParen,
    AParen,
    InnerBrace,
    ABrace,
    InnerDoubleQuote,
    ADoubleQuote,
    InnerSingleQuote,
    ASingleQuote,
    InnerParagraph,
    AParagraph,
    InnerSentence,
    ASentence,
    InnerBracket,
    ABracket,
    InnerAngle,
    AAngle,
    InnerTag,
    ATag,
    InnerBacktick,
    ABacktick,
}

/// What an operator acts on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Motion(Motion),
    TextObject(TextObject),
    /// The visual selection, as its kind takes it: in a visual mode only.
    Selection,
    /// The visual selection's rows, whole: `X D Y C S R`. In a block, `X`
    /// and `Y` take the block itself, as `d` and `y` do (`S` and `R` its
    /// rows).
    SelectedLines,
    /// `D` and `C` in a block: its rows from its left edge to their ends.
    /// Elsewhere it's [`Target::SelectedLines`].
    SelectedToEnd,
}

impl Target {
    /// Whether a delete over it writes register `1` even within a row
    /// (`:help quote_number`): `%`, `/`, `?` and `` ` ``, and `gs` and `gS`
    /// as `` ` ``.
    pub fn writes_numbered(self) -> bool {
        match self {
            Self::Motion(motion) if motion.is_prose() => return prose::writes_numbered(motion),
            Self::Motion(motion) if motion.is_line_motion() => {
                return line_motions::writes_numbered(motion);
            }
            _ => {}
        }
        matches!(
            self,
            Self::Motion(
                Motion::MatchPair
                    | Motion::Search(_)
                    | Motion::SearchWord(_)
                    | Motion::SearchNext
                    | Motion::SearchPrevious
                    | Motion::SearchWordForward
                    | Motion::SearchWordBackward
                    | Motion::Mark(MarkMotion { exact: true, .. })
                    | Motion::MarkAt { exact: true, .. }
                    | Motion::Jump(_)
            )
        )
    }

    /// Whether it's the visual selection's.
    pub fn is_selection(self) -> bool {
        matches!(
            self,
            Self::Selection | Self::SelectedLines | Self::SelectedToEnd
        )
    }
}

/// A visual mode's kind: `v`, `V` or `<C-v>`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VisualKind {
    Char,
    Line,
    /// A rectangle of display columns over the rows between the anchor and
    /// the cursor.
    Block,
}

/// `vim.visual`'s args: `v`, `V` or `<C-v>`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct VisualArgs {
    pub kind: VisualKind,
}

/// `vim.visual_swap`'s args: `o`, or `O` when `same_row`, which in a block
/// moves to the other corner on the cursor's row (elsewhere it's `o`).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SwapArgs {
    #[serde(default)]
    pub same_row: bool,
}

/// `vim.select`'s args: a text object typed in a visual mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SelectArgs {
    pub text_object: TextObject,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `vim.move`'s args.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MoveArgs {
    pub motion: Motion,
    /// Keep each selection's tail, extending it. In a visual mode the
    /// anchor stays whatever it says.
    #[serde(default)]
    pub extend: bool,
    /// The typed count; `None` when there was none, so `G` and `1G` differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `vim.operate`'s args.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OperateArgs {
    pub operator: Operator,
    pub target: Target,
    /// Both typed counts multiplied: `2d3w` is 6. Over a selection, how
    /// many times `>` and `<` shift; the others ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register: Option<char>,
}

/// The args of `vim.put` and `vim.visual_put`: `p`, or `P` when `before`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PutArgs {
    #[serde(default)]
    pub before: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register: Option<char>,
}

/// Where `vim.insert` puts the caret before the session starts.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InsertAt {
    /// `i`: before the caret's grapheme.
    Before,
    /// `a`: after it.
    After,
    /// `I`: before the row's first non-blank, or at its end if it's all
    /// blank.
    FirstNonBlank,
    /// `A`: at the row's end.
    LineEnd,
    /// `o`: on a new row below, indented as the caret's row is.
    LineBelow,
    /// `O`: on a new row above, indented as the caret's row is.
    LineAbove,
}

/// `vim.insert`'s args: `i a I A o O`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InsertArgs {
    pub at: InsertAt,
    /// How many times the typed text goes in (`3ix<Esc>` is `xxx`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// The args of the commands that take only a count: `vim.join`,
/// `vim.toggle_case`, `vim.undo`, `vim.redo` and `vim.repeat`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CountArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `vim.set_mark`'s args: `m{name}`, `a` to `z`, or `'` or `` ` `` for the
/// jump mark.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MarkArgs {
    pub name: char,
}

/// `vim.record`'s args: `q{a-z}`, or `q{A-Z}` to append to the lowercase
/// register.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RecordArgs {
    pub register: char,
}

/// `vim.play`'s args: `@{a-z}`, or `@@` (`register` `@`) for the last
/// register played, `count` times.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayArgs {
    pub register: char,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// `vim.replace_char`'s args: `r{char}`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReplaceCharArgs {
    pub char: char,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// What a command line is for: `/`, `?` or `:`. Its `vim.cmdline_kind`
/// value is `search_forward`, `search_backward` or `ex`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandLineKind {
    SearchForward,
    SearchBackward,
    /// An ex command line ([`crate::Ex`]).
    Ex,
}

impl CommandLineKind {
    /// Its `vim.cmdline_kind` value.
    pub fn name(self) -> &'static str {
        match self {
            Self::SearchForward => "search_forward",
            Self::SearchBackward => "search_backward",
            Self::Ex => "ex",
        }
    }
}

/// `vim.command_line`'s args: the kind of line, and what was pending when
/// it opened (`d2/`, `"a?`), which submitting it applies. An ex line takes
/// no register or operator; its count (`3:`) becomes the range it starts
/// with ([`crate::VimState::command_line_prefill`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommandLineArgs {
    pub kind: CommandLineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register: Option<char>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<Operator>,
}

/// `vim.command_line_submit`'s args: the line's text, as the host's line
/// holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SubmitArgs {
    pub text: String,
}
