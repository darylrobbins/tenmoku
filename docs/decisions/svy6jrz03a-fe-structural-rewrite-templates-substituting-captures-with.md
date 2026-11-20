# fe-structural: rewrite templates and the reparse check (T711, structural search SS5)

`Rewrite::compile(&Pattern, template)` and `Pattern::edits(&rewrite, source,
tree, opts, stop)`, in `crates/fe-structural/src/rewrite.rs`. These are the
choices the spec left open, or where the code departs from its wording.

## API

- `Pattern::edits` returns `Result<Edits, EditsError>`, not `Vec<Edit>`:
  the spec also asks for `checked` in the result. `Edits { edits, checked }`
  has `into_pairs()`, and `Edit::into_pair()` gives `(Range<usize>,
  String)`, the shape `fe_project::Replacer::edits` returns.
- `apply_edits(source, &[Edit])` applies edits in order (the check uses it;
  so can callers and tests). `CHECK_LIMIT` is the 1 MiB bound.
- `RewriteError { kind, offset }` mirrors `PatternError`; the offset is the
  metavariable's `$` in the template. Kinds: `UnknownMetavar` (no named
  metavariable of that name in the pattern), `AnonymousMetavar` (`$_`,
  `$_NAME`, `$$$_NAME`: they capture nothing) and `MixedMetavar` (`$A` in
  the template where the pattern has `$$$A`, or the reverse). Mixing is an
  error rather than accepted silently, as the pattern itself refuses it.
- `EditsError::Stopped(Stopped::Cancelled)` when `stop` says so during the
  search or the check. `EditsError::WrongPattern(RewriteError)` (review F4)
  when the rewrite was compiled for another pattern and uses a name this
  one doesn't bind, or binds the other way (single vs multi): checked
  before searching, with the error `Rewrite::compile` would give for this
  pattern, so a mismatch is never a silent empty substitution that deletes
  the captured code. A pattern binding the same names the same way fits. The check never gives `ParseFailed`; a parse that can't run
  gives `checked: false` with nothing flagged, since the edits are still
  right.

## Templates

- Names, lone `$` and `$Abc` follow the pattern scanner's rules (literal
  text). `$$` is a `$`. `$$$` with no name start after it is `$$` and then
  a `$`, so `$$$$` is `$$` and `$$$)` is `$$)`: an anonymous multi means
  nothing in a template, and this makes `$$` literal everywhere it isn't
  the start of `$$$NAME`. `$$A` is a `$` then `A`, as in a pattern.
- A capture is one range (as `Captures` stores it), so single and multi
  substitute the same way: the source slice. An empty multi run is the
  empty string, so `foo()` with `bar(0, $$$ARGS)` gives `bar(0, )`, as the
  criterion's template spells it. A name with no range (a rewrite compiled
  for another pattern) can't reach expansion: `edits` refuses it first.
- Literal text is kept exactly as written (review F1): no indent is added
  to a template's later lines, and whitespace-only lines and the spaces
  ending a literal part before a capture (review F2) stay. The one change
  is line endings (review F3): `\r\n` and a lone `\r` become `\n`, since
  `parse_str` refuses CR and buffers hold `\n` only; `template()` still
  gives the text as written, and error offsets are into it.

## Indentation

- A capture with no newline is copied as is. A multi-line capture's later
  lines have its base indent taken off (the spaces and tabs the file line
  it starts on begins with) and the target indent put on. Where a line
  starts with less than the base indent, or with different whitespace,
  only the common prefix is taken off; deeper indentation stays as the
  file spells it (tabs under a spaces target stay tabs).
- The target is the indent of the output line the capture lands on, as it
  will stand in the file: for the template's first line that is the
  match's start line's indent (the spec's rule); for a later template line
  it is the whitespace that line starts with, as written, since the
  template's own lines aren't moved to the match's depth either (F1). A
  single-line template gives exactly the spec's rule.
- When base and target are equal the capture is copied untouched (the
  common in-place case, `call(|| {…})` → `run(|| {…})`), so string literals
  spanning lines are only changed when the capture really moves. A capture
  that moves does get its multi-line strings' later lines reindented too;
  that's the same trade ast-grep makes.
- A capture line that is empty once the base indent is off gets no
  indent, so empty lines stay empty; any other whitespace is kept.
- Source text is `\n`-only: `fe_syntax::parse_str` refuses CR line endings
  (a debug assertion in fe-text), and buffers are normalized before search.

## Overlap

- Matches come from `find_all` in tree order; a match is dropped if it
  starts before the last kept edit ends (and a second empty match at the
  same place is dropped), so with `nested: true` the inner matches are
  dropped too. Captures are the file's text as it was: a match inside a
  capture (`foo(foo(1, 2), x)`) isn't rewritten in the same pass, which is
  also what a one-match-at-a-time rewrite that resumes after each
  replacement gives (the C6 test checks both).

## Reparse check

- Skipped (`checked: false`) for a file over 1 MiB. No edits: `checked:
  true`, no parse.
- Before: ERROR and MISSING nodes in the given tree (walking only subtrees
  with `has_error`). After: the same count in `parse_str` of the fully
  rewritten text. Only if it rose is each edit applied alone to the
  original and parsed; an edit is flagged when that count exceeds before.
  So at most one parse plus one per edit, each of at most 1 MiB, and the
  per-edit parses only when the rewrite as a whole breaks something.
- An edit that only breaks in combination with another (none alone raise
  the count) is not flagged; with non-overlapping edits this needs one
  edit's text to interact with another's, which a rewrite template rarely
  does. Edits that break alone but fix each other (an opener and a closer)
  are not checked one by one, since the whole didn't rise.
- `stop` is asked before each parse as well as during it: a short parse
  may never call its progress callback.
