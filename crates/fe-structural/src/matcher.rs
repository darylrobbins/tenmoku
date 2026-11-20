//! Matching a compiled [`Pattern`] against a file's syntax tree.
//!
//! The file tree is walked depth first; each node of the pattern root's kind
//! is compared with the pattern tree. Two nodes match when their kinds are
//! the same and, for a named leaf, their text; an anonymous token matches by
//! kind alone. Children are compared pairwise in order, extras (comments)
//! skipped on both sides, and a different number of them is a mismatch.
//! Field names are not compared. A wildcard matches any one node; a named
//! one binds it, and a later occurrence of the same name must cover the same
//! source text. Multi wildcards (`$$$`) are structural search SS4.

use std::ops::Range;

use tree_sitter::{Node, Tree, TreeCursor};

use crate::{
    captures::Captures,
    pattern::{Pattern, PatternNode},
};

/// How [`Pattern::find_all`] reports matches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchOptions {
    /// Report matches inside other matches too. Off, a node that matches
    /// isn't searched further, so no two matches overlap.
    pub nested: bool,
}

/// Where a pattern matched: the node's bytes, and what its metavariables
/// covered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// The matched node's byte range in the source.
    pub range: Range<usize>,
    /// The named metavariables' byte ranges in the source.
    pub captures: Captures,
}

/// Why a search returned no matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum Stopped {
    /// The stop callback said to stop.
    #[error("the search was cancelled")]
    Cancelled,
    /// The pattern holds a multi metavariable (`$$$`), which matching
    /// doesn't support yet.
    #[error("multi metavariables ($$$) are not supported yet")]
    Unsupported,
    /// The text couldn't be parsed in the pattern's language (only
    /// [`Pattern::find_in_text`] parses).
    #[error("the text could not be parsed")]
    ParseFailed,
}

/// How many file nodes are visited between asks of the stop callback.
const STOP_EVERY: usize = 1024;

impl Pattern {
    /// Every place the pattern matches in `tree`, a parse of `source` in the
    /// pattern's language (by [`fe_syntax::parse_str`]), in tree order:
    /// outer matches before the ones inside them, which are reported only
    /// with [`MatchOptions::nested`]. `stop` is asked before the first node
    /// and after every 1024 nodes; once it says `true` the search
    /// ends with [`Stopped::Cancelled`].
    pub fn find_all(
        &self,
        source: &str,
        tree: &Tree,
        opts: &MatchOptions,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<Match>, Stopped> {
        if self.has_multi() {
            return Err(Stopped::Unsupported);
        }
        let root = self.root();
        let mut matcher = Matcher {
            source,
            bindings: Vec::new(),
            cursor: tree.walk(),
        };
        let mut found = Vec::new();
        let mut cursor = tree.walk();
        let mut visited = 0usize;
        loop {
            if visited.is_multiple_of(STOP_EVERY) && stop() {
                return Err(Stopped::Cancelled);
            }
            visited += 1;
            let node = cursor.node();
            let matched = is_candidate(root, node) && matcher.matches_at(root, node);
            if matched {
                found.push(Match {
                    range: node.byte_range(),
                    captures: self.captures(&matcher.bindings),
                });
            }
            if (!matched || opts.nested) && cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return Ok(found);
                }
            }
        }
    }

    /// Parses `text` in the pattern's language and finds every outermost
    /// match in it: [`find_all`](Self::find_all) with default options and no
    /// stop.
    pub fn find_in_text(&self, text: &str) -> Result<Vec<Match>, Stopped> {
        self.find_in_text_with(text, &MatchOptions::default(), &mut || false)
    }

    /// Parses `text` in the pattern's language and finds the matches in it.
    /// `stop` is asked during the parse as well as the search.
    pub fn find_in_text_with(
        &self,
        text: &str,
        opts: &MatchOptions,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<Match>, Stopped> {
        if self.has_multi() {
            return Err(Stopped::Unsupported);
        }
        let mut stopped = false;
        let tree = fe_syntax::parse_str(self.language(), text, || {
            stopped = stop();
            stopped
        });
        let Some(tree) = tree else {
            return Err(if stopped {
                Stopped::Cancelled
            } else {
                Stopped::ParseFailed
            });
        };
        self.find_all(text, &tree, opts, stop)
    }

    fn has_multi(&self) -> bool {
        self.metavariables().iter().any(|m| m.multi)
    }

    /// The named metavariables' bindings, in the pattern's order.
    fn captures(&self, bindings: &[(&str, Range<usize>)]) -> Captures {
        let mut captures = Captures::default();
        for metavar in self.metavariables().iter().filter(|m| !m.anonymous) {
            if let Some((_, range)) = bindings.iter().find(|(name, _)| *name == metavar.name) {
                captures.push(&metavar.name, vec![range.clone()]);
            }
        }
        captures
    }
}

/// Whether `node` is worth comparing with the pattern root: of its kind, or,
/// for a pattern that is one wildcard, any named node that is no comment.
fn is_candidate(root: &PatternNode, node: Node<'_>) -> bool {
    if root.wildcard.is_some() {
        node.is_named() && !node.is_extra() && !node.is_missing()
    } else {
        same_kind(root, node)
    }
}

/// Same named-ness and kind. The id is compared first; the name too, since
/// a grammar can give one kind name several ids (through aliases).
fn same_kind(pattern: &PatternNode, node: Node<'_>) -> bool {
    pattern.named == node.is_named()
        && (pattern.kind_id == node.kind_id() || *pattern.kind == *node.kind())
}

/// A pattern node that is trivia. [`PatternNode`] doesn't record
/// [`Node::is_extra`], so a comment in the pattern is known by its kind
/// name: every grammar in the tree names its comment kinds `*comment*`.
fn is_pattern_trivia(pattern: &PatternNode) -> bool {
    pattern.wildcard.is_none() && pattern.kind.contains("comment")
}

struct Matcher<'s, 'p, 't> {
    source: &'s str,
    /// Named metavariables bound so far in this attempt.
    bindings: Vec<(&'p str, Range<usize>)>,
    /// Walks the candidate's subtree; reset for each candidate.
    cursor: TreeCursor<'t>,
}

impl<'p, 't> Matcher<'_, 'p, 't> {
    fn matches_at(&mut self, pattern: &'p PatternNode, node: Node<'t>) -> bool {
        self.bindings.clear();
        self.cursor.reset(node);
        self.matches(pattern)
    }

    /// Whether the node at the cursor matches `pattern`. On `true` the
    /// cursor is back at that node; on `false` it is anywhere below it.
    fn matches(&mut self, pattern: &'p PatternNode) -> bool {
        let node = self.cursor.node();
        if node.is_missing() {
            // Zero-width, inserted by error recovery: not text that is there.
            return false;
        }
        if let Some(wildcard) = &pattern.wildcard {
            if wildcard.anonymous {
                return true;
            }
            let range = node.byte_range();
            if let Some((_, bound)) = self
                .bindings
                .iter()
                .find(|(name, _)| *name == wildcard.name)
            {
                return self.source.get(bound.clone()) == self.source.get(range);
            }
            self.bindings.push((&wildcard.name, range));
            return true;
        }
        if !same_kind(pattern, node) {
            return false;
        }
        if pattern.named
            && let Some(text) = pattern.text.as_deref()
            && self.source.get(node.byte_range()) != Some(text)
        {
            return false;
        }
        self.children_match(&pattern.children)
    }

    /// Whether the cursor node's children, extras skipped, match `pattern`'s
    /// pairwise, trivia skipped, and are as many.
    fn children_match(&mut self, children: &'p [PatternNode]) -> bool {
        let mut wanted = children.iter().filter(|child| !is_pattern_trivia(child));
        if self.cursor.goto_first_child() {
            loop {
                if !self.cursor.node().is_extra() {
                    let Some(child) = wanted.next() else {
                        return false;
                    };
                    if !self.matches(child) {
                        return false;
                    }
                }
                if !self.cursor.goto_next_sibling() {
                    break;
                }
            }
            self.cursor.goto_parent();
        }
        wanted.next().is_none()
    }
}
