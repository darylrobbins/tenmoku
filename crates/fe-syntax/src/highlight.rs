use std::ops::Range;

/// Our highlight vocabulary. Grammar capture names map onto it, and themes
/// colour it. Names are dotted from general to specific, so a theme that only
/// colours `function` also colours `function.method`.
const NAMES: &[&str] = &[
    "attribute",
    "comment",
    "comment.documentation",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "escape",
    "function",
    "function.builtin",
    "function.macro",
    "function.method",
    "keyword",
    "label",
    "markup.heading",
    "markup.link",
    "markup.link.url",
    "markup.raw",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.special",
    "string.special.key",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Capture names some grammars use for something in [`NAMES`].
const ALIASES: &[(&str, &str)] = &[
    ("boolean", "constant.builtin"),
    ("delimiter", "punctuation.delimiter"),
    ("function.special", "function.macro"),
    ("namespace", "module"),
    ("string.escape", "escape"),
    ("text.literal", "markup.raw"),
    ("text.reference", "markup.link"),
    ("text.title", "markup.heading"),
    ("text.uri", "markup.link.url"),
];

/// A highlight from our vocabulary.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Highlight(u8);

impl Highlight {
    /// Every highlight, in index order.
    pub fn all() -> impl ExactSizeIterator<Item = Highlight> {
        (0..NAMES.len() as u8).map(Highlight)
    }

    pub fn name(self) -> &'static str {
        NAMES[self.0 as usize]
    }

    /// Position in [`Highlight::all`], for per-highlight lookup tables.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Maps a grammar's capture name to the most specific highlight it
    /// matches: `function.method.call` becomes `function.method`. Names
    /// outside the vocabulary (`spell`, `none`, …) map to nothing.
    pub fn from_capture(name: &str) -> Option<Highlight> {
        let mut name = name;
        loop {
            let canonical = ALIASES
                .iter()
                .find(|(alias, _)| *alias == name)
                .map_or(name, |(_, canonical)| canonical);
            if let Some(ix) = NAMES.iter().position(|known| *known == canonical) {
                return Some(Highlight(ix as u8));
            }
            name = &name[..name.rfind('.')?];
        }
    }
}

/// Builds a per-highlight table from a theme's styles, indexed by
/// [`Highlight::index`]. Each highlight takes the style of the longest
/// matching name: with `function` and `function.method` styled,
/// `function.builtin` takes `function`'s style.
pub fn resolve_theme<T: Clone>(styles: &[(&str, T)]) -> Vec<Option<T>> {
    Highlight::all()
        .map(|highlight| {
            let name = highlight.name();
            styles
                .iter()
                .filter(|(prefix, _)| {
                    name == *prefix
                        || name
                            .strip_prefix(prefix)
                            .is_some_and(|rest| rest.starts_with('.'))
                })
                .max_by_key(|(prefix, _)| prefix.len())
                .map(|(_, style)| style.clone())
        })
        .collect()
}

/// A run of text with one highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightSpan {
    pub range: Range<usize>,
    pub highlight: Highlight,
}

/// One query capture: a node's range, its highlight, and the precedence of
/// the pattern that captured it.
pub(crate) struct Capture {
    pub(crate) range: Range<usize>,
    pub(crate) highlight: Highlight,
    pub(crate) precedence: usize,
}

/// Flattens nested captures into sorted, non-overlapping spans within
/// `clip`, following tree-sitter's highlighting rules: a node nested in
/// another wins over it, and when several patterns capture the same node the
/// one with the highest precedence wins.
///
/// Byte by byte, that's: of the captures covering the byte, the one starting
/// last wins, then the shortest, then the highest precedence, then the
/// earliest in `captures`. Captures from one tree only nest, but this also
/// holds for ones that partly overlap, as an edited tree's can.
pub(crate) fn flatten(mut captures: Vec<Capture>, clip: Range<usize>) -> Vec<HighlightSpan> {
    captures.sort_by(|a, b| {
        a.range
            .start
            .cmp(&b.range.start)
            .then(b.range.end.cmp(&a.range.end))
            .then(b.precedence.cmp(&a.precedence))
    });
    // Keeps the highest precedence for each node.
    captures.dedup_by(|next, kept| next.range == kept.range);

    let mut spans = Vec::new();
    let mut push = |range: Range<usize>, highlight: Highlight| {
        let range = range.start.max(clip.start)..range.end.min(clip.end);
        if range.is_empty() {
            return;
        }
        match spans.last_mut() {
            Some(HighlightSpan {
                range: last,
                highlight: last_highlight,
            }) if *last_highlight == highlight && last.end == range.start => last.end = range.end,
            _ => spans.push(HighlightSpan { range, highlight }),
        }
    };

    // The captures started so far, latest last, with their ends. Ones that
    // have ended are popped once nothing later is left above them.
    let mut stack: Vec<(usize, Highlight)> = Vec::new();
    let mut pos = clip.start;
    for capture in captures {
        while let Some(&(end, highlight)) = stack.last()
            && end <= capture.range.start
        {
            push(pos..end, highlight);
            pos = pos.max(end);
            stack.pop();
        }
        if let Some(&(_, highlight)) = stack.last() {
            push(pos..capture.range.start, highlight);
        }
        pos = pos.max(capture.range.start);
        stack.push((capture.range.end, capture.highlight));
    }
    while let Some((end, highlight)) = stack.pop() {
        push(pos..end, highlight);
        pos = pos.max(end);
    }
    spans
}
