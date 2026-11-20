//! Matching a compiled [`Pattern`] against a file's syntax tree.
//!
//! The file tree is walked depth first; each node of the pattern root's kind
//! is compared with the pattern tree. Two nodes match when their kinds are
//! the same and, for a named leaf, their text; an anonymous token matches by
//! kind alone. Children are compared pairwise in order, extras (comments)
//! skipped on both sides, and a different number of them is a mismatch.
//! Field names are not compared. A wildcard matches any one node; a named
//! one binds it, and a later occurrence of the same name must cover the same
//! source text. A multi wildcard (`$$$`) matches a run of siblings, by
//! backtracking (see [`crate::multi`]). A `where` section's constraints are
//! checked as metavariables bind and, for `inside`, once a node matches.

use std::ops::Range;

use tree_sitter::{Node, Tree, TreeCursor};

use crate::{
    captures::Captures,
    constraints::Constraints,
    multi::{self, Cont, Search},
    pattern::{Pattern, PatternNode, Wildcard},
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
    /// The text couldn't be parsed in the pattern's language (only
    /// [`Pattern::find_in_text`] parses).
    #[error("the text could not be parsed")]
    ParseFailed,
}

/// How many steps (file nodes visited, by the walk or by a comparison, and
/// runs a multi wildcard tries) are taken between asks of the stop callback.
const STOP_EVERY: usize = 1024;

impl Pattern {
    /// Every place the pattern matches in `tree`, a parse of `source` in the
    /// pattern's language (by [`fe_syntax::parse_str`]), in tree order:
    /// outer matches before the ones inside them, which are reported only
    /// with [`MatchOptions::nested`]. `stop` is asked before the first node
    /// and after every 1024 steps, comparisons and backtracking included;
    /// once it says `true` the search ends with [`Stopped::Cancelled`].
    pub fn find_all(
        &self,
        source: &str,
        tree: &Tree,
        opts: &MatchOptions,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<Match>, Stopped> {
        let root = self.root();
        let constraints = self.constraints();
        let mut matcher = Matcher {
            source,
            bindings: Vec::new(),
            cursor: tree.walk(),
            constraints,
            search: Search::new(root),
            stop,
            steps: 0,
            cancelled: false,
        };
        let mut found = Vec::new();
        let mut cursor = tree.walk();
        loop {
            if matcher.tick() {
                return Err(Stopped::Cancelled);
            }
            let node = cursor.node();
            let matched = is_candidate(root, node)
                && matcher.matches_at(root, node)
                && (!constraints.has_place() || constraints.allows_place(node, source));
            if matcher.cancelled {
                return Err(Stopped::Cancelled);
            }
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
pub(crate) fn same_kind(pattern: &PatternNode, node: Node<'_>) -> bool {
    pattern.named == node.is_named()
        && (pattern.kind_id == node.kind_id() || *pattern.kind == *node.kind())
}

/// A pattern node that is trivia: an extra of the grammar, as the pattern's
/// parse marked it, skipped as the file's extras are.
pub(crate) fn is_pattern_trivia(pattern: &PatternNode) -> bool {
    pattern.wildcard.is_none() && pattern.extra
}

pub(crate) struct Matcher<'s, 'p, 't, 'f> {
    pub(crate) source: &'s str,
    /// Named metavariables bound so far in this attempt.
    pub(crate) bindings: Vec<(&'p str, Range<usize>)>,
    /// Walks a subtree being compared.
    pub(crate) cursor: TreeCursor<'t>,
    pub(crate) constraints: &'p Constraints,
    /// The backtracking search's pattern facts and failure memo.
    pub(crate) search: Search<'p>,
    stop: &'f mut dyn FnMut() -> bool,
    /// Steps taken, for asking `stop` every [`STOP_EVERY`].
    steps: usize,
    /// `stop` said to stop: every comparison fails from then on.
    pub(crate) cancelled: bool,
}

impl<'p, 't> Matcher<'_, 'p, 't, '_> {
    /// Takes a step, asking `stop` every [`STOP_EVERY`]: whether the search
    /// is cancelled.
    pub(crate) fn tick(&mut self) -> bool {
        if self.cancelled {
            return true;
        }
        if self.steps.is_multiple_of(STOP_EVERY) && (self.stop)() {
            self.cancelled = true;
        }
        self.steps += 1;
        self.cancelled
    }

    fn matches_at(&mut self, pattern: &'p PatternNode, node: Node<'t>) -> bool {
        self.bindings.clear();
        self.search.reset();
        self.node(pattern, node, &Cont::Done)
    }

    /// Whether `node` matches `pattern` with `then`, the rest of the
    /// pattern, matching after it. On `false` the bindings are as they were.
    pub(crate) fn node(
        &mut self,
        pattern: &'p PatternNode,
        node: Node<'t>,
        then: &Cont<'_, 'p, 't>,
    ) -> bool {
        let saved = self.bindings.len();
        let matched = if self.search.has_multi(pattern) {
            multi::node(self, pattern, node, then)
        } else {
            // No multi wildcard below: one way to match at most.
            self.cursor.reset(node);
            self.matches(pattern) && multi::resume(self, then)
        };
        if !matched {
            self.bindings.truncate(saved);
        }
        matched
    }

    /// Binds a named metavariable to `nodes` covering `range`, or, bound
    /// already, compares the text: whether it may.
    pub(crate) fn bind(
        &mut self,
        wildcard: &'p Wildcard,
        nodes: &[Node<'t>],
        range: Range<usize>,
    ) -> bool {
        if wildcard.anonymous {
            return true;
        }
        if let Some(bound) = self.bound(&wildcard.name) {
            return self.source.get(bound) == self.source.get(range);
        }
        if !self
            .constraints
            .allows(&wildcard.name, wildcard.multi, nodes, &range, self.source)
        {
            return false;
        }
        self.bindings.push((&wildcard.name, range));
        true
    }

    /// What `name` is bound to, if it is.
    pub(crate) fn bound(&self, name: &str) -> Option<Range<usize>> {
        self.bindings
            .iter()
            .find(|(bound, _)| *bound == name)
            .map(|(_, range)| range.clone())
    }

    /// Whether the node at the cursor matches `pattern`, which holds no
    /// multi wildcard. On `true` the cursor is back at that node; on `false`
    /// it is anywhere below it, and bindings may have been added.
    fn matches(&mut self, pattern: &'p PatternNode) -> bool {
        if self.tick() {
            return false;
        }
        let node = self.cursor.node();
        if node.is_missing() {
            // Zero-width, inserted by error recovery: not text that is there.
            return false;
        }
        if let Some(wildcard) = &pattern.wildcard {
            return self.bind(wildcard, &[node], node.byte_range());
        }
        if !same_kind(pattern, node) || !leaf_text_matches(pattern, node, self.source) {
            return false;
        }
        self.children_match(pattern)
    }

    /// Whether the cursor node's children, extras skipped, match `pattern`'s
    /// pairwise, trivia skipped, and are as many (or more, for a pattern
    /// node whose text ended before its parse did).
    fn children_match(&mut self, pattern: &'p PatternNode) -> bool {
        let mut wanted = pattern
            .children
            .iter()
            .filter(|child| !is_pattern_trivia(child));
        if self.cursor.goto_first_child() {
            loop {
                if self.tick() {
                    return false;
                }
                if !self.cursor.node().is_extra() {
                    let Some(child) = wanted.next() else {
                        let more = pattern.open_end;
                        self.cursor.goto_parent();
                        return more;
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

/// For a named leaf, whether `node` has its text.
pub(crate) fn leaf_text_matches(pattern: &PatternNode, node: Node<'_>, source: &str) -> bool {
    match pattern.text.as_deref() {
        Some(text) if pattern.named => source.get(node.byte_range()) == Some(text),
        _ => true,
    }
}
