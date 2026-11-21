# T712: fe-structural project scan (structural search SS7)

`fe_structural::scan` (crates/fe-structural/src/scan.rs) walks a project with
`fe_project::walk_files` and searches the files of the pattern's language.

## Decisions

- **Language by path only.** A file is searched when
  `fe_syntax::language_name_for_path` names the pattern's language: the name
  compared, without `language_for_path`, which would compile every other
  language's highlight query on first sight. Files picked only by a shebang or
  modeline aren't searched in v1.
- **Prefilter on named leaves, not all of `Pattern::literals()`.** A named
  leaf matches only a node with exactly its text, so its text must be in the
  file. An anonymous token matches by kind alone, and a grammar can give one
  kind several spellings (a case-insensitive keyword aliased to one name), so
  requiring it could skip a file that matches. Pattern extras (comments) are
  skipped when matching, so their text isn't required either.
  `prefilter_literals` is public; `ScanSummary::prefiltered` is false for a
  pattern with none (only metavariables), where every file is parsed.
- **WebAssembly is the caller's to say** (`ScanOptions::wasm`): fe-syntax's
  `Language` doesn't expose whether it is a registered grammar, and the card
  doesn't edit fe-syntax. `threads: None` is min(available, 8), or 4 with
  `wasm`.
- **The sink runs on the calling thread.** The walk (and the parses, on the
  walker's threads) runs on a scoped thread and sends each file with matches
  over a bounded channel; the caller drains what's there into a batch of up to
  64 files. Once `cancel` is set no further batch is delivered and the
  receiver is dropped, so a worker blocked sending gives up. One thread keeps
  the walker's path order.
- **Caps.** `max_matches` is reserved atomically per file; a file that doesn't
  fit is cut and `truncated` set, and the scan stops. Hitting the cap exactly
  isn't truncation. `ScanSummary::matches` counts what reached the sink.
- **Per-file deadline** covers the parse, the search and, with a rewrite, the
  rewrite check; a file past it is dropped and counted in `timed_out` (apart
  from `parse_failed`, which is the parser giving up). The deadline is checked
  before parsing too, so a zero deadline is deterministic.
- **Non-UTF-8**, UTF-16 included, is skipped and counted in `non_utf8`. A
  file holding a NUL byte counts too: BOM-less UTF-16 of ASCII text is valid
  UTF-8 between its zero bytes (review F1), and a NUL marks binary files.
- **Parse failures in tests.** A built-in grammar never fails to parse, so the
  `test-support` feature (on only in this crate's own tests, through a
  self dev-dependency as fe-project does) adds `testing::fail_parses_named`,
  which makes the parse of a file by that name fail as a parser giving up does
  (review F4).
- **Rows** are built under the file's deadline and the cancel flag (asked
  every 256 rows), and only for the matches the cap leaves room for; the line
  and its start are carried forward, so no prefix is scanned twice (review
  F3).
- **Rows.** `start_col` is a byte column; `first_line` is the match's own text
  up to its first line break (a `\r` before it dropped), cut to 512 bytes on a
  char boundary. With a rewrite the edits come from `Pattern::edits` (a second
  search of the file) so nested matches get `rewritten: None` and the reparse
  check's `flagged` carries over. A rewrite compiled for another pattern is
  caught up front (`ScanSummary::rewrite_error`), nothing scanned.

## Known limits

- One file's `find_all` result (and, with a rewrite, its `Pattern::edits` and
  reparse check) is computed whole before the cap cuts it (review F2): a
  pattern matching nearly every node of a 2 MiB file holds that many `Match`es
  until the per-file deadline or the file ends. Bounding it needs a limit in
  `find_all` (matcher.rs) and `edits` (rewrite.rs), outside this card's scope,
  which the card forbids widening: a follow-up. The rewrite check is skipped
  once the cap is reached.
