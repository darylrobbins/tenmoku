---
task_uid: "01m4p9g3cq28q9fsvgh3yksp44"
task: "T52"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-lsp/src/tests/decode_cost.rs builds in the gate and its #[ignore]d test runs the four cases through LspNavigation and the fake server"
    required: true
    manual: false
  - id: "C2"
    text: "PERF.md's Navigation section has a 'Language server answers: locations in unopened files' table (fastest and median of 10) compared with the 100 ms go-to-def target and search references' completion"
    required: true
    manual: false
  - id: "C3"
    text: "lsp-client.md §10's conversion-cost item points at that table; crates/fe-lsp/src/navigation.rs is unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "The PERF.md table names the host and commit it was measured on (neon, Linux, the CPU model from /proc/cpuinfo, and the commit SHA), since the other Navigation numbers are from the Mac"
    required: true
    manual: false
---

# Measure decoding language-server locations in unopened files against the navigation budgets (Mac T412)

## Problem

`docs/decisions/lsp-client.md` §10 Unanswered (line 1963): "Cost of the cross-file offset conversion for
references in unopened files. Each referenced file is loaded once per answer (§5). Not measured against
T108's budgets."

`LspNavigation`'s `Decoder::decode` (`crates/fe-lsp/src/navigation.rs:643`) handles each location in a file
that isn't open. On the calling thread, it reads the whole file with `load` (`navigation.rs:707`:
`Text::from_reader_detecting`, up to `DEFAULT_MAX_FILE_BYTES`, 50 MB), then converts each range with
`from_lsp`. T108's budgets (PERF.md Navigation) are go-to-def under 100 ms, while search-based references
complete in about 703 ms.

## Change

Measurement only. No change to `crates/fe-lsp/src/navigation.rs`.

- `crates/fe-lsp/src/tests/decode_cost.rs`, declared in `crates/fe-lsp/src/tests/mod.rs`, holds an
  `#[ignore]`d test that prints timings. It drives `LspNavigation` with the fake server that
  `src/tests/navigation.rs` uses. Run it with `cargo test --release -p fe-lsp decode_cost -- --ignored
  --nocapture`. Cases, all files unopened and in a temp dir:
  - a definitions answer of 50 locations in 50 files of about 4 KB;
  - a references answer of 1,000 locations in 500 files of about 4 KB, the nav bench's corpus shape;
  - 1,000 locations in one 10 MB file;
  - 1,000 locations in one 10 MB file of non-ASCII text, decoded as UTF-16.
  Each case times the request from the server's reply to the decoded answer, fastest of 10 runs and median.
- PERF.md's Navigation section gains a "Language server answers: locations in unopened files" table with
  those numbers. It compares the definitions case with the 100 ms go-to-def target and the references cases
  with search's 703 ms completion. If a case is over target, name the follow-up to file.
- In `lsp-client.md` §10, replace the "Not measured" item with a pointer to that table.
