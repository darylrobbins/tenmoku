//! The args of `vim.move` and `vim.operate`, in the shape of
//! `docs/decisions/command-model.md`'s "Vim composition" table.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `d y c > <`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Delete,
    Yank,
    Change,
    Indent,
    Outdent,
}

/// A motion, by its keys: `h l k j` are `left right up down`; `w b e` are
/// `word_forward word_backward word_end`, and `W B E` the `big_word_` ones;
/// `0 ^ $` are `line_start first_non_blank line_end`; `gg G` are
/// `document_start document_end`; and `f F t T` are `find_forward
/// find_backward till_forward till_backward`, carrying the char, and `; ,`
/// are `repeat_find repeat_find_reverse`; `%` is `match_pair` (with a count,
/// the row `count` percent down the text). `line` is the target of a doubled
/// operator such as `dd`: `count` lines from the caret's, like Vim's `_`.
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
}

/// How an operator takes a motion's range (`:help exclusive`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RangeClass {
    /// Up to the target, not including it: `h l w b 0 ^ F T`.
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
            | Self::TillBackward(_) => RangeClass::Exclusive,
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
}

/// `vim.move`'s args.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MoveArgs {
    pub motion: Motion,
    /// Keep each selection's tail, extending it (visual mode, later).
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
    /// Both typed counts multiplied: `2d3w` is 6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register: Option<char>,
}

/// `vim.put`'s args: `p`, or `P` when `before`.
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
