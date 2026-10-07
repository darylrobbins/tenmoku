---
task_uid: "01m49xn0sd2bdxvj35q6gjebdp"
task: "T154"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-vim test tests::reducer::grammar_folds_counts_registers_and_operators: feeding step() from an empty Pending, 2d3w commits vim.operate {\"operator\":\"delete\",\"target\":{\"motion\":\"word_forward\"},\"count\":6} and d3w gives count 3; 3dd gives target {\"motion\":\"line\"} count 3; \"ayy gives operator yank, target line, register a and no count; \u003e\u003e gives indent with target line; 5j commits vim.move {\"motion\":\"down\",\"count\":5} with extend false; G has no count and 1G has count 1; 0 commits line_start but 10j has count 10; every committed args value deserializes into MoveArgs or OperateArgs and serializes back to the same JSON; each invocation's source is Keyboard"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test tests::reducer::prefixes_targets_cancel_and_forward: dfx, dtx, dFx and dTx give find_forward, till_forward, find_backward and till_backward with char x; ci( and cib give inner_paren, da\" gives a_double_quote and yiW gives inner_big_word; gg gives document_start; after 3, \", \"a, d, g, f and di the result is Wait and Pending::kind() is count, register, register, operator, g, find and text_object respectively; d\u003cEsc\u003e, 3\u003cEsc\u003e, dq, \"! and g\u003cEsc\u003e give Cancel; with nothing pending q, :, v and \u003cC-x\u003e give Forward and \u003cEsc\u003e gives Cancel; 4294967296j saturates the count at u32::MAX without panicking; step() takes Pending by value and no model"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::state::mode_on_the_editor_frame: VimState::stamp(ContextFrame::new(\"editor\")) sets vim.mode to normal on that same frame; after feeding d, vim.mode is operator_pending; after 3 or \"a, vim.mode is normal and the frame has the flag vim.pending and vim.pending_kind count or register; after a commit or a cancel neither vim.pending nor vim.pending_kind is set; the predicate editor \u0026\u0026 vim.mode == normal holds on fe_vim::context(\u0026state) with nothing pending and fails after d; an editor frame that was never stamped has no vim.mode"
    required: true
    manual: false
  - id: "C4"
    text: "fe-vim test tests::commands::move_dispatches_through_the_registry: fe_vim::registry() has vim.move with context editor \u0026\u0026 vim.mode == normal and mutates_buffer false; HeadlessVim::feed(\"5j\") on a 10-row buffer moves the caret 5 rows and produces no transaction; dispatching the same invocation with a context stamped while d is pending gives DispatchError::NotInContext; HeadlessVim::feed(\"q\") returns FeedError::Forwarded"
    required: true
    manual: false
  - id: "C5"
    text: "fe-vim test tests::motions::carets_graphemes_and_goal_columns: with two carets, w and 3l move each caret; no caret ends past the last grapheme of a non-empty line after $, l at line end, or j onto a shorter line; on e\\u{301}xy, l from 0 lands on byte 3; with tab_size 4, j from display column 6 onto a row starting with a tab lands on the grapheme covering display column 6; $ then j keeps the caret at each following line's end; a non-empty selection collapses to its head before moving"
    required: true
    manual: false
  - id: "C6"
    text: "fe-vim test tests::motions::counts_are_bounded_by_the_text: on a 3-row buffer, vim.move invocations with count 4294967295 for down, word_forward and right each return within the test and land on the last row, the last word's start and the line's last grapheme"
    required: true
    manual: false
  - id: "C7"
    text: "fe-vim test motions_match_neovim (tests/oracle.rs): tests/corpus/motions.toml has at least 60 cases covering each of h j k l w b e W B E 0 ^ $ gg G f t F T with and without counts, empty lines, punctuation runs, leading whitespace and tabs, multibyte and combining graphemes, a find that fails, counts past the buffer's ends and the goal column across shorter lines; every case has an entry in motions.expected.json with identical buffer, cursor and keys and nvim \"0.12.4\"; HeadlessVim reproduces each expected buffer, cursor (row, byte column) and mode; the test does not run nvim"
    required: true
    manual: false
  - id: "C8"
    text: "fe-vim test corpus_loader_rejects_bad_cases (tests/oracle.rs): the corpus loader rejects a buffer ending in a newline, keys naming register + or *, keys starting with ., duplicate case names, a case missing from or extra in the expected file, a stale expected entry whose keys differ, and an expected file whose nvim field isn't 0.12.4"
    required: true
    manual: false
  - id: "C9"
    text: "record_corpus_with_neovim (tests/oracle.rs) is #[ignore]d; run by hand with nextest --run-ignored only it fails unless nvim --version reports v0.12.4, runs one nvim --clean --headless -l tests/support/oracle.lua per corpus file, and rewrites only tests/corpus/*.expected.json; oracle.lua sets only tabstop=4 shiftwidth=4 softtabstop=0 expandtab and clipboard=, and before each case wipes the buffer, clears every register and resets the character search; the committed motions.expected.json is that recorder's unedited output (manual: rerunning it leaves git status clean)"
    required: true
    manual: false
  - id: "C10"
    text: "scripts/check-no-gpui.sh prints ok: fe-vim; crates/fe-vim's normal dependencies are exactly fe-buffer, fe-command, fe-editor, fe-text, schemars, serde, serde_json, thiserror and unicode-segmentation; Cargo.lock gains only the fe-vim package; root Cargo.toml and Cargo.lock change only by added lines; no other file outside crates/fe-vim changes (the lane's docs/specs file excepted); no unsafe in crates/fe-vim; the gate's deny stage passes with deny.toml unchanged; src/lib.rs's crate docs state the clean-room rule of the spec"
    required: true
    manual: false
---

# Add fe-vim: a pure normal-mode key reducer, vim.mode on the editor frame, and motions checked against recorded Neovim

# Add fe-vim: a pure normal-mode key reducer, vim.mode on the editor frame, and motions checked against recorded Neovim

Milestone M7 "Vim", card 1 of 3. The design is `docs/decisions/vim-engine.md`. Read §1 (the
"First `fe-vim` card (outline)" and "write our own"), §6 ("How the Neovim oracle runs", "Spot
check"), §8, §9 (items 1–4, 7, 9 and 11) and §10 (all of it) before starting. The outline is too
big for one card under the 1,000-line limit, so it is split:

| Card | Holds | Non-test lines (estimate) |
|---|---|---|
| **1 (this)** | crate, keys, the whole pending-input grammar as a pure reducer, `vim.mode`, `vim.move` and its motions, the oracle harness and recorder, 60+ motion cases | ~950 |
| 2 | `vim.operate` with `d y > <`, the 12 text objects, `%`, `;` and `,`, registers, `x X D p P`, 50+ cases | ~880 |
| 3 | insert sessions as one transaction, `c C s S i a I A o O`, `J r ~`, `u <C-r>`, `.`, the §6 spot check, 150+ cases in all | ~600 |

## Clean room (read before writing anything)

`fe-vim` is written from Vim's documented behaviour (`:help` text and running `nvim`), never from
source. §2 found Vim-licensed C ported into hjkl, so:

- Don't open Vim's or Neovim's source (C, Lua or Vimscript runtime), hjkl's crates (any
  `hjkl-*`, its corpus included), Zed's `vim` crate (GPL, already banned by `CLEAN_ROOM.md`), or
  Helix (MPL).
- Learn behaviour by running `nvim --clean` (0.12.4 is installed here, from the Brewfile's
  `neovim`) and from `:help`. The corpus is the spec: when unsure, add a case and record it.
- `crates/fe-vim/src/lib.rs`'s crate docs state this rule.

## Change

A new GPUI-free crate, `crates/fe-vim`. It is picked up by the root `members = ["crates/*"]`.
Optionally add `fe-vim = { path = "crates/fe-vim" }` to `[workspace.dependencies]` as an
added line (root `Cargo.toml` and `Cargo.lock` are `shared_append_only` in `regatta.toml`, so
this card doesn't claim them). Dependencies, all already in `Cargo.lock`: `fe-buffer`,
`fe-command`, `fe-editor`, `fe-text`, `schemars`, `serde`, `serde_json`, `thiserror`,
`unicode-segmentation`; dev-dependencies `toml` and `serde_json`. No `fe-keymap` or GPUI
dependency, and no change to `deny.toml`.

### Module layout (cards 2 and 3 name these files)

| File | Holds |
|---|---|
| `src/lib.rs` | crate docs (the clean-room rule, the card split, what comes later), `mod` lines, re-exports |
| `src/key.rs` | `VimKey` and `parse_keys` (vim key notation) |
| `src/args.rs` | `Operator`, `Motion`, `TextObject`, `Target`, `MoveArgs`, `OperateArgs` |
| `src/reducer.rs` | `Pending`, `PendingKind`, `Step`, `step()` |
| `src/state.rs` | `Mode`, `VimState`, `stamp`, `context` |
| `src/motion.rs` | motion resolution over an `EditorModel`'s text |
| `src/commands.rs` | `VimCommands`, `VimTarget`, `registry()` |
| `src/headless.rs` | `HeadlessVim`, the driver the tests and the oracle harness use |
| `src/tests/mod.rs` + `reducer.rs`, `state.rs`, `motions.rs`, `commands.rs` | unit tests (`tests::<module>::<fn>`) |
| `tests/oracle.rs` | the corpus tests and the ignored recorder |
| `tests/support/mod.rs`, `tests/support/oracle.lua` | the harness and the nvim driver |
| `tests/corpus/motions.toml`, `tests/corpus/motions.expected.json` | cases and their recorded results |

### Keys (`src/key.rs`)

`VimKey` is what the reducer reads: `Char(char)` (the typed character, case kept: `A`, `$`,
`é`), `Ctrl(char)` (lowercase letter), `Esc`, `Enter`, `Backspace`, `Tab`, `Left`, `Right`, `Up`,
`Down`. It isn't `fe_keymap::Keystroke`, which lowercases letters and has no typed text. The GPUI
wiring card converts GPUI keystrokes to it. `parse_keys(&str) -> Result<Vec<VimKey>, KeyError>`
reads vim notation: plain characters, `<Esc>`, `<CR>`, `<BS>`, `<Tab>`, `<C-x>`, `<lt>`, arrow
names, case-insensitive inside `<…>`.

### Args (`src/args.rs`)

Serde (snake_case, externally tagged, the shape of `docs/decisions/command-model.md`'s "Vim
composition" table) and `JsonSchema`:

- `Operator`: `delete`, `yank`, `change`, `indent`, `outdent` (`d y c > <`).
- `Motion`: `left right up down` (`h l k j`), `word_forward word_backward word_end` (`w b e`),
  `big_word_forward big_word_backward big_word_end` (`W B E`), `line_start` (`0`),
  `first_non_blank` (`^`), `line_end` (`$`), `document_start` (`gg`), `document_end` (`G`),
  `find_forward find_backward till_forward till_backward` each carrying the char (`f F t T`),
  and `line` (the target of a doubled operator such as `dd`: `count` lines from the caret's,
  like Vim's `_`). Card 2 adds `match_pair`, `repeat_find` and `repeat_find_reverse`.
- `TextObject`: `inner_word a_word inner_big_word a_big_word inner_paren a_paren inner_brace
  a_brace inner_double_quote a_double_quote inner_single_quote a_single_quote`.
- `Target`: `{"motion": Motion}` or `{"text_object": TextObject}`. (Not `fe_command::Target`.)
- `MoveArgs { motion, extend: bool (default false; visual mode uses it later), count: Option<u32> }`.
- `OperateArgs { operator, target, count: Option<u32>, register: Option<char> }`.

`count` is `Option` so `G` and `1G` differ (§9.3).

### The reducer (`src/reducer.rs`, §9.1 and §10 step 2)

```rust
pub fn step(pending: Pending, key: VimKey) -> Step;
pub enum Step { Wait(Pending), Commit(Invocation), Cancel, Forward }
```

- **Pure.** It takes the pending value and a key, and never sees a model or buffer (§1:
  "It never touches the buffer").
- **One pending value** holds the whole incomplete command: count, `"{reg}`, operator, second
  count, and a prefix awaiting its next key (`g`, `f`/`t`/`F`/`T`, `i`/`a` after an operator).
  `Pending::kind() -> Option<PendingKind>`: `count`, `register`, `operator`, `g`, `find`,
  `text_object`. `None` means nothing is pending.
- **Grammar** for the normal-mode keys of this card: counts (`0` is `line_start` unless a count
  is being typed), `"` then a register name (`" 0-9 - a-z A-Z _ + *`; anything else cancels),
  operators `d y c > <`, a doubled operator (`dd`, `yy`, `cc`, `>>`, `<<`) as target
  `{"motion":"line"}`, the motions above, `gg`, `f t F T` plus any `Char`, and after an
  operator `i`/`a` plus `w W ( ) b { } B " '` as text objects. `2d3w` folds to count 6.
  Counts saturate at `u32::MAX` rather than overflow.
- **Commits:** a motion with no operator commits `vim.move`; an operator with its target
  commits `vim.operate`. The invocation has `source: Keyboard`; the caller stamps its context.
  Cards 2 and 3 add their keys to this grammar.
- **Cancel and Forward** (§9.1, §9.11, §10):
  - With something pending, `<Esc>` or any key that doesn't continue the command is `Cancel`:
    the pending value is dropped and nothing runs.
  - With nothing pending, `<Esc>` is `Cancel`, and a key outside the grammar (`q`, `:`, `/`,
    `v`, `Ctrl` keys other than this grammar's) is `Forward`, so it propagates (§10 step 2).
- `step` is only for normal and operator-pending mode. In insert mode the caller doesn't call it
  (§10 step 3); card 3 adds that mode.

### Mode on the editor frame (`src/state.rs`, §9.2, §10)

- `Mode`: `Normal` here (card 3 adds `Insert`; visual modes come later).
- `VimState` (one per editor): the mode, the pending value, and per-caret goal columns.
  `VimState::feed(&mut self, key) -> Step` runs `step` and keeps the new pending value.
- `vim.mode` is `normal`, or `operator_pending` while an operator is pending.
- `VimState::stamp(&self, frame: ContextFrame) -> ContextFrame` sets, **on the editor's own
  frame** (§10: "publishes it as `vim.mode` on the focused editor's context frame"):
  - `vim.mode`
  - the flag `vim.pending` and `vim.pending_kind` while anything is pending, a bare count or a
    completed `"a` included (§9.1, so a host can keep keymap bindings off while a command is
    incomplete)
- `fe_vim::context(&VimState) -> ContextStack` is `fe_editor::commands::context()` with its
  editor frame stamped.
- With vim off nothing stamps the frame, so `vim.mode` is never set (§10).

### Motions (`src/motion.rs`) and `vim.move` (`src/commands.rs`)

- `VimCommands` is a `CommandTarget` marker (the house pattern of fe-workbench's
  `OutlineCommands`/`DockCommands`): `Tx = fe_buffer::TransactionId`, `Target<'a> =
  VimTarget<'a> { model: &'a mut EditorModel, host: &'a mut dyn EditorHost, state: &'a mut
  VimState }`.
- `fe_vim::registry() -> Registry<VimCommands>` registers `vim.move` with
  `register_with_args::<MoveArgs>`, context `editor && vim.mode == normal`, and
  `mutates_buffer(false)`.
- **Normal-mode cursor.** A caret sits at the start of a grapheme. On a non-empty line it is
  never past the last grapheme; on an empty line it is at column 0. A non-empty selection
  collapses to its head first.
- **Units (§9.9).** Counts step graphemes. Goal columns for `j`/`k` are display columns, with
  tabs expanded to `model.tab_size()`. The reducer never carries char indices.
- **Motions.**
  - `h`/`l` stay on the line (nvim's default `whichwrap` doesn't include them).
  - `w b e W B E` follow Vim's word classes: an empty line is a word, and they cross lines.
  - `0`, `^`, and `$` (with a count, `count - 1` lines down; it sets the goal to the line end,
    so `$j` stays at line ends).
  - `j`/`k` keep the goal column (`curswant`) per caret in `VimState`. The goal resets on any
    other motion, or when the carets aren't where vim last left them.
  - `gg`/`G`: a count goes to that line, clamped. nvim's default `startofline` is off, so
    they keep the goal column (§6, row 6).
  - `f t F T` stay on the line, take a count, and leave the caret alone when there's no
    match. Remember the last find in `VimState` for card 2's `;`/`,`.
- **Every caret moves** (`EditorModel` is multi-cursor; §1 point 4). The oracle checks one
  caret; a unit test checks two.
- **Bounded by the text, not the count.** `j`/`k`/`gg`/`G` compute the row directly, and
  repeated motions stop once a step doesn't move. A script's `count` of 4,294,967,295 finishes
  promptly.
- Moves set selections through `EditorModel::set_selections`. `fe-editor` is not changed:
  everything here uses its public API.

### `HeadlessVim` (`src/headless.rs`)

`HeadlessVim::new(text: &str, cursor: (u32, usize))` owns an `EditorModel`, a
`HeadlessHost`, a `VimState` and the registry. `feed(&mut self, keys: &str) -> Result<(),
FeedError>` parses the keys and routes each one as §10 step 2 does. On `Commit` it stamps the
invocation with `context(&state)`, taken after the reducer has cleared its pending value, and
dispatches it. `Forward` is `FeedError::Forwarded(key)`, so a corpus case can't silently pass
over a key nothing handled, and a dispatch error is `FeedError::Dispatch`.
`dispatch(&mut self, &Invocation)` runs one invocation as given. It also has accessors for the
text, the primary cursor as (row, byte column), the mode name, the state and the model. Tests
go through `HeadlessVim` rather than building a `VimTarget`, so later cards can add fields to
the target without editing this card's tests.

### The oracle (`tests/`, §1 and §6)

The design goal: the gate needs no nvim, and the expectations are nvim's, not hand-written.

- **Corpus.** `tests/corpus/<area>.toml` holds `[[case]]` tables with `name`, `buffer`,
  `cursor = [row, byte_col]` and `keys` (vim notation). Hand-written. The loader rejects:
  - a buffer ending in `\n` (lines are joined with `\n`, as nvim holds them)
  - keys naming register `+` or `*` (headless nvim would use the real macOS clipboard; card 2
    tests those registers in unit tests)
  - keys starting with `.`, since nvim's last change can't be reset between cases
  - duplicate names
- **Expectations.** `tests/corpus/<area>.expected.json`, written only by the recorder, is
  `{"nvim": "0.12.4", "cases": [{name, buffer, cursor, keys, expect: {buffer, cursor, mode,
  registers}}]}`. `registers` maps each non-empty register of `" 0-9 - a-z` to `{text,
  linewise}`. The inputs are repeated, so an edited case without a re-record fails as stale.
- **The gate test** `motions_match_neovim`, in `tests/oracle.rs`:
  - Loads `motions.toml` and its expected file, and fails on a missing, extra or stale case, or
    on an `nvim` field other than the pinned `0.12.4`.
  - Drives each case through `HeadlessVim`, and compares the buffer, the cursor and the mode
    (nvim's `n` is `normal`).
  - Card 2 adds the register comparison. This card records registers but doesn't compare
    them yet.
  - It doesn't run nvim.
- **The recorder** `record_corpus_with_neovim`, `#[ignore = "needs nvim 0.12.4; rewrites
  tests/corpus/*.expected.json"]`, is run by hand:
  `cargo nextest run -p fe-vim --run-ignored only -E 'test(=record_corpus_with_neovim)'`.
  - It fails unless `nvim --version`'s first line is `NVIM v0.12.4`.
  - For each corpus file it writes the cases as JSON to a temp file, then runs one
    `nvim --clean --headless -l tests/support/oracle.lua <in.json> <out.json>`. nvim's own
    messages go to stderr and are ignored.
  - It rewrites only `*.expected.json`. No `nvim-rs`: it is LGPL-3.0 (§1).
- **`oracle.lua`** reads cases with `vim.json`. It sets only `tabstop=4 shiftwidth=4
  softtabstop=0 expandtab` (to match `fe-editor`'s default tab size of 4) and `clipboard=`;
  everything else is `--clean`'s defaults.
  - Before each case it wipes the buffer, clears every register and calls
    `setcharsearch({char = ''})`.
  - It sets lines and cursor, then calls `nvim_feedkeys(nvim_replace_termcodes(keys), "xt",
    false)`.
  - It records lines, `nvim_win_get_cursor` (byte column), `nvim_get_mode().mode`, and
    `getreg`/`getregtype` for the registers above.
  - A probe of this driver on this Mac took about 50 ms per file. It reproduced §4's results
    for `d3w` and `ciwXY<Esc>`, and moved `l` over `e\u{301}` as one grapheme. Registers carry
    over between cases in one process unless cleared, which is why it clears them.
- **`motions.toml`** has at least 60 cases. They cover:
  - each of `h j k l w b e W B E 0 ^ $ gg G f t F T`, with and without counts
  - empty lines, punctuation runs, leading whitespace and tabs
  - multibyte and combining graphemes
  - a find that fails, and counts past the buffer's ends
  - `$j` and `j` across shorter lines (the goal column)

## Out of scope

- Operators, text objects, registers and `x X D p P %;,` (card 2). Insert mode, `c`, `J r ~`,
  `u <C-r>` and `.` (card 3). Visual modes, macros, marks, search and ex (later M7 cards, per §1).
- **The GPUI wiring** is a separate later card, not filed yet: §10's key routing in
  `crates/fe-gpui/src/keymap.rs`, no `EntityInputHandler` in normal mode, a vim layer in the
  default keymap, the `vim.enabled` setting and "Toggle Vim Mode" command, and cursor shapes
  (§9.7). This card changes no fe-gpui, fe-keymap, fe-workbench, fe-config, fe-editor or
  fe-command file.
- Importing hjkl's corpus data (§1 allows it under `CLEAN_ROOM.md`; this milestone doesn't, so
  nobody needs to open hjkl).

## Decided (lead defaults; the user may revisit)

- **`vim.move` and `vim.operate`, in fe-vim's own `Registry<VimCommands>`,** rather than
  `editor.move`/`editor.operate` as §1 and `command-model.md` sketch:
  - `editor.move` already exists, with macOS motions (`word_right` wraps lines; `left` crosses
    line breaks).
  - Vim's handlers need `VimState` (goal columns, the last find, and registers in card 2),
    which `EditorTarget` doesn't carry.
  - A later card can promote them to `editor.*` when a non-vim user appears.
- **Mode and pending state live in `VimState`,** stamped onto the editor frame (§10), not in a
  field of `EditorModel`, so `fe-editor` doesn't change.
- **Committed expectations.** The gate compares against recorded nvim output and never needs
  nvim; re-recording is a manual, ignored test pinned to nvim 0.12.4.
- **Oracle options** `ts=4 sw=4 sts=0 et`, otherwise `--clean`.
