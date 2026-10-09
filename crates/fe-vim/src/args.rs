//! The args of `vim.move` and `vim.operate`, in the shape of
//! `docs/decisions/command-model.md`'s "Vim composition" table, and of the
//! command line's commands.

use std::{
    collections::BTreeSet,
    sync::{Mutex, PoisonError},
};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// `d y c > <`, and over a visual selection only, `~ u U J r{char}`, and
/// over a block `I` and `A`.
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
/// "foo", "forward": true}}`.
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
    /// Up to the target, not including it: `h l w b 0 ^ F T / ?`.
    Exclusive,
    /// Up to and including the target: `e $ f t %`.
    Inclusive,
    /// Whole rows: `j k gg G` and a doubled operator.
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
            | Self::Search(_) => RangeClass::Exclusive,
            Self::WordEnd
            | Self::BigWordEnd
            | Self::LineEnd
            | Self::FindForward(_)
            | Self::TillForward(_)
            | Self::RepeatFind
            | Self::RepeatFindReverse
            | Self::MatchPair => RangeClass::Inclusive,
            Self::Up | Self::Down | Self::DocumentStart | Self::DocumentEnd | Self::Line => {
                RangeClass::Linewise
            }
        }
    }
}

/// `iw aw iW aW i( a( i{ a{ i" a" i' a'` (`ib` and `iB` are `i(` and `i{`).
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
    /// (`:help quote_number`): `%`, `/` and `?`.
    pub fn writes_numbered(self) -> bool {
        matches!(self, Self::Motion(Motion::MatchPair | Motion::Search(_)))
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

/// `vim.replace_char`'s args: `r{char}`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReplaceCharArgs {
    pub char: char,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// What a command line is for: `/` or `?`. Its `vim.cmdline_kind` value is
/// `search_forward` or `search_backward`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandLineKind {
    SearchForward,
    SearchBackward,
}

impl CommandLineKind {
    /// Its `vim.cmdline_kind` value.
    pub fn name(self) -> &'static str {
        match self {
            Self::SearchForward => "search_forward",
            Self::SearchBackward => "search_backward",
        }
    }
}

/// `vim.command_line`'s args: the kind of line, and what was pending when
/// it opened (`d2/`, `"a?`), which submitting it applies.
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
