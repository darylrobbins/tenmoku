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
    "constant.enum_member",
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
    "type.parameter",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Capture names some grammars use for something in [`NAMES`].
const ALIASES: &[(&str, &str)] = &[
    ("boolean", "constant.builtin"),
    ("character", "string"),
    ("conditional", "keyword"),
    ("delimiter", "punctuation.delimiter"),
    ("exception", "keyword"),
    ("field", "property"),
    ("float", "number"),
    ("function.special", "function.macro"),
    ("include", "keyword"),
    ("method", "function.method"),
    ("namespace", "module"),
    ("parameter", "variable.parameter"),
    ("preproc", "keyword"),
    ("repeat", "keyword"),
    ("storageclass", "keyword"),
    ("string.escape", "escape"),
    ("tag.attribute", "attribute"),
    ("text.literal", "markup.raw"),
    ("text.reference", "markup.link"),
    ("text.title", "markup.heading"),
    ("text.uri", "markup.link.url"),
    ("type.qualifier", "keyword"),
    ("variable.member", "property"),
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
    /// outside the vocabulary (`spell`, …) map to nothing.
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
pub struct HighlightSpan<H = Highlight> {
    pub range: Range<usize>,
    pub highlight: H,
}

/// A run of text with one highlight, or with none where `@none` cleared it.
/// Injected layers keep their cleared runs until painted over the host, so
/// that they clear its highlights too, unlike bytes no capture covers.
pub(crate) type Painted = HighlightSpan<Option<Highlight>>;

/// The runs of `spans` that have a highlight.
pub(crate) fn highlighted(spans: Vec<Painted>) -> Vec<HighlightSpan> {
    spans
        .into_iter()
        .filter_map(|span| {
            span.highlight.map(|highlight| HighlightSpan {
                range: span.range,
                highlight,
            })
        })
        .collect()
}

/// One query capture: a node's range, its highlight, and the precedence of
/// the pattern that captured it. A capture without a highlight, `@none`,
/// clears the highlights of the captures around it, as Scala's query does
/// for code interpolated in a string.
pub(crate) struct Capture {
    pub(crate) range: Range<usize>,
    pub(crate) highlight: Option<Highlight>,
    pub(crate) precedence: usize,
}

/// Flattens nested captures into sorted, non-overlapping spans within
/// `clip`, following tree-sitter's highlighting rules: a node nested in
/// another wins over it, and when several patterns capture the same node the
/// one with the highest precedence wins.
///
/// Byte by byte, that's: of the captures covering the byte, the one starting
/// last wins, then the shortest, then the highest precedence, then the
/// earliest in `captures`; if that one has no highlight, the byte is in a
/// cleared span. A byte no capture covers is in no span.
/// Captures from one tree only nest, but this also holds for ones that
/// partly overlap, as an edited tree's can.
pub(crate) fn flatten(mut captures: Vec<Capture>, clip: Range<usize>) -> Vec<Painted> {
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
    let mut push = |range: Range<usize>, highlight: Option<Highlight>| {
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
    let mut stack: Vec<(usize, Option<Highlight>)> = Vec::new();
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

/// The parts of `spans` inside `ranges`. Both are sorted and don't overlap.
pub(crate) fn clip<H: Copy>(
    spans: Vec<HighlightSpan<H>>,
    ranges: &[Range<usize>],
) -> Vec<HighlightSpan<H>> {
    let mut clipped = Vec::with_capacity(spans.len());
    let mut first = 0;
    for span in spans {
        while ranges
            .get(first)
            .is_some_and(|range| range.end <= span.range.start)
        {
            first += 1;
        }
        for range in ranges[first..]
            .iter()
            .take_while(|range| range.start < span.range.end)
        {
            let range = span.range.start.max(range.start)..span.range.end.min(range.end);
            if !range.is_empty() {
                clipped.push(HighlightSpan {
                    range,
                    highlight: span.highlight,
                });
            }
        }
    }
    clipped
}

/// `top` painted over `base`: each byte takes its highlight from `top` if a
/// span there covers it, even a cleared one, else from `base`. Both are sorted and
/// non-overlapping, and so is the result, with touching spans of one
/// highlight merged as [`flatten`] does.
pub(crate) fn overlay<H: Copy + Eq>(
    base: Vec<HighlightSpan<H>>,
    top: Vec<HighlightSpan<H>>,
) -> Vec<HighlightSpan<H>> {
    if top.is_empty() {
        return base;
    }
    // The parts of `base` that `top` leaves uncovered.
    let mut pieces = Vec::with_capacity(base.len() + top.len());
    let mut first = 0;
    for span in base {
        let mut start = span.range.start;
        while top.get(first).is_some_and(|top| top.range.end <= start) {
            first += 1;
        }
        for top in top[first..]
            .iter()
            .take_while(|top| top.range.start < span.range.end)
        {
            if start < top.range.start {
                pieces.push(HighlightSpan {
                    range: start..top.range.start,
                    highlight: span.highlight,
                });
            }
            start = start.max(top.range.end);
        }
        if start < span.range.end {
            pieces.push(HighlightSpan {
                range: start..span.range.end,
                highlight: span.highlight,
            });
        }
    }
    pieces.extend(top);
    pieces.sort_by_key(|span| span.range.start);

    let mut merged: Vec<HighlightSpan<H>> = Vec::with_capacity(pieces.len());
    for span in pieces {
        match merged.last_mut() {
            Some(last)
                if last.highlight == span.highlight && last.range.end == span.range.start =>
            {
                last.range.end = span.range.end;
            }
            _ => merged.push(span),
        }
    }
    merged
}
