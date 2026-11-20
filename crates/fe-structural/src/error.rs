//! Why a pattern didn't compile, and where.

/// What went wrong compiling a [`Pattern`](crate::Pattern).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PatternErrorKind {
    /// The pattern is empty or only whitespace.
    Empty,
    /// The pattern doesn't parse in the language: the offset is the first
    /// syntax error, or the place a token is missing.
    Syntax,
    /// A metavariable can't stand in for one node here: no placeholder for it
    /// parsed as a single token (as in `x$A`, where it would join `x`).
    MetavarNotANode,
    /// The same name is used both as `$NAME` and as `$$$NAME`.
    MixedMetavar,
    /// The language couldn't parse at all: no grammar could be set on a
    /// parser, or a WebAssembly grammar ran past its deadline.
    ParseFailed,
}

/// A pattern that didn't compile: what went wrong, and the byte offset in
/// the pattern source as the user wrote it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{} at byte {offset}", describe(*kind))]
pub struct PatternError {
    pub kind: PatternErrorKind,
    pub offset: usize,
}

fn describe(kind: PatternErrorKind) -> &'static str {
    match kind {
        PatternErrorKind::Empty => "empty pattern",
        PatternErrorKind::Syntax => "syntax error in pattern",
        PatternErrorKind::MetavarNotANode => "metavariable is not a single node",
        PatternErrorKind::MixedMetavar => "metavariable used as both $NAME and $$$NAME",
        PatternErrorKind::ParseFailed => "pattern could not be parsed",
    }
}
