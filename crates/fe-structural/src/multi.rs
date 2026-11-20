//! Multi wildcards (`$$$`): a run of zero or more siblings, found by
//! backtracking.
//!
//! A child list is matched pattern child by child against the node's
//! children, trivia and extras skipped. A multi wildcard takes as few
//! siblings as it can (lazily), more only when the rest of the pattern
//! doesn't match; two in a row are allowed, the first the lazier. A run may
//! hold anything, separators included, but no MISSING node.
//!
//! The rest of the pattern is passed down as a continuation ([`Cont`]): a
//! list's choice stands only if everything after it, in this list and in
//! the lists around it, matches too. So in `foo($$$A, $$$B) + bar($$$A)` a
//! mismatch at `bar` makes `foo`'s list try its next split. A subtree with
//! no multi wildcard has one way to match at most and is compared directly.
//!
//! Polynomial time: a failed state, `(pattern list, file node, pattern
//! child i, sibling j)` with the bindings the rest of the pattern compares
//! against, is remembered for the candidate. Where the list's node and the
//! pattern's lists sit fixes what comes after a state, so with no such
//! binding a candidate takes O(p·n²) steps for p pattern nodes over n
//! siblings. A repeated name whose first binding can vary (it follows a
//! multi wildcard in a list holding two or more) multiplies that by the
//! ranges it can take; [`varying_repeat`] limits a pattern to one such
//! name, since with several the problem is NP-hard. A later occurrence of a
//! bound multi takes only the run with its text's length.
//!
//! A multi wildcard's capture is one range, from the start of its first
//! sibling to the end of its last, separators and extras between them
//! included. An empty run's range is empty, at the end of the sibling
//! before it (else the start of the one after it, else the parent's start).

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};

use tree_sitter::Node;

use crate::{
    matcher::{Matcher, is_pattern_trivia, leaf_text_matches, same_kind},
    pattern::{PatternNode, Wildcard},
};

/// The multi wildcard `pattern` stands for in a child list: itself, or the
/// one a wrapper with no other child holds (as a Python statement does,
/// `def f(): $$$BODY` putting it in an `expression_statement`).
pub(crate) fn as_multi(pattern: &PatternNode) -> Option<&Wildcard> {
    if let Some(wildcard) = &pattern.wildcard {
        return wildcard.multi.then_some(wildcard);
    }
    if pattern.extra {
        return None;
    }
    let mut children = wanted(pattern);
    match (children.next(), children.next()) {
        (Some(only), None) => as_multi(only),
        _ => None,
    }
}

/// `pattern`'s children that are compared: trivia skipped.
fn wanted(pattern: &PatternNode) -> impl Iterator<Item = &PatternNode> {
    pattern.children.iter().filter(|c| !is_pattern_trivia(c))
}

/// The second name, in pattern order, that is used more than once and
/// whose first occurrence can bind more than one way: after the first
/// multi wildcard of a list holding two or more, at any depth. `None` if
/// the pattern has at most one such name.
pub(crate) fn varying_repeat(root: &PatternNode) -> Option<&str> {
    fn walk<'p>(pattern: &'p PatternNode, varying: bool, out: &mut Vec<(&'p str, bool)>) {
        if let Some(wildcard) = &pattern.wildcard {
            if !wildcard.anonymous {
                out.push((&wildcard.name, varying));
            }
            return;
        }
        let children: Vec<_> = wanted(pattern).collect();
        let multis: Vec<_> = children.iter().map(|c| as_multi(c)).collect();
        let first = multis.iter().position(Option::is_some);
        let several = multis.iter().flatten().count() >= 2;
        for (i, child) in children.into_iter().enumerate() {
            let varying = varying || (several && first.is_some_and(|f| i >= f));
            match multis[i] {
                Some(wildcard) if !wildcard.anonymous => out.push((&wildcard.name, varying)),
                Some(_) => {}
                None => walk(child, varying, out),
            }
        }
    }
    let mut occurrences = Vec::new();
    walk(root, false, &mut occurrences);
    let mut seen: Vec<(&str, bool, usize)> = Vec::new();
    for (name, varying) in occurrences {
        match seen.iter_mut().find(|(n, ..)| *n == name) {
            Some((_, _, count)) => *count += 1,
            None => seen.push((name, varying, 1)),
        }
    }
    seen.iter()
        .filter(|(_, varying, count)| *varying && *count > 1)
        .nth(1)
        .map(|(name, ..)| *name)
}

/// What the search knows of the pattern, and the failures it has seen in
/// the current candidate.
pub(crate) struct Search<'p> {
    /// Each pattern node's pre-order index, the index after its subtree,
    /// and whether a multi wildcard is at or below it. By address: the
    /// pattern is borrowed for the whole search.
    nodes: HashMap<*const PatternNode, (usize, usize, bool)>,
    /// Each named metavariable's first and last occurrence, as pre-order
    /// indexes.
    names: Vec<(&'p str, usize, usize)>,
    failed: HashSet<Key>,
}

/// A failed state: pattern list, file node, `i`, `j`, and the ranges bound
/// to the names live there.
type Key = (usize, usize, usize, usize, Vec<Option<Range<usize>>>);

impl<'p> Search<'p> {
    pub(crate) fn new(root: &'p PatternNode) -> Search<'p> {
        fn walk<'p>(pattern: &'p PatternNode, next: &mut usize, search: &mut Search<'p>) -> bool {
            let pre = *next;
            *next += 1;
            let mut multi = false;
            if let Some(wildcard) = &pattern.wildcard {
                multi = wildcard.multi;
                if !wildcard.anonymous {
                    match search.names.iter_mut().find(|(n, ..)| *n == wildcard.name) {
                        Some((_, _, last)) => *last = pre,
                        None => search.names.push((&wildcard.name, pre, pre)),
                    }
                }
            }
            for child in &pattern.children {
                multi |= walk(child, next, search);
            }
            search
                .nodes
                .insert(std::ptr::from_ref(pattern), (pre, *next, multi));
            multi
        }
        let mut search = Search {
            nodes: HashMap::new(),
            names: Vec::new(),
            failed: HashSet::new(),
        };
        walk(root, &mut 0, &mut search);
        search
    }

    /// Forgets the failures: a new candidate.
    pub(crate) fn reset(&mut self) {
        if !self.failed.is_empty() {
            self.failed.clear();
        }
    }

    pub(crate) fn has_multi(&self, pattern: &PatternNode) -> bool {
        self.nodes
            .get(&std::ptr::from_ref(pattern))
            .is_some_and(|&(.., multi)| multi)
    }

    /// The pre-order index where the rest of the pattern starts at child
    /// `i` of `list`'s pattern node.
    fn position(&self, list: &List<'p, '_>, i: usize) -> usize {
        match list.wanted.get(i) {
            Some(child) => self.nodes[&std::ptr::from_ref(*child)].0,
            None => self.nodes[&std::ptr::from_ref(list.pattern)].1,
        }
    }
}

/// What remains to match after a child list's current choice: nothing (the
/// candidate matched), or the rest of an enclosing list.
pub(crate) enum Cont<'c, 'p, 't> {
    Done,
    Next {
        list: &'c List<'p, 't>,
        i: usize,
        j: usize,
        then: &'c Cont<'c, 'p, 't>,
    },
}

/// Matches what `then` says remains.
pub(crate) fn resume<'p, 't>(m: &mut Matcher<'_, 'p, 't, '_>, then: &Cont<'_, 'p, 't>) -> bool {
    match then {
        Cont::Done => true,
        Cont::Next { list, i, j, then } => list.from(m, *i, *j, then),
    }
}

/// Whether `node` matches `pattern`, which has a multi wildcard at or
/// below it, with `then` matching after it.
pub(crate) fn node<'p, 't>(
    m: &mut Matcher<'_, 'p, 't, '_>,
    pattern: &'p PatternNode,
    node: Node<'t>,
    then: &Cont<'_, 'p, 't>,
) -> bool {
    if m.tick() || node.is_missing() {
        return false;
    }
    if let Some(wildcard) = &pattern.wildcard {
        // A multi one standing for one node: the pattern root.
        return m.bind(wildcard, &[node], node.byte_range()) && resume(m, then);
    }
    if !same_kind(pattern, node) || !leaf_text_matches(pattern, node, m.source) {
        return false;
    }
    let Some(list) = List::new(m, pattern, node) else {
        return false;
    };
    list.from(m, 0, 0, then)
}

/// A pattern node's children against a file node's.
pub(crate) struct List<'p, 't> {
    pattern: &'p PatternNode,
    /// The pattern's children, trivia skipped.
    wanted: Vec<&'p PatternNode>,
    /// Each one's multi wildcard, if it is one.
    multis: Vec<Option<&'p Wildcard>>,
    /// How many of `wanted[i..]` aren't multi: siblings a run must leave.
    fixed_after: Vec<usize>,
    /// The file node's id and start.
    id: usize,
    start: usize,
    /// Its children, extras skipped.
    nodes: Vec<Node<'t>>,
}

impl<'p, 't> List<'p, 't> {
    /// `None` if the search was cancelled while reading the children.
    fn new(
        m: &mut Matcher<'_, 'p, 't, '_>,
        pattern: &'p PatternNode,
        node: Node<'t>,
    ) -> Option<Self> {
        let wanted: Vec<_> = wanted(pattern).collect();
        let multis: Vec<_> = wanted.iter().map(|c| as_multi(c)).collect();
        let mut fixed_after = vec![0; wanted.len() + 1];
        for i in (0..wanted.len()).rev() {
            fixed_after[i] = fixed_after[i + 1] + usize::from(multis[i].is_none());
        }
        let mut nodes = Vec::new();
        m.cursor.reset(node);
        if m.cursor.goto_first_child() {
            loop {
                if m.tick() {
                    return None;
                }
                let child = m.cursor.node();
                if !child.is_extra() {
                    nodes.push(child);
                }
                if !m.cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        Some(List {
            pattern,
            wanted,
            multis,
            fixed_after,
            id: node.id(),
            start: node.start_byte(),
            nodes,
        })
    }

    /// Whether `wanted[i..]` matches `nodes[j..]` with `then` after it. On
    /// `false` the bindings are as they were.
    fn from(
        &self,
        m: &mut Matcher<'_, 'p, 't, '_>,
        i: usize,
        j: usize,
        then: &Cont<'_, 'p, 't>,
    ) -> bool {
        if m.cancelled {
            return false;
        }
        let n = self.nodes.len();
        if n - j < self.fixed_after[i] {
            return false;
        }
        let key = self.key(m, i, j);
        if m.search.failed.contains(&key) {
            return false;
        }
        let saved = m.bindings.len();
        let matched = if i == self.wanted.len() {
            // Siblings left over are fine only where the pattern's text
            // ended inside the node.
            (j == n || self.pattern.open_end) && resume(m, then)
        } else {
            match self.multis[i] {
                Some(wildcard) => self.run(m, wildcard, i, j, then),
                None => {
                    let next = Cont::Next {
                        list: self,
                        i: i + 1,
                        j: j + 1,
                        then,
                    };
                    m.node(self.wanted[i], self.nodes[j], &next)
                }
            }
        };
        if !matched {
            m.bindings.truncate(saved);
            if !m.cancelled {
                m.search.failed.insert(key);
            }
        }
        matched
    }

    /// The state's key: the bindings of names used both before and after
    /// it, the only ones the rest can depend on.
    fn key(&self, m: &Matcher<'_, 'p, 't, '_>, i: usize, j: usize) -> Key {
        let at = m.search.position(self, i);
        let live = m
            .search
            .names
            .iter()
            .filter(|(_, first, last)| *first < at && at <= *last)
            .map(|(name, ..)| m.bound(name))
            .collect();
        (
            std::ptr::from_ref(self.pattern) as usize,
            self.id,
            i,
            j,
            live,
        )
    }

    /// Whether the multi wildcard `wanted[i]` can take a run from `j` with
    /// the rest matching after it: shortest runs first.
    fn run(
        &self,
        m: &mut Matcher<'_, 'p, 't, '_>,
        wildcard: &'p Wildcard,
        i: usize,
        j: usize,
        then: &Cont<'_, 'p, 't>,
    ) -> bool {
        let n = self.nodes.len();
        let mut last = n - self.fixed_after[i + 1];
        // The list's last child takes everything left.
        let mut first = if i + 1 == self.wanted.len() { last } else { j };
        let bound = if wildcard.anonymous {
            None
        } else {
            m.bound(&wildcard.name)
        };
        if let Some(bound) = bound {
            // Bound already: only the run as long as its text can match.
            let Some(k) = self.run_end(j, bound.len()) else {
                return false;
            };
            if k < first || k > last {
                return false;
            }
            (first, last) = (k, k);
        }
        if self.nodes[j..first].iter().any(|node| node.is_missing()) {
            return false;
        }
        let saved = m.bindings.len();
        for k in first..=last {
            if k > first && self.nodes[k - 1].is_missing() {
                return false;
            }
            if m.tick() {
                return false;
            }
            let range = self.range(j, k);
            if m.bind(wildcard, &self.nodes[j..k], range) && self.from(m, i + 1, k, then) {
                return true;
            }
            m.bindings.truncate(saved);
        }
        false
    }

    /// Where a run from `j` covering `len` bytes ends: the `k` with
    /// `nodes[j..k]` that long.
    fn run_end(&self, j: usize, len: usize) -> Option<usize> {
        if len == 0 {
            return Some(j);
        }
        let end = self.nodes.get(j)?.start_byte() + len;
        let rest = &self.nodes[j..];
        let k = rest.partition_point(|node| node.end_byte() < end);
        (rest.get(k)?.end_byte() == end).then_some(j + k + 1)
    }

    /// The bytes `nodes[j..k]` cover; empty, at the insertion point, for
    /// an empty run.
    fn range(&self, j: usize, k: usize) -> Range<usize> {
        if k > j {
            return self.nodes[j].start_byte()..self.nodes[k - 1].end_byte();
        }
        let at = match (j.checked_sub(1), self.nodes.get(j)) {
            (Some(before), _) => self.nodes[before].end_byte(),
            (None, Some(after)) => after.start_byte(),
            (None, None) => self.start,
        };
        at..at
    }
}
