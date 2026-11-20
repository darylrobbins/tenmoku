//! An ast-grep-style pattern compiled into an owned tree for one language.
//!
//! A pattern is code in the language with metavariables in it: `$NAME`
//! stands for one node, `$$$NAME` for any number of sibling nodes, `$_` and
//! `$$$` for the same without a name, and `$$` for a literal `$`. NAME is
//! `[A-Z_][A-Z0-9_]*`, as in ast-grep. Each metavariable is replaced by a
//! placeholder token the grammar can parse (an identifier, else a string,
//! else a number), the text is parsed, and the tree is copied out with the
//! placeholders marked as wildcards. A `where` section after the code
//! constrains what the metavariables match (see `constraints.rs`).

use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use fe_syntax::Language;
use tree_sitter::{Node, Tree};

use crate::{
    constraints::{self, Constraints},
    error::{PatternError, PatternErrorKind},
    multi,
};

/// A metavariable of a pattern, by name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Metavar {
    /// Its name without the `$`s. An anonymous one gets a unique numbered
    /// name of its own, `_any0`, `_any1`, … in pattern order: lowercase, so
    /// no `$NAME` can spell it.
    pub name: String,
    /// `$$$NAME`: it stands for any number of sibling nodes.
    pub multi: bool,
    /// `$_`, `$_NAME` or `$$$` (any name starting with `_`, as in ast-grep):
    /// what it matches isn't captured, and two of them needn't match the
    /// same text. Each occurrence is a metavariable of its own.
    pub anonymous: bool,
}

/// A wildcard in the pattern tree: where a metavariable stood.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Wildcard {
    /// The metavariable's [name](Metavar::name).
    pub name: String,
    /// `$$$NAME`: any number of sibling nodes.
    pub multi: bool,
    /// `$_`, `$_NAME` or `$$$`.
    pub anonymous: bool,
}

/// One node of a compiled pattern, owned: no borrow of a tree or its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternNode {
    /// The grammar's kind id, as [`Node::kind_id`] gives it.
    pub kind_id: u16,
    /// The kind's name, as [`Node::kind`] gives it. Shared by every node
    /// of the kind in one pattern.
    pub kind: Arc<str>,
    /// Whether the kind is named (not punctuation or a keyword token).
    pub named: bool,
    /// The field it fills in its parent, if any.
    pub field: Option<Arc<str>>,
    /// An extra of the grammar (a comment, as [`Node::is_extra`] says):
    /// skipped when matching, as the file's extras are.
    pub extra: bool,
    /// The pattern's text ended inside this node: the parse needed a
    /// suffix (`;`, a newline) the pattern doesn't hold, and the children
    /// that are only suffix were left out. A file node may have more
    /// children after the ones the pattern gives.
    pub open_end: bool,
    /// The text of a leaf, as the pattern spells it (`$$` already a `$`).
    /// `None` for a node with children and for a wildcard.
    pub text: Option<Box<str>>,
    /// Its children in order, named and anonymous, extras included. Empty
    /// for a leaf and a wildcard.
    pub children: Vec<PatternNode>,
    /// `Some` where a metavariable stood: the node matches any node (or, for
    /// a multi one, any run of siblings). Its kind is the placeholder's.
    pub wildcard: Option<Wildcard>,
}

impl PatternNode {
    fn collect_literals<'a>(&'a self, out: &mut Vec<&'a str>) {
        if self.wildcard.is_some() {
            return;
        }
        if let Some(text) = self.text.as_deref() {
            if !text.is_empty() {
                out.push(text);
            }
            return;
        }
        for child in &self.children {
            child.collect_literals(out);
        }
    }
}

/// A pattern compiled for one language: an owned tree, `Send + Sync`, cheap
/// to share between scan threads behind an `Arc`.
#[derive(Clone, Debug)]
pub struct Pattern {
    language: &'static Language,
    source: Box<str>,
    root: PatternNode,
    metavars: Vec<Metavar>,
    constraints: Constraints,
}

impl Pattern {
    /// Compiles `source`, an ast-grep-style pattern, for `language`.
    ///
    /// The code may be followed by a `where` section: a line holding only
    /// `where`, then one constraint per line, all of which a match meets:
    ///
    /// - `$NAME: kind=<kind>`: the node is of that kind (each named node of
    ///   the run, for `$$$NAME`).
    /// - `$NAME: regex=<re>`: the regular expression (unanchored) matches
    ///   in the text the metavariable covers.
    /// - `$NAME: text=<text>`: that text is exactly `<text>`.
    /// - `$NAME: not kind=<kind>` (or `not regex=`, `not text=`): the
    ///   opposite.
    /// - `inside: kind=<kind>`, optionally with ` name=<text>`: an ancestor
    ///   of the matched node is of that kind (and its `name` field's text is
    ///   `<text>`); `not inside: …`, no ancestor is.
    ///
    /// ```text
    /// $X.unwrap()
    /// where
    /// $X: kind=identifier
    /// not inside: kind=function_item name=test
    /// ```
    ///
    /// A constraint on a name the pattern doesn't bind (or on `$_`), an
    /// unknown key and a bad regex are errors. Offsets in a [`PatternError`]
    /// are into the whole of `source`.
    pub fn compile(language: &'static Language, source: &str) -> Result<Pattern, PatternError> {
        let (code, section) = constraints::split(source);
        let mut pattern = Self::compile_code(language, code)?;
        if let Some((at, section)) = section {
            pattern.constraints = Constraints::parse(section, at, &pattern.metavars)?;
        }
        pattern.source = source.into();
        Ok(pattern)
    }

    /// Compiles the pattern's code, without a `where` section.
    fn compile_code(language: &'static Language, source: &str) -> Result<Pattern, PatternError> {
        let scan = scan(source)?;
        let Some(first) = source.find(|c: char| !c.is_whitespace()) else {
            return Err(PatternError {
                kind: PatternErrorKind::Empty,
                offset: 0,
            });
        };
        let tag = placeholder_tag(source);
        let mut tried = HashSet::new();
        let mut first_failure: Option<PatternError> = None;
        // `Err(None)`: tried already.
        let mut attempt_choice = |choice: &[usize]| {
            if !tried.insert(choice.to_vec()) {
                return Ok(Err(None));
            }
            let outcome = try_choice(language, source, &scan, &tag, choice, first)?;
            Ok(outcome.map_err(|failed| {
                log::trace!("pattern attempt {choice:?} failed: {}", failed.error);
                first_failure.get_or_insert_with(|| failed.error.clone());
                Some(failed)
            }))
        };
        // Guided first: the spelling of the hole that is no node, or the
        // one nearest the error, moves on.
        let mut choice = vec![0usize; scan.holes.len()];
        loop {
            let failed = match attempt_choice(&choice)? {
                Ok(pattern) => return check_repeats(pattern, &scan),
                Err(Some(failed)) => failed,
                Err(None) => break,
            };
            match blame(&failed.holes, &failed.clean, failed.at.as_ref(), &choice) {
                Some(hole) => choice[hole] += 1,
                None => break,
            }
        }
        // Then every other combination, up to a limit: error recovery can
        // blame a hole for another's spelling, so the guided walk can miss
        // one that parses.
        let mut choice = vec![0usize; scan.holes.len()];
        for _ in 0..MAX_COMBINATIONS {
            if let Ok(pattern) = attempt_choice(&choice)? {
                return check_repeats(pattern, &scan);
            }
            // The next combination, the first hole counting fastest.
            let Some(hole) = choice.iter().position(|&c| c + 1 < CANDIDATES) else {
                break;
            };
            choice[hole] += 1;
            choice[..hole].fill(0);
        }
        Err(first_failure.unwrap_or(PatternError {
            kind: PatternErrorKind::Syntax,
            offset: first,
        }))
    }

    /// The language the pattern was compiled for.
    pub fn language(&self) -> &'static Language {
        self.language
    }

    /// The pattern as the user wrote it.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The root of the pattern tree: the innermost node that covers all of
    /// the pattern's text, not the file or statement wrapping it.
    pub fn root(&self) -> &PatternNode {
        &self.root
    }

    /// The root's kind name, `call_expression` for `foo($A)` in Rust.
    pub fn root_kind(&self) -> &str {
        &self.root.kind
    }

    /// The metavariables by name, in order of first appearance, anonymous
    /// ones included (each under its numbered name).
    pub fn metavariables(&self) -> &[Metavar] {
        &self.metavars
    }

    /// The text of every leaf of the pattern that isn't a wildcard, in order:
    /// what a file must contain for the pattern to match there.
    pub fn literals(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.root.collect_literals(&mut out);
        out
    }

    /// The `where` section's constraints (none without one).
    pub(crate) fn constraints(&self) -> &Constraints {
        &self.constraints
    }
}

/// How many placeholder combinations are parsed after the guided walk
/// gives up: every one for up to five metavariables.
const MAX_COMBINATIONS: usize = 243;

/// A combination of placeholder spellings that didn't parse cleanly.
struct Failed {
    /// What the first suffix's attempt says is wrong, in source terms.
    error: PatternError,
    /// Each hole's placeholder bytes in the built text.
    holes: Vec<Range<usize>>,
    /// Which holes parsed as a node of their own.
    clean: Vec<bool>,
    /// The first syntax error's bytes in the built text, if any.
    at: Option<Range<usize>>,
}

/// Parses the source with each hole spelled as `choice` says, with each
/// suffix in turn: the pattern, what went wrong, or an error if the
/// language couldn't parse at all.
fn try_choice(
    language: &'static Language,
    source: &str,
    scan: &Scan,
    tag: &str,
    choice: &[usize],
    first: usize,
) -> Result<Result<Pattern, Failed>, PatternError> {
    let mut built = build(source, scan, choice, tag);
    // What the first suffix's attempt found, for blame.
    let mut verdict = None;
    for suffix in SUFFIXES {
        let parsed = attempt(language, &mut built, suffix).ok_or(PatternError {
            kind: PatternErrorKind::ParseFailed,
            offset: first,
        })?;
        match parsed {
            Ok(tree) => return Ok(Ok(finish(language, source, scan, &built, &tree))),
            Err(found) => {
                verdict.get_or_insert(found);
            }
        }
    }
    let Some((clean, at)) = verdict else {
        unreachable!("SUFFIXES is not empty");
    };
    let error = match at {
        Some(ref range) => PatternError {
            kind: PatternErrorKind::Syntax,
            offset: built.source_offset(range.start),
        },
        None => {
            let hole = clean.iter().position(|&c| !c).unwrap_or(0);
            PatternError {
                kind: PatternErrorKind::MetavarNotANode,
                offset: scan.holes[hole].source.start,
            }
        }
    };
    Ok(Err(Failed {
        error,
        holes: built.holes,
        clean,
        at,
    }))
}

/// Refuses a pattern with more than one repeated name whose first match
/// can vary (see `multi.rs`): with several, matching is NP-hard. The error
/// is at the second such name's second occurrence.
fn check_repeats(pattern: Pattern, scan: &Scan) -> Result<Pattern, PatternError> {
    let Some(name) = multi::varying_repeat(&pattern.root) else {
        return Ok(pattern);
    };
    let offset = scan
        .holes
        .iter()
        .filter(|hole| scan.metavars[hole.metavar].name == name)
        .nth(1)
        .map_or(0, |hole| hole.source.start);
    Err(PatternError {
        kind: PatternErrorKind::VaryingRepeats,
        offset,
    })
}

/// What is tried after the pattern's text, in order, for a grammar that
/// wants it ended: nothing, a newline (Go's statements end at one, and
/// without it `fmt.Println(x)` parses as a conversion), then `;` (Rust's
/// expression statements). A suffix is never part of the pattern's span.
const SUFFIXES: [&str; 3] = ["", "\n", ";"];

/// Which holes are a node of their own, and the first error, if any.
type Verdict = (Vec<bool>, Option<Range<usize>>);

/// Parses `built`'s text with `suffix` after it: the tree if it has no error
/// and every hole is a node, else what was wrong. `None` if the parse
/// couldn't run. The suffix stays on `built.text` only with a tree.
fn attempt(language: &Language, built: &mut Built, suffix: &str) -> Option<Result<Tree, Verdict>> {
    let len = built.text.len();
    built.text.push_str(suffix);
    let Some(tree) = fe_syntax::parse_str(language, &built.text, || false) else {
        built.text.truncate(len);
        return None;
    };
    let root = tree.root_node();
    let clean: Vec<bool> = built
        .holes
        .iter()
        .map(|range| node_at(root, range).is_some())
        .collect();
    let error = first_error(root);
    if error.is_none() && clean.iter().all(|&c| c) {
        return Some(Ok(tree));
    }
    built.text.truncate(len);
    Some(Err((clean, error)))
}

/// A metavariable occurrence in the source.
#[derive(Debug)]
struct Hole {
    /// Its bytes in the source, `$`s included.
    source: Range<usize>,
    /// Index into [`Scan::metavars`].
    metavar: usize,
}

/// What [`scan`] found: the metavariables, each occurrence, and each `$$`.
#[derive(Debug, Default)]
struct Scan {
    metavars: Vec<Metavar>,
    holes: Vec<Hole>,
    /// Where `$$` stands for a literal `$`: the first byte of each.
    dollars: Vec<usize>,
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_uppercase() || b == b'_'
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'
}

/// Finds the metavariables in `source`. A `$` that starts none of them is
/// left as it is, as is a name run on into lowercase (`$Abc`).
fn scan(source: &str) -> Result<Scan, PatternError> {
    let bytes = source.as_bytes();
    let mut scan = Scan::default();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut anonymous = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'$' {
            i += 1;
            continue;
        }
        let start = i;
        let multi = bytes[i..].starts_with(b"$$$");
        if !multi && bytes[i..].starts_with(b"$$") {
            scan.dollars.push(start);
            i += 2;
            continue;
        }
        let name_start = start + if multi { 3 } else { 1 };
        let mut end = name_start;
        if end < bytes.len() && is_name_start(bytes[end]) {
            end += 1;
            while end < bytes.len() && is_name_byte(bytes[end]) {
                end += 1;
            }
        }
        let runs_on =
            end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] >= 0x80);
        if (end == name_start && !multi) || (end > name_start && runs_on) {
            // A lone `$`, or `$Abc`: literal text.
            i = if multi { start + 3 } else { start + 1 };
            continue;
        }
        let name = &source[name_start..end];
        // `$_`, `$_NAME` and `$$$` capture nothing, as in ast-grep: each is
        // a metavariable of its own.
        let metavar = if name.is_empty() || name.starts_with('_') {
            let name = format!("_any{anonymous}");
            anonymous += 1;
            scan.metavars.push(Metavar {
                name,
                multi,
                anonymous: true,
            });
            scan.metavars.len() - 1
        } else if let Some(&index) = by_name.get(name) {
            if scan.metavars[index].multi != multi {
                return Err(PatternError {
                    kind: PatternErrorKind::MixedMetavar,
                    offset: start,
                });
            }
            index
        } else {
            scan.metavars.push(Metavar {
                name: name.to_owned(),
                multi,
                anonymous: false,
            });
            by_name.insert(name.to_owned(), scan.metavars.len() - 1);
            scan.metavars.len() - 1
        };
        scan.holes.push(Hole {
            source: start..end,
            metavar,
        });
        i = end;
    }
    Ok(scan)
}

/// A tag that makes every placeholder text new to `source`: empty unless the
/// source already holds `__tsMV`, else the first number that it doesn't
/// hold after `__tsMV`.
fn placeholder_tag(source: &str) -> String {
    if !source.contains("__tsMV") {
        return String::new();
    }
    (0usize..)
        .map(|n| n.to_string())
        .find(|tag| !source.contains(&format!("__tsMV{tag}")))
        .unwrap_or_default()
}

/// The placeholder spellings to try, in order.
const CANDIDATES: usize = 3;

fn placeholder(metavar: &Metavar, candidate: usize, tag: &str) -> String {
    let m = if metavar.multi { "M" } else { "" };
    let ident = format!("__tsMV{tag}{m}_{}__", metavar.name);
    match candidate {
        0 => ident,
        1 => format!("\"{ident}\""),
        _ => "1".to_owned(),
    }
}

/// The source with each hole replaced by its chosen placeholder.
struct Built {
    text: String,
    /// The length of `text` without any suffix [`attempt`] put after it.
    pattern_len: usize,
    /// Each hole's placeholder bytes in `text`.
    holes: Vec<Range<usize>>,
    /// Ranges of `text` and where they came from in the source:
    /// `(text range, source start, copied)`. A copied range maps byte for
    /// byte; any other maps to its source start.
    map: Vec<(Range<usize>, usize, bool)>,
    /// The source's length, where anything past the end of `text` maps.
    source_len: usize,
}

impl Built {
    /// Where a byte of `text` came from in the source.
    fn source_offset(&self, offset: usize) -> usize {
        for (range, source, copied) in &self.map {
            if range.contains(&offset) {
                return if *copied {
                    source + (offset - range.start)
                } else {
                    *source
                };
            }
        }
        // Past the end (a token missing at the end): the end of the source.
        self.source_len
    }
}

fn build(source: &str, scan: &Scan, choice: &[usize], tag: &str) -> Built {
    enum Cut<'a> {
        Hole(usize, &'a Hole),
        Dollar(usize),
    }
    let mut cuts: Vec<(usize, Cut<'_>)> = scan
        .holes
        .iter()
        .enumerate()
        .map(|(i, hole)| (hole.source.start, Cut::Hole(i, hole)))
        .chain(scan.dollars.iter().map(|&at| (at, Cut::Dollar(at))))
        .collect();
    cuts.sort_by_key(|(at, _)| *at);
    let mut text = String::with_capacity(source.len() + 16 * scan.holes.len());
    let mut holes = vec![0..0; scan.holes.len()];
    let mut map = Vec::new();
    let mut copied_to = 0;
    let copy = |text: &mut String, map: &mut Vec<_>, from: usize, to: usize| {
        if to > from {
            map.push((text.len()..text.len() + (to - from), from, true));
            text.push_str(&source[from..to]);
        }
    };
    for (at, cut) in cuts {
        copy(&mut text, &mut map, copied_to, at);
        match cut {
            Cut::Hole(i, hole) => {
                let spelled = placeholder(&scan.metavars[hole.metavar], choice[i], tag);
                let range = text.len()..text.len() + spelled.len();
                map.push((range.clone(), hole.source.start, false));
                text.push_str(&spelled);
                holes[i] = range;
                copied_to = hole.source.end;
            }
            Cut::Dollar(at) => {
                map.push((text.len()..text.len() + 1, at, false));
                text.push('$');
                copied_to = at + 2;
            }
        }
    }
    copy(&mut text, &mut map, copied_to, source.len());
    Built {
        pattern_len: text.len(),
        text,
        holes,
        map,
        source_len: source.len(),
    }
}

/// The node that is exactly `range`: the smallest node spanning it, if it
/// starts and ends there. For a string placeholder that is the string, not
/// its content.
fn node_at<'t>(root: Node<'t>, range: &Range<usize>) -> Option<Node<'t>> {
    let node = root.descendant_for_byte_range(range.start, range.end)?;
    (node.byte_range() == *range).then_some(node)
}

/// The bytes of the first ERROR or MISSING node, in tree order.
fn first_error(root: Node<'_>) -> Option<Range<usize>> {
    if !root.has_error() {
        return None;
    }
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            return Some(node.byte_range());
        }
        // Only go down into a subtree that holds the error.
        if node.has_error() && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                // `has_error` said there was one; a tree that disagrees is
                // blamed at its start.
                return Some(root.start_byte()..root.start_byte());
            }
        }
    }
}

/// Which hole to try the next spelling of: the first one that isn't a node
/// of its own, else the one nearest the first error. `None` once that hole
/// has no spelling left.
fn blame(
    holes: &[Range<usize>],
    clean: &[bool],
    error: Option<&Range<usize>>,
    choice: &[usize],
) -> Option<usize> {
    let left = |i: &usize| choice[*i] + 1 < CANDIDATES;
    if let Some(i) = (0..holes.len()).find(|&i| !clean[i]) {
        return left(&i).then_some(i);
    }
    let error = error?;
    let distance = |hole: &Range<usize>| {
        if hole.end < error.start {
            error.start - hole.end
        } else {
            hole.start.saturating_sub(error.end)
        }
    };
    (0..holes.len())
        .filter(left)
        .min_by_key(|&i| distance(&holes[i]))
}

/// Copies the accepted tree out, from its pattern root down.
fn finish(
    language: &'static Language,
    source: &str,
    scan: &Scan,
    built: &Built,
    tree: &Tree,
) -> Pattern {
    let wildcards: HashMap<usize, Wildcard> = built
        .holes
        .iter()
        .zip(&scan.holes)
        .filter_map(|(range, hole)| {
            let node = node_at(tree.root_node(), range)?;
            let metavar = &scan.metavars[hole.metavar];
            let wildcard = Wildcard {
                name: metavar.name.clone(),
                multi: metavar.multi,
                anonymous: metavar.anonymous,
            };
            Some((node.id(), wildcard))
        })
        .collect();
    let root = pattern_root(tree.root_node(), &built.text[..built.pattern_len]);
    Pattern {
        language,
        source: source.into(),
        root: copy(
            root,
            None,
            &built.text,
            built.pattern_len,
            &wildcards,
            &mut Names::default(),
        ),
        metavars: scan.metavars.clone(),
        constraints: Constraints::default(),
    }
}

/// From the tree root down, through each node with exactly one named child
/// that still covers all of the pattern's non-whitespace text.
fn pattern_root<'t>(root: Node<'t>, text: &str) -> Node<'t> {
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    let covers = |node: &Node<'_>| node.start_byte() <= start && node.end_byte() >= end;
    let mut node = root;
    loop {
        if node.named_child_count() != 1 {
            return node;
        }
        match node.named_child(0) {
            Some(child) if covers(&child) => node = child,
            _ => return node,
        }
    }
}

/// Kind and field names, one copy of each per pattern.
#[derive(Default)]
struct Names(HashMap<Box<str>, Arc<str>>);

impl Names {
    fn get(&mut self, name: &str) -> Arc<str> {
        if let Some(name) = self.0.get(name) {
            return name.clone();
        }
        let shared: Arc<str> = name.into();
        self.0.insert(name.into(), shared.clone());
        shared
    }
}

/// Copies `node` out of the parse of `text`, of which the pattern is
/// `..end`.
fn copy(
    node: Node<'_>,
    field: Option<Arc<str>>,
    text: &str,
    end: usize,
    wildcards: &HashMap<usize, Wildcard>,
    names: &mut Names,
) -> PatternNode {
    let mut out = PatternNode {
        kind_id: node.kind_id(),
        kind: names.get(node.kind()),
        named: node.is_named(),
        field,
        extra: node.is_extra(),
        open_end: false,
        text: None,
        children: Vec::new(),
        wildcard: None,
    };
    if let Some(wildcard) = wildcards.get(&node.id()) {
        out.wildcard = Some(wildcard.clone());
        return out;
    }
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.start_byte() >= end && child.end_byte() > child.start_byte() {
                // Only the suffix the parse needed, not the pattern's.
                out.open_end = true;
            } else {
                let field = cursor.field_name().map(|field| names.get(field));
                out.children
                    .push(copy(child, field, text, end, wildcards, names));
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    } else {
        out.text = Some(text[node.byte_range()].into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_finds_each_kind_of_metavariable_and_leaves_lone_dollars() {
        let scan = scan("f($A, $$$REST, $_, $$$, $$, $a, $Abc, $, $_0, $_X, $_X)").unwrap();
        let names: Vec<_> = scan
            .metavars
            .iter()
            .map(|m| (m.name.as_str(), m.multi, m.anonymous))
            .collect();
        assert_eq!(
            names,
            [
                ("A", false, false),
                ("REST", true, false),
                ("_any0", false, true),
                ("_any1", true, true),
                ("_any2", false, true),
                ("_any3", false, true),
                ("_any4", false, true),
            ]
        );
        assert_eq!(scan.holes.len(), 7);
        assert_eq!(scan.dollars, [24]);
    }

    #[test]
    fn mixing_single_and_multi_under_one_name_is_an_error() {
        let error = scan("f($A, $$$A)").unwrap_err();
        assert_eq!(error.kind, PatternErrorKind::MixedMetavar);
        assert_eq!(error.offset, 6);
    }

    #[test]
    fn built_text_maps_back_to_the_source() {
        let source = "a $$ $B c";
        let scan = scan(source).unwrap();
        let built = build(source, &scan, &[0], "");
        assert_eq!(built.text, "a $ __tsMV_B__ c");
        assert_eq!(built.source_offset(0), 0);
        assert_eq!(built.source_offset(2), 2);
        assert_eq!(built.source_offset(3), 4);
        assert_eq!(built.source_offset(6), 5);
        assert_eq!(built.source_offset(15), 8);
    }

    #[test]
    fn placeholder_spellings_are_searched_past_the_guided_walk() {
        // T708 F1: JavaScript's `import $A from $B` needs an identifier, then
        // a string. Error recovery blames `$A` while `$B` is an identifier,
        // so the guided walk runs `$A` out of spellings first.
        let js = fe_syntax::language_by_name("javascript").unwrap();
        let pattern = Pattern::compile(js, "import $A from $B").unwrap();
        let found = pattern.find_in_text("import x from \"y\"\n").unwrap();
        assert_eq!(found.len(), 1);
        // JSON keys are strings, and so is a value spelled as an identifier.
        let json = fe_syntax::language_by_name("json").unwrap();
        let pattern = Pattern::compile(json, "{$K: $V}").unwrap();
        let text = "{\"a\": \"b\"}";
        let found = pattern.find_in_text(text).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(&text[found[0].captures.single("K").unwrap()], "\"a\"");
    }

    #[test]
    fn pattern_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<Pattern>();
    }
}
