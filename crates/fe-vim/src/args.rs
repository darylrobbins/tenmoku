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
/// find_backward till_forward till_backward`, carrying the char. `line` is
/// the target of a doubled operator such as `dd`: `count` lines from the
/// caret's, like Vim's `_`.
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
    Line,
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
