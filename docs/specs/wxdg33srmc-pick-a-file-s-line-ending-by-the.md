---
task_uid: "01m54cv9kebktey8wxdg33srmc"
task: "T138"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-text test line_ending_majority::line_ending_follows_the_majority_of_breaks passes: \"a\\nb\\r\\nc\\r\\nd\\r\\n\" reads as CrLf, \"a\\r\\nb\\nc\\nd\\n\" as Lf, \"a\\rb\\rc\\r\" as CrLf and a file with no break as Lf, each through Text::from_file_contents, Text::from_reader and Text::from_reader_detecting with the text normalised to \\n as today; a 200 KB file with one LF break first and a CRLF majority after the first 64 KiB reads as CrLf; with 65,535 and with 65,536 x bytes (a CRLF split between reads for from_reader and for from_reader_detecting), x bytes + \"\\r\\n\" + \"a\\n\" (a tie) reads as Lf and x bytes + \"\\r\\n\\r\\n\" + \"a\\n\" reads as CrLf, through both readers; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-text -p fe-buffer -p fe-editor passes with no existing test edited (tests::normalises_line_endings, which reads \"a\\rb\\n\" as Lf, a tie, stays as it is); cargo clippy -p fe-text --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Pick a file's line ending by the majority of its line breaks, CR-ended against LF, as VS Code does, instead of from its first break (Mac T749)

## Problem

fe-text picks a file's line ending from its **first** line break: `LineEnding::detect`
(crates/fe-text/src/lib.rs, "Guess from the first line break") for `Text::from_file_contents`,
and `Normalizer::push` (same file) for the streaming readers `Text::from_reader` and
`Text::from_reader_detecting`, which is what opening a file uses. A lone CR never counts.

Reproduced at Mac main 80e6c76d (all three readers agree):

| file bytes | fastedit | VS Code |
|---|---|---|
| `a\nb\r\nc\r\nd\r\n` (1 LF, 3 CRLF) | Lf | CRLF |
| `a\r\nb\nc\nd\n` (1 CRLF, 3 LF) | CrLf | LF |
| `a\rb\rc\r` (classic Mac, 3 lone CR) | Lf | CRLF |

The text is normalised to `\n` on load and written back with the detected ending, so saving the first
file rewrites three of its four line breaks, and the status bar shows the minority ending.

VS Code (src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeTextBufferBuilder.ts, `_getEOL`)
counts every break: with `cr`, `lf` and `crlf` counts, it picks CRLF when `cr + crlf` is more than
half of `cr + lf + crlf`, else LF, and the default (LF here) when there is no break.

## Change

In crates/fe-text/src/lib.rs, count the file's line breaks by kind (CRLF, lone LF, lone CR) and pick
`LineEnding::CrLf` when the CR-ended ones (CRLF plus lone CR) are strictly more than half of all
breaks, else `LineEnding::Lf`; a file with no break stays `Lf`, as today.

- `LineEnding::detect(text)` counts over the whole string.
- `Normalizer` keeps the three counts instead of `line_ending: Option<LineEnding>` and
  `decode_stream` picks the ending from them at the end. A CRLF split across two reads (a chunk ending
  in `\r`, the next starting with `\n`, which `ended_with_cr` already handles) counts once, as CRLF;
  a `\r` that ends the file is a lone CR.
- Update `LineEnding::detect`'s doc comment and the module docs to say "by majority, as VS Code does".

Nothing else changes: normalisation to `\n`, `write_to`/`write_encoded`, and fe-buffer's and
fe-editor's use of the detected ending stay as they are. No change outside crates/fe-text.

The regression test is a new integration test, crates/fe-text/tests/line_ending_majority.rs, whose
test fn sits at the top level of the file. It checks the three files above through
`Text::from_file_contents`, `Text::from_reader` and `Text::from_reader_detecting`, plus a 200 KB file
whose CRLF majority starts after the 64 KiB read window and one with a CRLF split exactly at a read
boundary: 65,535 and 65,536 `x` bytes before a `\r\n` split it between reads for `from_reader` (64 KiB reads) and `from_reader_detecting` (a 64 KiB + 1 byte head) respectively.
