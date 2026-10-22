---
task_uid: "01m55v1feqf7hn4pthbkz6qbnf"
task: "T186"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test lsp::trust_tests::lenses::server_lenses_show_above_their_rows passes: a lens with a command and one resolving to 2 references show as one lens row above row 3; an edit then 250 ms asks again and the old row stays until the answer; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit tests lsp::trust_tests::lenses::clicking_a_lens_runs_its_command and lsp::trust_tests::lenses::refresh_and_the_setting pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show language servers' code lenses as clickable rows above their lines, resolved, refreshed and running their commands, behind editor.code_lens, as VS Code does (Mac T948)

## Problem

LSP completeness track, item 7 (user-approved 2026-10-17). Once the cards this depends on land:

- T858 gives editors clickable lens rows from `LensProvider`s (`crates/fe-gpui/src/lens.rs`): `Lens { row,
  items: Vec<LensItem { title, invocation }> }`, a click dispatching the item's `fe_command::Invocation`;
- fe-lsp asks for, resolves and runs code lenses (`LspManager::code_lenses`, `resolve_code_lens`,
  `run_lens_command`) and reports `workspace/codeLens/refresh`;
- `editor.code_lens` (default true) exists.

Nothing connects them. VS Code asks for a document's lenses when it shows and after edits (debounced),
resolves those without a command before drawing them, keeps the old lenses until the new answer lands (so rows
don't jump while typing), and re-asks on refresh; clicking runs the command.

## Change

1. New `crates/fe-workbench/src/lsp_lenses.rs`, declared from `crates/fe-workbench/src/lsp.rs` as
   `#[path = "lsp_lenses.rs"] mod lenses;`, run from `Workbench::sync_lsp`:
   - Per buffer on a server with a `codeLensProvider`, with `editor.code_lens` on: ask when it opens on its
     server, 250 ms after its last edit, and at once on `Refresh { what: CodeLens }` from its server. Lenses
     without a command are resolved (all of an answer's, concurrently) before the answer is shown; one that
     resolves to no command is dropped. A landed answer replaces the buffer's lenses only if it is for the
     buffer's current version; until then the old ones show, their rows following edits (anchored).
   - One `LensProvider` per buffer, added to each of its editors: a lens row per buffer row with the lenses
     starting there, in the server's order; each item's title is the command's title, its invocation
     `lsp.run_lens` with `{server, lens}` args (the lens kept in a table by id).
   - New command `lsp.run_lens` (context `editor`, not in the palette or menus): runs the lens's command:
     the client commands of the references card when it lands; otherwise `run_lens_command`; a command neither
     knows (`Err(Unavailable)`) shows an info toast "<title>: this command isn't supported", as VS Code reports
     an unknown command.
   - The setting off removes the providers; a buffer leaving its server removes its lenses.
2. `crates/fe-workbench/src/lsp.rs`: the `sync_lsp` call, the `Refresh` arm, the setting read.
   `crates/fe-workbench/src/workbench_commands.rs`: the `lsp.run_lens` command; `typed_command_ids_tests.rs`
   gains its row.
3. Tests: new `crates/fe-workbench/src/lsp_lenses_tests.rs`, declared at the end of
   `crates/fe-workbench/src/lsp_trust_tests.rs` as `#[path = "lsp_lenses_tests.rs"] mod lenses;`, with the fake
   of `tests/lsp.rs`:
   - `server_lenses_show_above_their_rows`: after opening `src/main.rs` the fake gets `textDocument/codeLens`;
     a lens on row 3 with command `Run` and one without (resolving to `2 references`) show as one lens row
     above row 3 with items `Run` and `2 references`; an edit then 250 ms asks again, and the old row stays
     until the answer.
   - `clicking_a_lens_runs_its_command`: the fake lists `rust-analyzer.runSingle`; clicking `Run` sends
     `workspace/executeCommand` with its arguments; a lens with an unlisted command shows the toast and sends
     nothing.
   - `refresh_and_the_setting`: `workspace/codeLens/refresh` asks again at once; `editor.code_lens = false`
     removes the lens rows.

Order: T854 adds a mod line to `lsp_trust_tests.rs` first. Linux-OK.

## Implementation notes

- `editor.code_lens` reaches `Workbench` only through `Workbench::new` (workbench.rs) and `reload_settings` (workbench_root.rs), as `editor.inlay_hints` does, so a one-line read in each sets `lsp.code_lens_on`; these two files are outside the listed scope.
- The references card hasn't landed, so every command the server doesn't list gets the "isn't supported" toast.
- The first ask for a buffer on its server goes at once; later ones wait 250 ms after the last edit.
