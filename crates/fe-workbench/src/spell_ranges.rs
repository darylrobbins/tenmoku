//! Which of a buffer's text is spell checked: [`spellable_ranges`] finds the
//! comments and strings of code, and the prose of plain text and Markdown. A
//! code identifier must never be checked, so whatever isn't known to be words
//! is left out.
//!
//! All of it comes from the parse [`Syntax`] already has, for Markdown too: its
//! tree says which rows are prose, and its highlights which parts of those
//! aren't. Nothing here parses, so it's cheap enough to ask for what's visible.

use std::ops::Range;

use fe_config::settings::SpellScope;
use fe_syntax::{Highlight, HighlightSpan, InjectedNode, Syntax};
use fe_text::Text;

/// The largest buffer that gets spell checked.
pub const MAX_SPELLABLE_BYTES: usize = 2 * 1024 * 1024;

/// What [`checked_spellable_ranges`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spellable {
    pub ranges: Vec<Range<usize>>,
    /// Whether the highlights asked for all finished, however far past the rows
    /// that went for a whole token. If not, there are no `ranges`, and asking
    /// again may find some.
    pub complete: bool,
}

impl Spellable {
    fn found(ranges: Vec<Range<usize>>) -> Self {
        Self {
            ranges,
            complete: true,
        }
    }

    fn unfinished() -> Self {
        Self::default()
    }
}

/// The bytes of `text` to spell check among `rows`, sorted and not overlapping.
/// `syntax` must have been told of every edit to `text`; it's `None` for a
/// buffer with no grammar, which is plain text.
///
/// A comment, a string or a Markdown paragraph that `rows` only partly covers
/// is found whole, though it can take several ranges: one for each line of a
/// comment, and one for each stretch of a string or paragraph between what
/// isn't words. Plain text has nothing to go whole, so it's just `rows`, and
/// all of it is one range.
///
/// There are none for a buffer over [`MAX_SPELLABLE_BYTES`] or without a full
/// tree that's current: one too big for it, one not parsed since its last edit,
/// or one whose highlighting failed.
///
/// Nor are there any from highlighting that didn't finish, since what isn't
/// painted can't be told from what wasn't reached: a query cut short or a
/// language not compiled yet. [`Syntax::take_missing_languages`] says when to
/// ask again.
pub fn spellable_ranges(
    syntax: Option<&Syntax>,
    text: &Text,
    scopes: &[SpellScope],
    rows: Range<u32>,
) -> Vec<Range<usize>> {
    checked_spellable_ranges(syntax, text, scopes, rows).ranges
}

/// [`spellable_ranges`], saying whether its having none is because highlighting
/// didn't finish: the highlights of the queries it made, which can reach past
/// `rows`. Those it doesn't ask for at all, for a buffer too big or with no tree
/// that's current, count as finished.
pub fn checked_spellable_ranges(
    syntax: Option<&Syntax>,
    text: &Text,
    scopes: &[SpellScope],
    rows: Range<u32>,
) -> Spellable {
    let window = row_bytes(text, rows);
    if window.is_empty() || text.len() > MAX_SPELLABLE_BYTES {
        return Spellable::found(Vec::new());
    }
    let Some(syntax) = syntax else {
        return Spellable::found(match scopes.contains(&SpellScope::Text) {
            true => vec![window],
            false => Vec::new(),
        });
    };
    // An edited tree's captures stretch over what was typed, which can be code.
    if syntax.failed() || syntax.tree().is_none() || syntax.needs_parse() {
        return Spellable::found(Vec::new());
    }
    if syntax.language().name() == "markdown" {
        return match scopes.contains(&SpellScope::Text) {
            true => markdown_prose(syntax, text, window),
            false => Spellable::found(Vec::new()),
        };
    }
    code_tokens(syntax, text, scopes, window)
}

/// The bytes of `rows`, through the line break of the last.
fn row_bytes(text: &Text, rows: Range<u32>) -> Range<usize> {
    if rows.is_empty() || rows.start >= text.row_count() {
        return 0..0;
    }
    let end = match rows.end < text.row_count() {
        true => text.row_start(rows.end),
        false => text.len(),
    };
    text.row_start(rows.start)..end
}

/// What a highlight in code is, to spell check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Comment,
    String,
}

/// What `highlight` is in `scopes`, if it's words. A code span in a doc comment
/// isn't, nor is the part of a string that's a placeholder, a regular
/// expression, a symbol or an object's key.
fn token_of(highlight: Highlight, scopes: &[SpellScope]) -> Option<Token> {
    match highlight.name() {
        "comment" | "comment.documentation" if scopes.contains(&SpellScope::Comments) => {
            Some(Token::Comment)
        }
        "string" if scopes.contains(&SpellScope::Strings) => Some(Token::String),
        _ => None,
    }
}

/// The text of the comments and strings touching `window`.
fn code_tokens(
    syntax: &Syntax,
    text: &Text,
    scopes: &[SpellScope],
    window: Range<usize>,
) -> Spellable {
    // Highlighting the window parses the layers of injected languages, which
    // are where their tokens' boundaries are.
    if !syntax.checked_highlights(window.clone()).complete {
        return Spellable::unfinished();
    }
    let crossing = tokens_crossing(syntax, scopes, window.clone());
    let reach = (crossing.iter().map(|(_, token)| token)).fold(window.clone(), |reach, token| {
        reach.start.min(token.start)..reach.end.max(token.end)
    });
    match whole_token_spans(syntax, text, scopes, reach) {
        Some(spans) => Spellable::found(words_in_spans(
            syntax, text, scopes, &window, &crossing, &spans,
        )),
        None => Spellable::unfinished(),
    }
}

/// The words of `spans`, sorted, for those in a scope that touch `window` or
/// are part of a token of their own kind in `crossing`. What's in a token of
/// another kind isn't part of it: a comment in a string's interpolation on
/// another row is another token, one that doesn't touch `window`. This reads
/// the tree once and each span once, so it takes time linear in what it's given.
pub(crate) fn words_in_spans(
    syntax: &Syntax,
    text: &Text,
    scopes: &[SpellScope],
    window: &Range<usize>,
    crossing: &[(Token, Range<usize>)],
    spans: &[HighlightSpan],
) -> Vec<Range<usize>> {
    let touches = |a: &Range<usize>, b: &Range<usize>| a.start < b.end && b.start < a.end;
    let kept: Vec<_> = spans
        .iter()
        .filter_map(|span| Some((span, token_of(span.highlight, scopes)?)))
        .filter(|(span, token)| {
            touches(&span.range, window)
                || (crossing.iter())
                    .any(|(kind, range)| kind == token && touches(&span.range, range))
        })
        .collect();
    let (Some((first, _)), Some((last, _))) = (kept.first(), kept.last()) else {
        return Vec::new();
    };
    let literals = literals(syntax, first.range.start..last.range.end);
    let host = syntax.language().name();
    // The literals holding the current span, the innermost last.
    let mut open: Vec<&Literal> = Vec::new();
    let mut next = 0;
    let mut ranges = Vec::new();
    for (span, token) in kept {
        // A string's highlights can run on from one literal into another, such
        // as a layer's string painted over the end of its host's, so each part
        // that a literal holds is read on its own.
        let mut from = span.range.start;
        while from < span.range.end {
            while literals.get(next).is_some_and(|it| it.range.start <= from) {
                open.push(&literals[next]);
                next += 1;
            }
            while open.last().is_some_and(|it| it.range.end <= from) {
                open.pop();
            }
            let upto = match token {
                Token::Comment => span.range.end,
                Token::String => (open.last().map(|it| it.range.end))
                    .into_iter()
                    .chain(literals.get(next).map(|it| it.range.start))
                    .fold(span.range.end, usize::min),
            };
            let part = from..upto.max(from + 1);
            let source = text.slice_to_string(part.clone());
            let pieces = match token {
                Token::Comment => comment_text(&source),
                Token::String => string_text(&source, part.clone(), open.last().copied(), host),
            };
            let words = pieces
                .into_iter()
                .filter(|piece| has_word(&source[piece.clone()]));
            ranges.extend(words.map(|piece| part.start + piece.start..part.start + piece.end));
            from = part.end;
        }
    }
    ranges
}

/// The comments and strings in `scopes` that cross either edge of `window`, as
/// the trees have them, the outermost of each kind, since an escape or a hole
/// splits their highlights. That's the host language's tree and those of the
/// injected layers highlighting has parsed.
fn tokens_crossing(
    syntax: &Syntax,
    scopes: &[SpellScope],
    window: Range<usize>,
) -> Vec<(Token, Range<usize>)> {
    let mut tokens = Vec::new();
    let Some(tree) = syntax.tree() else {
        return tokens;
    };
    for at in [window.start, window.end - 1] {
        let mut string: Option<Range<usize>> = None;
        let mut comment: Option<Range<usize>> = None;
        let mut widest = |kind: &str, range: Range<usize>| {
            let slot = match (is_string_kind(kind), is_comment_kind(kind)) {
                (true, _) => &mut string,
                (_, true) => &mut comment,
                _ => return,
            };
            if slot.as_ref().is_none_or(|it| range.len() > it.len()) {
                *slot = Some(range);
            }
        };
        let mut found = tree.root_node().descendant_for_byte_range(at, at + 1);
        while let Some(node) = found {
            widest(node.kind(), node.byte_range());
            found = node.parent();
        }
        for node in syntax.injected_nodes(at..at + 1) {
            widest(&node.kind, node.range);
        }
        if scopes.contains(&SpellScope::Strings) {
            tokens.extend(string.map(|range| (Token::String, range)));
        }
        if scopes.contains(&SpellScope::Comments) {
            tokens.extend(comment.map(|range| (Token::Comment, range)));
        }
    }
    tokens
}

/// Whether a node of this `kind`, in any grammar, is a string: the whole
/// of one, or the text of one that has no node for the whole, as in PHP's
/// heredocs. What holds strings together, like C's `concatenated_string`, or
/// has one inside, like Ruby's `string_array`, is not.
fn is_string_kind(kind: &str) -> bool {
    matches!(
        kind,
        "string"
            | "string_literal"
            | "raw_string_literal"
            | "raw_string"
            | "verbatim_string_literal"
            | "interpolated_string_expression"
            | "interpreted_string_literal"
            | "line_string_literal"
            | "multi_line_string_literal"
            | "template_string"
            | "ansi_c_string"
            | "translated_string"
            | "bare_string"
            | "encapsed_string"
            | "heredoc"
            | "nowdoc"
    ) || is_string_text_kind(kind)
}

/// Whether a node of this `kind` is the name that opens or closes a heredoc,
/// which highlights as a string but is a delimiter, whichever of the two ends of
/// the heredoc it's at.
fn is_heredoc_marker_kind(kind: &str) -> bool {
    matches!(kind, "heredoc_start" | "heredoc_end" | "heredoc_beginning")
}

/// Whether a node of this `kind` is the words of a string, rather than a
/// delimiter, an escape or an interpolation.
fn is_string_text_kind(kind: &str) -> bool {
    matches!(
        kind,
        "string_content"
            | "string_fragment"
            | "string_value"
            | "multiline_string_fragment"
            | "multiline_string_content"
            | "raw_string_content"
            | "string_literal_content"
            | "verbatim_string_content"
            | "interpreted_string_literal_content"
            | "raw_string_literal_content"
            | "heredoc_content"
            | "heredoc_body"
            | "nowdoc_body"
            | "nowdoc_string"
            | "line_str_text"
            | "multi_line_str_text"
            | "raw_str_part"
    )
}

/// Whether a node of this `kind` is a comment: `comment`, `line_comment`,
/// `block_comment`, `multiline_comment`. Its parts, such as a doc comment's
/// marker or text, aren't named so.
fn is_comment_kind(kind: &str) -> bool {
    kind == "comment" || kind.ends_with("_comment")
}

/// The highlights around `window`, wide enough that a comment or string
/// touching it isn't cut off. Highlights end at the range they're asked for, so
/// this looks a row past `window` each way, and goes on, further each time, for
/// as long as a token that's in `window` reaches that range's edge. `None` if
/// highlighting didn't finish.
fn whole_token_spans(
    syntax: &Syntax,
    text: &Text,
    scopes: &[SpellScope],
    window: Range<usize>,
) -> Option<Vec<HighlightSpan>> {
    let row_above = |offset: usize, step: usize| {
        text.row_start(text.offset_to_point(offset.saturating_sub(step)).row)
    };
    let row_below = |offset: usize, step: usize| {
        let row = text.offset_to_point((offset + step).min(text.len())).row;
        row_bytes(text, row..row + 1).end
    };
    let mut range = row_above(window.start, 1)..row_below(window.end, 1);
    let mut step = window.len();
    loop {
        let highlights = syntax.checked_highlights(range.clone());
        if !highlights.complete {
            return None;
        }
        let spans = highlights.spans;
        let is_token = |span: &&HighlightSpan| token_of(span.highlight, scopes).is_some();
        let cut_above = range.start > 0
            && (spans.iter().find(is_token)).is_some_and(|span| {
                span.range.start <= range.start && span.range.end > window.start
            });
        let cut_below = range.end < text.len()
            && (spans.iter().rfind(is_token))
                .is_some_and(|span| span.range.end >= range.end && span.range.start < window.end);
        if !cut_above && !cut_below {
            return Some(spans);
        }
        if cut_above {
            range.start = row_above(range.start, step);
        }
        if cut_below {
            range.end = row_below(range.end, step);
        }
        step = step.saturating_mul(2);
    }
}

/// Whether `c` can be part of what marks a comment rather than being its text.
/// Those that say something about a word beside them aren't among them: `.`, `$`,
/// `@`, `_`, `\`, `'`, `:` and `` ` ``.
fn is_comment_mark(c: char) -> bool {
    "/*#!-;%<>=|~+()[]{}\"".contains(c)
}

/// The text of each line of a comment's `source`: without the marks at either
/// end, which on a line of a block comment or a run of doc comments may be
/// `/*`, `*`, `*/`, `///` or `//!`. Only the marks up against the end count, so
/// a `/tmp` first in a `// /tmp` comment stays a path.
fn comment_text(source: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut at = 0;
    for line in source.split_inclusive('\n') {
        let rest = line
            .trim_start()
            .trim_start_matches(is_comment_mark)
            .trim_start();
        let start = line.len() - rest.len();
        let kept = rest.trim_end().trim_end_matches(is_comment_mark).trim_end();
        lines.push(at + start..at + start + kept.len());
        at += line.len();
    }
    lines
}

/// A string as a tree has it, the host's or an injected layer's.
pub(crate) struct Literal {
    range: Range<usize>,
    /// The language of the tree it's in.
    language: &'static str,
    /// The parts of `range` that aren't its text, sorted: its delimiters,
    /// escapes and interpolations, and none if it's a single token to the tree.
    not_text: Vec<Range<usize>>,
}

/// The strings touching `range` in the host language's tree and in the trees of
/// injected layers, sorted by where they start, an outer one before what it
/// holds. In each tree they're the outermost, so what's in an interpolation is
/// part of that and not its text. What a string's children are varies by
/// language, but those that are its text say so, by their kind: `string_content`,
/// `string_fragment`, `line_str_text`. Any other is taken for not being words,
/// whether it's C++'s `R"tag(` or C's `\x41`.
fn literals(syntax: &Syntax, range: Range<usize>) -> Vec<Literal> {
    let host = host_string_nodes(syntax, range.clone());
    let mut literals = literals_in(&host);
    literals.extend(literals_in(&syntax.injected_nodes(range)));
    literals.sort_by(|a, b| {
        (a.range.start, std::cmp::Reverse(a.range.end))
            .cmp(&(b.range.start, std::cmp::Reverse(b.range.end)))
    });
    literals
}

/// The strings among `nodes`, one or more trees' nodes, parents before children.
pub(crate) fn literals_in(nodes: &[InjectedNode]) -> Vec<Literal> {
    let mut literals: Vec<Literal> = Vec::new();
    // For each node: `None` outside any string, `Some(None)` in one but not
    // its text, such as an interpolation, and the literal it's the text of or
    // the root of otherwise.
    let mut holder: Vec<Option<Option<usize>>> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let parent = node.parent.and_then(|parent| holder[parent]);
        let own = match parent {
            Some(Some(literal)) if node.named && is_string_text_kind(&node.kind) => {
                Some(Some(literal))
            }
            Some(Some(literal)) => {
                literals[literal].not_text.push(node.range.clone());
                Some(None)
            }
            Some(None) => Some(None),
            // A heredoc's marker is painted as a string, and is no words.
            None if is_heredoc_marker_kind(&node.kind) => {
                literals.push(Literal {
                    range: node.range.clone(),
                    language: node.language,
                    not_text: vec![node.range.clone()],
                });
                Some(None)
            }
            None if is_string_kind(&node.kind) => {
                literals.push(Literal {
                    range: node.range.clone(),
                    language: node.language,
                    not_text: Vec::new(),
                });
                Some(Some(literals.len() - 1))
            }
            None => None,
        };
        holder.push(own);
    }
    for literal in &mut literals {
        literal.not_text.sort_by_key(|range| range.start);
    }
    literals
}

/// The strings of the host language's tree touching `range`, each with its
/// descendants, as [`InjectedNode`]s to read like a layer's.
fn host_string_nodes(syntax: &Syntax, range: Range<usize>) -> Vec<InjectedNode> {
    let mut nodes = Vec::new();
    let Some(tree) = syntax.tree() else {
        return nodes;
    };
    let language = syntax.language().name();
    let mut cursor = tree.walk();
    // The index in `nodes` of each node above the cursor, within a string.
    let mut parents: Vec<usize> = Vec::new();
    loop {
        let node = cursor.node();
        // Whatever follows in the tree starts later still.
        if node.start_byte() >= range.end {
            return nodes;
        }
        let touches = node.end_byte() > range.start;
        if touches
            && (!parents.is_empty()
                || is_string_kind(node.kind())
                || is_heredoc_marker_kind(node.kind()))
        {
            nodes.push(InjectedNode {
                language,
                kind: node.kind().to_string(),
                named: node.is_named(),
                range: node.byte_range(),
                parent: parents.last().copied(),
            });
            if cursor.goto_first_child() {
                parents.push(nodes.len() - 1);
                continue;
            }
        } else if touches && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return nodes;
            }
            parents.pop();
        }
    }
}

/// The words of `source`, the bytes `span` of a string, without its delimiters,
/// escapes or `{}` holes. The tree says what those are in a string of the host
/// language that it has the parts of, `literal`, host or injected. Otherwise, in
/// a string it has as one token, they're told from the text, by the rules of the
/// literal's language, or of `host`, the buffer's, with no literal.
pub(crate) fn string_text(
    source: &str,
    span: Range<usize>,
    literal: Option<&Literal>,
    host: &str,
) -> Vec<Range<usize>> {
    let language = literal.map_or(host, |it| it.language);
    let c_like = matches!(language, "c" | "cpp");
    let not_text = literal.map_or(&[][..], |it| &it.not_text);
    let first = not_text.partition_point(|cut| cut.end <= span.start);
    let mut pieces = Vec::new();
    let mut from = span.start;
    for cut in not_text[first..]
        .iter()
        .take_while(|cut| cut.start < span.end)
    {
        // A string in an interpolation is inside its cut, and has no text.
        let start = cut.start.max(span.start);
        if from < start {
            pieces.push(from..start);
        }
        from = from.max(cut.end.min(span.end));
        // In C, a `\x` takes as many digits as follow, past where its grammar
        // stops. Elsewhere it has a length, as in Rust's `\x41bad`.
        if c_like && source[start - span.start..].starts_with("\\x") {
            let after = source[from - span.start..].chars();
            from += after.take_while(char::is_ascii_hexdigit).count();
        }
    }
    if from < span.end {
        pieces.push(from..span.end);
    }
    // A delimiter the tree doesn't have as a part of its own is at an end.
    let starts = literal.is_none_or(|it| it.range.start == span.start);
    let ends = literal.is_none_or(|it| it.range.end == span.end);
    pieces
        .into_iter()
        .flat_map(|piece| {
            let piece = piece.start - span.start..piece.end - span.start;
            let opens = starts && piece.start == 0;
            let closes = ends && piece.end == source.len();
            text_of_piece(
                source,
                piece,
                opens,
                closes,
                not_text.is_empty().then_some(language),
            )
        })
        .collect()
}

/// `piece` of a string's `source` in the pieces it has for words: less its
/// delimiters if it `opens` or `closes` the string, its `{}` holes, and with
/// `escapes`, the language whose escapes they are, its escapes, unless it opens
/// as a raw string.
fn text_of_piece(
    source: &str,
    piece: Range<usize>,
    opens: bool,
    closes: bool,
    escapes: Option<&str>,
) -> Vec<Range<usize>> {
    let start = match opens {
        true => piece.start + string_open(&source[piece.clone()]),
        false => piece.start,
    };
    let end = match closes {
        true => {
            let rest = source[start..piece.end].trim_end_matches('#');
            start + rest.trim_end_matches(['"', '\'', '`']).len()
        }
        false => piece.end,
    };
    let escapes = escapes.filter(|_| !is_raw_string(&source[piece.start..start]));
    let mut pieces = Vec::new();
    let mut from = start;
    let mut search = start;
    // The end of the line last looked at, and up to where it has no `}` left,
    // so that each line is read once however many braces it has.
    let mut line_end = start;
    let mut no_close_until = start;
    while let Some(at) = source[search..end].find(['{', '\\']) {
        let at = search + at;
        let cut = match source[at..].starts_with('{') {
            true => {
                if at >= line_end {
                    line_end = source[at..end].find('\n').map_or(end, |to| at + to);
                }
                let close = match at < no_close_until {
                    true => None,
                    false => source[at..line_end].find('}').map(|close| close + 1),
                };
                if close.is_none() {
                    no_close_until = line_end;
                }
                close
            }
            false => escapes.map(|language| escape_len(language, &source[at..end])),
        };
        // A brace with no `}` on its line is just a brace.
        let Some(cut) = cut else {
            search = at + 1;
            continue;
        };
        pieces.push(from..at);
        from = at + cut;
        search = from;
    }
    pieces.push(from..end);
    pieces
        .into_iter()
        .map(|piece| trimmed(source, piece))
        .collect()
}

/// How long the escape `text` of a string in `language` starts with is, its
/// backslash included: with the digits of `\x41`, `\u00e9`, `\U000000e9` or
/// `\101`, and otherwise just the character after the backslash. A `\x` in C
/// and C++ takes as many digits as follow, and elsewhere two; a `\u` takes four
/// and a `\U` eight, where the language has them. A `\u{e9}`'s braces are a
/// hole's.
fn escape_len(language: &str, text: &str) -> usize {
    let Some(kind) = text[1..].chars().next() else {
        return 1;
    };
    let after = text[1 + kind.len_utf8()..].chars();
    let more = match kind {
        'x' if matches!(language, "c" | "cpp") => after.take_while(char::is_ascii_hexdigit).count(),
        'x' => after.take(2).take_while(char::is_ascii_hexdigit).count(),
        'u' => after.take(4).take_while(char::is_ascii_hexdigit).count(),
        'U' => after.take(8).take_while(char::is_ascii_hexdigit).count(),
        '0'..='7' => after.take(2).take_while(|c| matches!(c, '0'..='7')).count(),
        _ => 0,
    };
    1 + kind.len_utf8() + more
}

/// Whether `open`, as [`string_open`] found it, starts a string with no escapes.
fn is_raw_string(open: &str) -> bool {
    open.trim_end_matches(['"', '\'', '`'])
        .contains(['r', 'R', '@'])
}

/// How much of a string's `source` opens it: its quotes, with a prefix such as
/// `r#`, `b`, `f` or `u8` before them. The part of a string after an escape can
/// end in its closing quote, so only prefixes that strings have count: `teh"` is
/// a word and a quote, and one that reads as a prefix is too short to check. An
/// apostrophe rarely opens a string, so it needs a prefix that's Python's: `I'm`
/// has no quote in it.
fn string_open(source: &str) -> usize {
    let Some(quote_at) = source.find(['"', '\'', '`']) else {
        return 0;
    };
    let (prefix, rest) = source.split_at(quote_at);
    let name = prefix.trim_matches('#');
    let opens = match rest.starts_with('\'') {
        true if prefix.is_empty() => true,
        true => is_one_of(prefix, &["r", "b", "u", "f", "rb", "br", "rf", "fr"]),
        false if name.is_empty() => true,
        false => is_one_of(
            name,
            &[
                "r", "b", "f", "u", "l", "c", "s", "e", "x", "rb", "br", "rf", "fr", "u8", "cr",
                "lr", "ur", "u8r", "$", "@", "$@", "@$",
            ],
        ),
    };
    if !opens {
        return 0;
    }
    let quote = rest.chars().next().expect("found a quote");
    source.len() - rest.trim_start_matches(quote).len()
}

/// Whether `prefix` is one of `known`, in any case.
fn is_one_of(prefix: &str, known: &[&str]) -> bool {
    known.iter().any(|known| prefix.eq_ignore_ascii_case(known))
}

/// `piece` of `source` without whitespace at either end.
fn trimmed(source: &str, piece: Range<usize>) -> Range<usize> {
    let text = &source[piece.clone()];
    let start = piece.start + (text.len() - text.trim_start().len());
    start..start + text.trim().len()
}

/// Whether `text` has anything to spell check, not only punctuation.
fn has_word(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

/// The prose of the Markdown paragraphs, headings and list items touching
/// `window`. Those are the block grammar's `inline` nodes, which is why code
/// blocks, HTML blocks, reference definitions, front matter and tables aren't
/// looked at; a table's cells have no inline highlights to tell code from words.
///
/// The inline grammar is an injected language with no tree to hand, but its
/// highlights are there: what they paint is markup, but for heading and link
/// text.
fn markdown_prose(syntax: &Syntax, text: &Text, window: Range<usize>) -> Spellable {
    let inlines = inline_nodes(syntax, window);
    let (Some(first), Some(last)) = (inlines.first(), inlines.last()) else {
        return Spellable::found(Vec::new());
    };
    let highlights = syntax.checked_highlights(first.start..last.end);
    if !highlights.complete {
        return Spellable::unfinished();
    }
    let markup: Vec<_> = (highlights.spans.iter())
        .filter(|span| !is_prose(span.highlight, text.char_at(span.range.start)))
        .collect();
    let mut ranges = Vec::new();
    for inline in inlines {
        let source = text.slice_to_string(inline.clone());
        let first = markup.partition_point(|span| span.range.end <= inline.start);
        let painted: Vec<_> = markup[first..]
            .iter()
            .take_while(|span| span.range.start < inline.end)
            .collect();
        // Within code or a link's destination, a `<` isn't a tag's.
        let not_html: Vec<_> = (painted.iter())
            .filter(|span| matches!(span.highlight.name(), "markup.raw" | "markup.link.url"))
            .map(|span| {
                span.range.start.saturating_sub(inline.start)..span.range.end - inline.start
            })
            .collect();
        let html = raw_html(&source, &not_html)
            .into_iter()
            .map(|cut| inline.start + cut.start..inline.start + cut.end);
        let mut cuts: Vec<_> = (painted.iter().map(|span| span.range.clone()))
            .chain(html)
            .collect();
        cuts.sort_by_key(|cut| cut.start);
        let mut from = inline.start;
        for cut in cuts {
            let up_to = cut.start.clamp(from, inline.end);
            push_prose(&source, inline.start, from..up_to, &mut ranges);
            from = from.max(cut.end);
        }
        let end = inline.end.max(from);
        push_prose(&source, inline.start, from..end, &mut ranges);
    }
    Spellable::found(ranges)
}

/// The raw HTML in an inline node's `source`, tags and all that's inside
/// comments, processing instructions and CDATA, as HTML's highlights leave
/// bits of them out. What looks like a tag counts, whether or not it's quite
/// one to the grammar, such as `<span hidden>`: `<` then a letter, `/`, `!` or
/// `?`. So `<b and c>` is lost, but `a < b` isn't. Not one after an odd number of
/// backslashes, nor one in `not_html`, sorted ranges of `source`. It takes
/// linear time: each kind's close is looked for until there's none left, and
/// [`tag_len`] reads no further than what it cuts, or than a `<` that follows.
pub(crate) fn raw_html(source: &str, not_html: &[Range<usize>]) -> Vec<Range<usize>> {
    const FORMS: [(&str, &str); 4] = [
        ("<!--", "-->"),
        ("<?", "?>"),
        ("<![CDATA[", "]]>"),
        ("<", ">"),
    ];
    let mut no_close = [false; FORMS.len()];
    let mut not_html = not_html.iter().peekable();
    let mut cuts = Vec::new();
    let mut search = 0;
    while let Some(at) = source[search..].find('<') {
        let at = search + at;
        let rest = &source[at..];
        search = at + 1;
        while not_html.peek().is_some_and(|range| range.end <= at) {
            not_html.next();
        }
        if not_html.peek().is_some_and(|range| range.start <= at) {
            continue;
        }
        let starts_tag = (rest[1..].chars().next())
            .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'));
        let backslashes = source[..at].len() - source[..at].trim_end_matches('\\').len();
        if !starts_tag || backslashes % 2 == 1 {
            continue;
        }
        let Some(form) = FORMS.iter().position(|(open, _)| rest.starts_with(open)) else {
            continue;
        };
        if no_close[form] {
            continue;
        }
        let (open, close) = FORMS[form];
        let body = &rest[open.len()..];
        let end = match close {
            ">" => match tag_len(body) {
                Some(len) => at + open.len() + len,
                None => continue,
            },
            _ => match body.find(close) {
                Some(to) => at + open.len() + to + close.len(),
                None => {
                    no_close[form] = true;
                    continue;
                }
            },
        };
        cuts.push(at..end);
        search = end;
    }
    cuts
}

/// How much of `body`, the text after a tag's `<`, is the rest of that tag, if
/// it's one: through its `>`, past any attribute's quoted value.
///
/// A quote that doesn't close on its line is one bad tag, which mustn't take the
/// rest of the paragraph with it: the tag ends at the next `>` on that line, or
/// without one before the next `<`, or at the line's end. Each kind of quote can
/// fail that way once a line, as none of it is left there, so this stays linear.
/// A later tag is read for its quotes as ever. A tag whose quotes all closed but
/// that has no `>` goes no further than the line of its last one. With no quotes
/// and no `>` before another `<` or the end, it's no tag.
fn tag_len(body: &str) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut after_equals = false;
    let mut last_close = None;
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            b'>' => return Some(at + 1),
            b'<' => break,
            b'"' | b'\'' if after_equals => {
                let rest = &bytes[at + 1..];
                let stop = rest.iter().position(|&c| c == byte || c == b'\n');
                if let Some(to) = stop.filter(|&to| rest[to] == byte) {
                    at += to + 2;
                    after_equals = false;
                    last_close = Some(at);
                    continue;
                }
                let line = &rest[..stop.unwrap_or(rest.len())];
                let len = match line.iter().position(|&c| c == b'>') {
                    Some(close) => close + 1,
                    None => (line.iter().position(|&c| c == b'<')).unwrap_or(line.len()),
                };
                return Some(at + 1 + len);
            }
            _ => {}
        }
        if !byte.is_ascii_whitespace() {
            after_equals = byte == b'=';
        }
        at += 1;
    }
    let last_close = last_close?;
    let line_end = body[last_close..at].find('\n');
    Some(line_end.map_or(at, |to| last_close + to))
}

/// Whether Markdown's `highlight` on text starting with `first` is words: a
/// heading's or a link's text, but not a `[label]`, which keeps its brackets.
fn is_prose(highlight: Highlight, first: Option<char>) -> bool {
    match highlight.name() {
        "markup.heading" => true,
        "markup.link" => first != Some('['),
        _ => false,
    }
}

/// Adds `range` of an inline node starting at `base`, whose text is `source`,
/// less its HTML entities, which the highlights don't paint: `&nbsp;` isn't a
/// word.
fn push_prose(source: &str, base: usize, range: Range<usize>, ranges: &mut Vec<Range<usize>>) {
    let mut from = range.start - base;
    let end = range.end - base;
    if from >= end {
        return;
    }
    let mut push = |piece: Range<usize>| {
        let piece = trimmed(source, piece);
        if has_word(&source[piece.clone()]) {
            ranges.push(base + piece.start..base + piece.end);
        }
    };
    let mut search = from;
    while let Some(at) = source[search..end].find('&') {
        let at = search + at;
        let Some(len) = entity_len(&source[at..end]) else {
            search = at + 1;
            continue;
        };
        push(from..at);
        from = at + len;
        search = from;
    }
    push(from..end);
}

/// How long the HTML entity `text` starts with is: `&amp;`, `&#38;`, `&#x26;`.
fn entity_len(text: &str) -> Option<usize> {
    let body = text.strip_prefix('&')?;
    let name = body.strip_prefix('#').unwrap_or(body);
    let len = name.find(|c: char| !c.is_ascii_alphanumeric())?;
    (len > 0 && name[len..].starts_with(';')).then(|| text.len() - name.len() + len + 1)
}

/// The bytes of each of the block grammar's `inline` nodes touching `window`,
/// in order.
fn inline_nodes(syntax: &Syntax, window: Range<usize>) -> Vec<Range<usize>> {
    let mut nodes = Vec::new();
    let Some(tree) = syntax.tree() else {
        return nodes;
    };
    let mut cursor = tree.walk();
    loop {
        let node = cursor.node();
        let touches = node.start_byte() < window.end && node.end_byte() > window.start;
        if touches && node.kind() == "inline" {
            nodes.push(node.byte_range());
        }
        if touches && node.kind() != "inline" && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return nodes;
            }
        }
    }
}
