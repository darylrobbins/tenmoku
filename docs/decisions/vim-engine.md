# Vim engine: adopt, fork or write our own `hjkl`

Status: **decided: write our own** `fe-vim`, clean room, checked against
headless Neovim. Evaluated 2026-10-05. Source card T70 (foundation card 2,
the M7 spike).

**What was read.** The `kryptic-sh/hjkl` workspace at tag `v0.42.1`, commit
`8c078c53c233a9a9e3aae5bb0ef9f9d45955143d` (2026-09-21). That is also `main`
as of 2026-10-05. The crates.io tarballs of `hjkl-engine`, `hjkl-buffer`,
`hjkl-keymap`, `hjkl-editor` and `hjkl-ex` 0.42.1 record the same commit in
`.cargo_vcs_info.json`, and their `src/` is byte-identical to the repo
(`diff -rq`). The vim FSM no longer lives in `hjkl-engine`: it moved into
`hjkl-vim` (hjkl issue #267, `hjkl-engine/src/lib.rs:67`). So `hjkl-vim` and
`hjkl-vim-types` 0.42.1 were read as well.

Everything ran outside this repository, in a scratch directory: a clone, the
tarballs, a throwaway probe crate (`hjkl-*` `=0.42.1`) and hjkl's own test
suites. This task changes no `Cargo.toml` or `Cargo.lock` here.

**Paths.** `crates/…` and `apps/…` paths are relative to the hjkl repo at
`8c078c5`. To open one, prefix
`https://github.com/kryptic-sh/hjkl/blob/8c078c53c233a9a9e3aae5bb0ef9f9d45955143d/`
and append `#L<line>`. Paths under `fastedit/` are this repo.

## 1. Summary and recommendation

**Recommendation: write our own.** hjkl works well, but it can't be adopted
and shouldn't be forked:

1. **Provenance.** Parts of the engine say they port Vim's C code. `ap`
   is "a faithful port of nvim's `current_par()` (src/nvim/textobject.c)".
   Word, paragraph and sentence motions are built on Vim's internal
   `fwd_word()`, `cls()`, `findpar()` and `findsent()` (§2). That code comes
   from Vim and is under the Vim licence, which isn't on our allowlist.
   Forking would copy it into proprietary files. Adopting would ship it
   under an MIT label that may not cover it.
2. **It owns the buffer.** The FSM is concrete on `hjkl_buffer::View`, a
   `ropey` 1.6 rope with its own undo tree, marks and folds. `View` is
   sealed. Making the editor generic is what hjkl's own notes call a
   "multi-thousand-LOC patch" (§3, §5). Adopting it means running a second
   copy of every document and syncing the two. That costs memory on the
   large files we are built for, and it bypasses `Origin`-scoped undo,
   anchors and AI edits.
3. **It doesn't emit actions.** It mutates its own buffer and reports
   *edits* afterwards: `ContentEdit`s, plus an opt-in change log of
   position-range replacements. It never reports *invocations*: `d3w`
   comes out as "delete 0..14", not as `operate { Delete, WordFwd, 3 }`
   (§3, §4). Getting `editor.operate` / `editor.move` invocations out of it
   means rewriting its dispatch, which is a fork.
4. **Units and cursors don't match ours.** Columns are chars, not
   graphemes. `l` stops on a combining mark and `x` deletes the mark alone
   (§5). Vim drives one caret, while `EditorModel` is multi-cursor and
   byte-offset.
5. **Churn.** There have been 104 releases in 5½ months, across 31 breaking
   minor lines. One maintainer wrote 2,353 of 2,383 commits (§7).

What we keep from hjkl:

- The proof that a Neovim oracle works. Of its 1,049 corpus cases, the 888
  that run in-process were compared with nvim 0.12.4 here, and all passed.
  The other 161 need the `hjkl` app binary and were skipped (§6).
- Its feature checklist (`crates/hjkl-compat-oracle/COVERAGE.md`) as a scope
  list.
- Optionally, its MIT corpus *data* (initial buffer, keys, expected result).
  That data is Neovim behaviour, not code. Copy it only per `CLEAN_ROOM.md`:
  attribution header plus a `THIRD_PARTY.md` row. Don't open its engine
  source while writing ours.

**Rough size of each option** (non-test Rust lines, comments included;
hjkl's are dense):

| Option | Ours to write or own | Notes |
|---|---|---|
| Adopt | ~2–4k lines of glue, plus 23 lockfile entries | A `Host` impl, key translation, two-way sync between `hjkl_buffer::View` and `fe_buffer::Buffer`, and mapping mode and selection. Still no `editor.operate` invocations, and every document is held twice. |
| Fork | ~38k lines copied, then ~8–10k changed | Copy `hjkl-engine` 13.8k + `hjkl-vim` 18.2k + `hjkl-buffer` 5.5k + `hjkl-vim-types` 0.6k (plus ~16k inline tests). Then make it generic over our buffer, rewrite the Vim-derived motions and objects clean room, strip the app features (LSP intent, blame, syntax spans, sneak, comment, reflow, folds), and move to byte and grapheme units. |
| Write our own | ~8–12k lines | Grammar/pending reducer ~1.5k, motions ~2.5k (some already in `EditorModel::move_cursors`), text objects ~1.5k, operators + registers ~1.5k, visual char/line/block ~1.5k, insert session/dot/macros/marks/search ~1.5k, ex subset with ranges (`:s :g :d :m :t :normal :w :q`) ~1.5–2k. Plus a ~0.6k oracle harness and the ~500 cases M7 requires. |

Measured hjkl sizes: `fd -e rs . <crate>/src`, with lines from a
`#[cfg(test)]` marker to end of file counted as tests. Non-test lines are
engine 13,825, vim 18,206, buffer 5,532, vim-types 590, ex 6,864 and keymap
911.

**First `fe-vim` card (outline).** *"fe-vim: normal-mode grammar over the
command registry, checked against headless Neovim."*

- New GPUI-free crate `fe-vim`, depending on `fe-command` (card 1a) and
  `fe-editor`.
- A pure pending-input reducer:
  `step(state, key) -> Wait(state) | Commit(Invocation) | Cancel | Forward`.
  It covers count, `"{reg}`, operator, a second count, motion or text
  object, and `f`/`t` targets. It emits
  `editor.operate { operator, target, count, register }` and
  `editor.move { motion, extend, count }`. It never touches the buffer.
- Initial set:
  - motions `h j k l w b e W B E 0 ^ $ gg G f t F T ; , %`
  - operators `d c y > <` with charwise/linewise/inclusive rules
  - text objects `iw aw iW aW i( a( i{ a{ i" a" i' a'`
  - `x X D C s S J r ~ p P u <C-r> . o O i a I A`
- Registers `" 0 1-9 - a-z A-Z _ + *` as a service in `fe-vim` (`+`/`*`
  through the host clipboard).
- An insert session is one `fe_buffer` transaction, started by the
  operator that opened it.
- Dot-repeat of the last mutating invocation plus its inserted text.
- Mode stored on the editor and exposed as `vim.mode`.
- Oracle: a TOML corpus (initial buffer, cursor, keys; expected buffer,
  cursor, mode, register) and a dev-only harness. The harness runs one
  `nvim --clean --headless -l oracle.lua` per corpus file and compares
  against `fe-vim` driven headlessly. Don't use `nvim-rs`: it is
  LGPL-3.0. A Lua driver needs no Rust dependency; §6 shows it working.
- Exit: about 150 cases green against nvim 0.12, the spot-check commands of
  §6 among them.

Visual modes, macros, marks, search, ex and `.` across visual come in later
cards.

## 2. Licence and provenance

**Licence metadata and `LICENSE` files.** All evaluated crates say
`license = "MIT"`.

| Crate | `Cargo.toml` `license` | `LICENSE` in tarball | Same as repo `LICENSE`? |
|---|---|---|---|
| hjkl-engine 0.42.1 | MIT | MIT | identical |
| hjkl-buffer 0.42.1 | MIT | MIT | identical |
| hjkl-keymap 0.42.1 | MIT | MIT | differs only in the copyright line, `mxaddict <mxaddict@kryptic.sh>` vs `mxaddict` |
| hjkl-editor 0.42.1 | MIT | MIT | identical |
| hjkl-ex 0.42.1 | MIT | MIT | identical |
| hjkl-vim 0.42.1 (holds the FSM) | MIT | MIT | not compared |
| hjkl-vim-types 0.42.1 | MIT | **none** | — |
| hjkl-lang 0.42.1 (engine dep) | MIT | **none** | — |

The repo `LICENSE` is "MIT License, Copyright (c) 2026 mxaddict".
`hjkl-vim-types` and `hjkl-lang` ship no licence text in their tarballs. MIT
requires the notice to travel with copies, so a fork of those would take the
text from the repo root.

**Runtime dependency licences.** A probe crate depending on all five crates
plus `hjkl-vim` at `=0.42.1` resolves 81 packages. I ran
`cargo deny check licenses` against a copy of our `deny.toml`:

```
licenses ok
```

So nothing falls outside our allowlist. The only licence that isn't on the
plain allowlist is `option-ext` 0.2.0, MPL-2.0. It is already a
crate-by-crate exception in our `deny.toml` and already in our lock through
`zed-font-kit`. Others worth noting, all allowed: `libloading` 0.9.0 (ISC),
`zerocopy` (BSD-2-Clause/Apache/MIT) and `walkdir`/`winapi-util`
(Unlicense/MIT).

Metadata can't show what §2's last question finds: code *inside* the hjkl
crates that is derived from Vim.

**Ported code (Vim/Neovim C, Zed, Helix).**

- **Neovim/Vim C source: yes.** Per the clean-room rule, I stopped reading
  each of these at the doc comment and didn't read the function bodies.
  - `crates/hjkl-vim/src/vim/text_object.rs:2038-2041`: "`ap` ('around
    paragraph'): faithful port of nvim's `current_par()` operator path …
    (src/nvim/textobject.c, nvim 0.12.5). Line numbers below are 1-based to
    mirror nvim". Added in `1b37d77f` (2026-08-30).
  - `crates/hjkl-vim/src/vim/text_object.rs:838`: "Flat port of vim's
    `findsent(FORWARD, 1)`". Added in `d15da589`.
  - `crates/hjkl-vim/src/vim/text_object.rs:2150`: the `current_par`
    "extend" path (`textobject.c`).
  - `crates/hjkl-engine/src/motions.rs:251` (`findpar()`), `:572`
    (`hasFolding()`), `:603`/`:621` (`fwd_word()`, `end_word()`,
    `bck_word()`), `:1271` (`dec()`), `:1295` (`inc()`) and `:1309`
    (`cls()`). These are helpers named after, and documented as, Vim's own
    internal functions in `search.c`/`textobject.c`. They were added in
    `3ac77c0a` (2026-08-04) and `5462536c` (2026-09-16).
  - `crates/hjkl-vim/src/vim/op_motion.rs:79,149` (`end_word()`,
    `do_pending_operator()`) and `crates/hjkl-vim/src/vim/operator.rs:78`
    (`nv_wordcmd()`) cite Vim functions for behaviour. I didn't read
    further to tell a citation from a port.

  These functions date from Vim, so the Vim licence applies even where
  Neovim's copy was read. Neovim's own Apache-2.0 grant covers only code
  contributed to Neovim. The Vim licence isn't on our `deny.toml`
  allowlist: it puts conditions on distributing modified copies. And
  `CLEAN_ROOM.md` requires everything we ship to be ours or permissively
  licensed.
- **Zed: none found.** The evaluated crates never mention Zed (`rg -i zed`).
- **Helix: no ports found.** It is mentioned only as a design comparison.
  `crates/hjkl-buffer/src/buffer.rs:18` says "the Helix Document+View
  model"; `crates/hjkl-engine/src/selection_shift.rs:60` and
  `crates/hjkl-engine/src/editor.rs:795` name a `HelixState` discipline that
  isn't among these crates. Nothing says it was copied.
- **Origin.** hjkl was "imported wholesale from sqeel-vim with full git
  history" (`crates/hjkl-engine/src/lib.rs:11`), and `hjkl-buffer` was
  extracted from `sqeel-buffer` (`crates/hjkl-buffer/src/lib.rs:7`). Both
  are the same author's earlier projects. The first commit is 2026-04-23,
  "extract vim engine into sqeel-vim workspace crate".

## 3. API shape

**How input is fed.** One key at a time:

```rust
hjkl_vim::dispatch_input(&mut Editor<hjkl_buffer::View, H>, hjkl_engine::Input) -> bool
```

(`crates/hjkl-vim/src/lib.rs:90`.) `Input` is
`{ key: Key, ctrl, alt, shift }`, with
`Key = Char(char) | Backspace | Enter | Esc | arrows …`
(`crates/hjkl-engine/src/input.rs:12,34`). There is also a planned
`types::Input` enum (`Char | Key | Mouse | Paste | Focus… | Resize`,
`crates/hjkl-engine/src/types.rs:973`), reached through `feed_input`
(`crates/hjkl-vim/src/lib.rs:56`). It drops mouse, paste, focus and resize.

In Normal, Visual and operator-pending modes, every key is consumed
("unknown keys swallow silently", `crates/hjkl-vim/src/normal.rs:19-20`).
Macros are encoded and decoded as vim key notation
(`hjkl_engine::decode_macro`, `encode_macro`, `input.rs:44,83`).

**What comes out.** Mutation in place. The FSM edits its own
`hjkl_buffer::View` through `Editor::mutate_edit`
(`crates/hjkl-engine/src/editor.rs:2637`). Hosts observe after the fact
through these:

- `take_content_edits() -> Vec<ContentEdit>` (`editor.rs:3221`): byte
  offsets in tree-sitter `InputEdit` shape
  (`crates/hjkl-buffer/src/engine_types.rs:83`).
- `take_content_change()` (`editor.rs:3243`): the whole document as an
  `Arc<String>`.
- `take_changes() -> Vec<Edit>` (`editor.rs:3809`), an **opt-in** change
  log. "Recording is opt-in: nothing in hjkl reads this log, so it defaults
  OFF … A host that wants the log calls
  `buffer_mut().set_change_log_enabled(true)` once" (`editor.rs:3794-3797`,
  `crates/hjkl-buffer/src/buffer.rs:731`). Each entry is
  `{ range: Pos..Pos, replacement: String }`, in engine `Pos`, which is
  char columns (§5). Coverage is partial by its own account: char inserts,
  deletes and replaces are exact, while line and block deletes, joins,
  splits and block inserts are a "best-effort placeholder covering the
  touched range" (`editor.rs:3800-3808`). With logging on, the probe got
  one entry for `d3w` and three for `ciwXY<Esc>` (§4).
- Side channels: `take_lsp_intent`, `take_fold_ops`, `take_errors`, and
  `Host::emit_intent` / `emit_cursor_shape`.

`hjkl_vim::EngineCmd` (`crates/hjkl-vim/src/cmd.rs:5`) looks like an action
type, but it is keyed by raw keys. For example,
`ApplyOpMotion { op, motion_key: char, total_count }`. It is produced only
by the app-side chord reducer `hjkl_vim::pending::step`
(`pending.rs:138`), not by `dispatch_input`.

**Who owns the state.** `Editor` owns everything (`editor.rs:775-1060`):

- the buffer (`B`, in practice `hjkl_buffer::View`)
- the host (`H: Host`), registers, search and change banks, global marks,
  jumplists, settings and style tables
- the vim discipline state, type-erased in
  `discipline: Box<dyn DisciplineState>` and downcast by `hjkl-vim`

Per-document state sits in `hjkl_buffer::Buffer` behind
`Arc<Mutex<…>>`: rope, undo tree, lowercase marks, folds, change log
(`crates/hjkl-buffer/src/content.rs:45-128`, `buffer.rs:61-66`). The host
owns clipboard, time, viewport, search prompt, cursor shape and intents
(`Host` trait, `types.rs:991-1072`).

**Is `View` sealed, and what would unsealing take?** Yes.
`pub trait View: Cursor + Query + BufferEdit + Search + sealed::Sealed + Send {}`
(`types.rs:1503`), with `Sealed` in a `pub(crate)` module (`types.rs:1323-1334`).
Unsealing alone gains nothing:

- `Editor::new`, `vim_editor`, `install` and `dispatch_input` exist only for
  `Editor<hjkl_buffer::View, H>` (`editor.rs:1668`,
  `crates/hjkl-vim/src/vim/mod.rs:92,99`, `crates/hjkl-vim/src/lib.rs:90`).
- `crates/hjkl-engine/src/buffer_impl.rs:10-24` explains that the FSM calls
  about 46 methods on the concrete buffer and that only 13 are on the trait.
  Making the editor generic "requires relocating those ~33 helpers … a
  separate, multi-thousand-LOC patch tracked for the 0.1.0 cut".

Undo, marks, folds and the change log also live inside the concrete
`Buffer`. A foreign buffer would have to re-implement them or hand them
back.

## 4. Vim model

| Concept | Representation |
|---|---|
| Operators | `hjkl_vim_types::Operator` (`crates/hjkl-vim-types/src/lib.rs:111`): Delete, Change, Yank, Upper/Lower/ToggleCase, Indent, Outdent, Fold, Reflow (2), AutoIndent, Filter, Comment. |
| Motions | `Motion` (`lib.rs:165`), 46 variants. Data-carrying ones include `Find { ch, forward, till }`, `SearchNext`, `WordAtCursor` and `ChangeWordEnd` for the `cw` rule. Range class is `RangeKind { Exclusive, Inclusive, Linewise }` (`lib.rs:295`). |
| Text objects | `TextObject` (`lib.rs:275`): `Word { big }`, `Quote(char)`, `Bracket(char)`, `Paragraph`, `XmlTag`, `Sentence`; `inner` is carried beside it. |
| Counts | An editor-side accumulator. In the probe, `d3w` keeps `pending = Op { op: Delete, count1: 1 }` after the `3`, and the `3` lands in the count, giving `count: 3` in `LastChange`. `pending.rs:33-44` documents `total = count1 × inner_count`. |
| Pending input | `hjkl_vim_types::Pending` (`lib.rs:20`): `Op`, `OpTextObj`, `OpG`, `G`, `Find`, `OpFind`, `Replace`, `Z`, `SetMark`, `GotoMark*`, `SelectRegister`, `RecordMacroTarget`, `PlayMacroTarget`, brackets, sneak. |
| Registers | `Registers` (`crates/hjkl-engine/src/registers.rs:59-80`): unnamed `"`, `"0`, `"1`–`"9` ring, `"-`, `"a`–`"z` (`A`–`Z` append), `"_` (black hole, `:93`), `"+`/`"*` as **one** `clip` slot, synced by the host through `Host::write_clipboard`/`read_clipboard` (`types.rs:996-1007`), and read-only `"%`. `Slot` holds linewise/blockwise flags. There is no `".`, `":` or `"/` in `read()` (`registers.rs:194-208`). |
| Modes | `hjkl_vim_types::Mode` (`lib.rs:6`): Normal, Insert, Visual, VisualLine, VisualBlock. Operator-pending is a `Pending` state rather than a mode. `hjkl_vim::Mode` (`crates/hjkl-vim/src/lib.rs:39`) adds `OpPending`/`CommandLine` for the keymap. The coarse `VimMode` and `CoarseMode` are for app chrome. |
| Dot-repeat | `LastChange` (`lib.rs:308`), with variants such as `OpMotion { op, motion, count, inserted, register }`, `OpTextObj`, `LineOp`, `CharDel`, `Paste`, `VisualOp`, `VisualBlockInsert` …, replayed by `replay_last_change` (`crates/hjkl-vim/src/vim/dot_repeat.rs:33`). |
| Macros | Keys recorded in `end_step` are encoded to vim notation and stored as register text (`crates/hjkl-vim/src/step.rs:51-64`). `@r` decodes the text and re-feeds it through `dispatch_input`. |
| Marks | Lowercase marks live per buffer in `Buffer.marks` (`content.rs:128`) and are restored by undo. Uppercase marks are global, `Editor.global_marks` (`editor.rs:907`). There is also a changelist bank and jumplists. |
| Search | `SearchBank` (last pattern, direction, history; `editor.rs:685`) and `SearchPrompt` (`crates/hjkl-engine/src/search.rs`). Patterns are translated from Vim's magic syntax to `regex` (`substitute.rs:14`). |
| Ex | `hjkl-ex`: `Registry<H>` of `ExCommand`s (`crates/hjkl-ex/src/registry.rs:59`), `try_dispatch` (`lib.rs:55`) → `ExEffect` (`effect.rs:7`). This includes `:s`, `:g`, ranges, `:normal` and `:set`, plus file, shell and app commands. |

**Traces.** Here is the probe, driving
`hjkl_vim::vim_editor(View::new(), DefaultHost::new(), Options::default())`
with `buffer_mut().set_change_log_enabled(true)` so that `take_changes()`
records. The buffer starts as
`"one two three four five\nsecond line\nthird foo line"` with the cursor at
(0, 0). Each line below is one `dispatch_input` call, followed by what the
host can observe. `count()` and `pending_register()` are shown where they
hold state that `pending()` doesn't.

```
== d3w
  dispatch_input('d') -> true  mode=Normal pending=Op { op: Delete, count1: 1 } cursor=(0,0)
  dispatch_input('3') -> true  mode=Normal pending=Op { op: Delete, count1: 1 } count()=3
  dispatch_input('w') -> true  mode=Normal pending=None                         cursor=(0,0)
  content()            = "four five\nsecond line\nthird foo line\n"
  take_changes()       = [EngineEdit { range: Pos { line: 0, col: 0 }..Pos { line: 0, col: 14 }, replacement: "" }]
  take_content_edits() = [ContentEdit { start_byte: 0, old_end_byte: 14, new_end_byte: 0,
                           start_position: (0,0), old_end_position: (0,14), new_end_position: (0,0) }]
  last_change()        = OpMotion { op: Delete, motion: WordFwd, count: 3, inserted: None, register: None }
  registers            = "\"" = "one two three ", "-" = "one two three ", "0"/"1" empty
  undo_stack_len()     = 1
== "ayy
  '"' -> pending=SelectRegister
  'a' -> pending=None, pending_register()=Some('a')
  'y' -> pending=Op { op: Yank, count1: 1 };  'y' -> pending=None
  take_changes() = [];  take_content_edits() = []
  last_change()        = LineOp { op: Yank, count: 1, inserted: None, register: Some('a') }   <- see below
  registers            = "\"" = "four five\n", "a" = "four five\n", "0" untouched (named register used)
  undo_stack_len()     = 1   (no new undo step)
== j w ciw X Y <Esc>
  'j' cursor=(1,0); 'w' cursor=(1,7)
  'c' -> pending=Op { op: Change, count1: 1 }
  'i' -> pending=OpTextObj { op: Change, count1: 1, inner: true }
  'w' -> mode=Insert            (word deleted)
  'X' -> cursor=(1,8); 'Y' -> cursor=(1,9);  <Esc> -> mode=Normal cursor=(1,8)
  content()            = "four five\nsecond XY\nthird foo line\n"
  take_changes()       = [EngineEdit { range: (1,7)..(1,11), replacement: "" },
                          EngineEdit { range: (1,7)..(1,7),  replacement: "X" },
                          EngineEdit { range: (1,8)..(1,8),  replacement: "Y" }]
  take_content_edits() = [delete bytes 17..21, insert 17..18, insert 18..19]   (3 edits)
  last_change()        = OpTextObj { op: Change, obj: Word { big: false }, inner: true,
                                     inserted: Some("XY"), register: None }
  registers            = "\"" = "line", "-" = "line"
  undo_stack_len()     = 2   (ciw plus the typed text is ONE undo step)
== j 0 w .
  '.' -> cursor=(2,7);  content() = "four five\nsecond XY\nthird XY line\n"
  take_content_edits() = [delete 26..29, insert "XY" 26..28]   (the replay inserts in one edit)
  last_change()        = unchanged (OpTextObj … inserted: Some("XY"))
  registers            = "\"" = "foo", "-" = "foo"
  undo_stack_len()     = 3
== u
  content() = "four five\nsecond XY\nthird foo line\n"; one inverse ContentEdit; undo_stack_len() = 2
== d3w "ayy .   (fresh editor, same buffer)
  hjkl:  content() = "four five\nsecond line\nthird foo line\n"   ('.' replayed the yank; nothing deleted)
         last_change() = LineOp { op: Yank, count: 1, inserted: None, register: Some('a') }
  nvim:  "line\nthird foo line"                                    ('.' repeated d3w)
```

The `d3w`, `ciw`, `.` and `u` results are correct vim results. For `d3w`, a
within-line delete goes to `"-`, not `"1`. For `"ayy`, `"0` is left alone
when a register is named.

**`"ayy` exposes a dot-repeat divergence.** In Vim a yank isn't a change,
so it must not replace the last change for `.` (`:h .`; Neovim's default
`cpoptions` `aABceFs_` has no `y`). hjkl stores the yank as `LastChange::LineOp`.
`d3w"ayy.` therefore replays the yank and deletes nothing, where nvim 0.12.4
deletes three more words. In the main trace above this is masked, because
`ciw` overwrites `last_change` before the `.`. Run as
`KEYS='d3w"ayy.' nvim --clean --headless -l f4.lua` against the same buffer.

Nothing above is reported as an invocation. The change log gives edit
payloads (a range and its replacement text). Recovering
`editor.operate { Delete, WordFwd, 3, None }` means reading `last_change()`
after the fact, and that isn't updated for motions, and is wrongly updated
for yanks.

## 5. Host buffer contract

**What the host must provide.**

- A `Host` impl (`types.rs:991`): clipboard read/write, `now()`, an optional
  `should_cancel`, `prompt_search`, display-line mapping for `gj`/`gk`,
  syntax highlights, `emit_cursor_shape`, `viewport()`/`viewport_mut()` and
  `emit_intent`.
- The buffer *is* `hjkl_buffer::View`: `ropey` **1.6.1** (we pin
  `ropey 2.0.0-beta.1`), wrapped as `Arc<Mutex<Buffer>>` plus a charwise
  cursor (`buffer.rs:61-66`).
- One primary cursor: `Position { row, col }` with col in chars
  (`crates/hjkl-buffer/src/position.rs:1-10`). The visual anchor lives in
  vim state, and secondary selections in `Editor.extra_selections`.
- `set_content` normalises to a trailing `\n` (the probe's `content()`
  gains one).

**Multiple cursors.** The engine tracks secondary selections through every
edit (`editor.rs:790-808`, `tests/multi_cursor.rs`, 32 tests). The vim
discipline doesn't use them: "vim drives one caret, so this costs an
`is_empty()` check per edit and nothing else". Nothing in `hjkl-vim`
references `extra_selections`.

**Undo.** Undo is snapshot-based. Each step stores a whole-rope snapshot,
an O(1) `Rope` clone, plus cursor and marks, in an undo tree inside
`hjkl_buffer::Buffer` (`crates/hjkl-buffer/src/undo.rs`;
`Editor::push_undo_at`, `editor.rs:4917`). Groups are a depth-counted RAII
guard, `Editor::undo_group()` (`editor.rs:4908`): the first `push_undo` in
the outermost group takes the snapshot. The probe shows an insert session,
including the `c` that opened it, collapsing to one step.

The *grouping* maps naturally onto `fe_buffer` transactions:
`fastedit/crates/fe-buffer/src/buffer.rs:382` `start_transaction(origin)`
and `:408` `end_transaction`, which also nest. The *storage* doesn't. hjkl
restores by swapping whole ropes and then diffing rows to synthesise one
inverse `ContentEdit`. That would bypass our edit log, `Origin`-scoped undo
(an AI-only undo, for instance) and anchor mapping. Undo would have to be
rerouted to `EditorModel::undo`, which is a fork.

**Index units.** There are three:

- engine `Pos { line, col }`, documented as grapheme columns
  (`engine_types.rs:12-20`)
- buffer `Position`, in char columns
- `ContentEdit`, in bytes

`buffer_impl.rs:36-46` admits that the grapheme and char columns "happen to
match … (graphemes without combining marks == chars)". The probe confirms
it's chars. On `"e\u{301}xy"`, `l` moves to column 1, the combining mark,
and `x` deletes just U+0301, leaving `"exy"`. Neovim treats `é` as one cell.
Our model is byte offsets moved by grapheme (`fastedit/crates/fe-text/src/lib.rs:7`,
`fastedit/crates/fe-editor/src/lib.rs:513-514`).

**Could it run over `EditorModel` without copying the buffer?** No. The FSM
only accepts `Editor<hjkl_buffer::View, _>`, `View` is sealed, and the
buffer type owns undo, marks and folds. The best an adopter could do is keep
a second rope per document, mirror our edits into it and replay its
`ContentEdit`s back as `fe_buffer` edits. That doubles memory, and the two
undo histories would disagree. A rope clone is cheap; the initial copy and
the line-index work on a multi-GB file are not.

## 6. Coverage against Neovim

**Tests.** I ran hjkl's own suites with
`cargo +1.98.1 test -p hjkl-engine -p hjkl-buffer -p hjkl-keymap -p hjkl-editor -p hjkl-ex -p hjkl-vim -p hjkl-vim-types`:
**1,851 passed, 0 failed, 1 ignored**.

| Crate | Tests |
|---|---|
| engine | 579 |
| ex | 562 |
| vim | 431 |
| buffer | 237 |
| keymap | 37 |
| editor | 5 |

`hjkl-vim`'s unit tests took 69 s; everything else took under 5 s.

**How the Neovim oracle runs.** `hjkl-compat-oracle` (workspace-only,
`publish = false`) holds **1,049 corpus cases** in 70 TOML files under
`crates/hjkl-compat-oracle/corpus/`. Each case is an initial buffer and
cursor, keys in vim notation, and an expected buffer, cursor, mode and
register. There are two paths:

- **In-process with live nvim.** `run_oracle` (`src/diff.rs:64`) runs
  `hjkl_driver::run_case`, which replays the keys through `dispatch_input`.
  It then runs `nvim_driver::run_case`, which spawns `nvim --headless --embed`
  through `nvim-rs` (LGPL-3.0, dev-only there; `src/nvim_driver.rs:58,106`).
  It checks nvim against the authored expectations and hjkl against nvim.
  Without nvim on `PATH`, it pins hjkl to the authored values instead.
- **Through the app binary.** Fifteen suites are ex, `:s`, magic regex,
  bare `&` repeat, search-pattern ranges, abbreviations, put,
  `nvim_api_tier`, the sneak fallback and round-3 ex. They spawn a
  separately built `hjkl --nvim-api` per case and compare it with the
  *authored* expectations, not a live nvim (`tests/oracle.rs:113-170`).
  Without the binary each one prints `skipping …` and the test still passes
  (`tests/oracle.rs:80-112`).

| | Files | Cases | This run |
|---|---|---|---|
| In-process, compared with live nvim 0.12.4 | 55 | 888 | **888 passed**, 0 failed |
| Binary-backed, authored expectations | 15 | 161 | **skipped**: no `hjkl` binary built |
| Total | 70 | 1,049 | |

What ran:

- `cargo +1.98.1 test -p hjkl-compat-oracle` passed 96 Rust tests, 75 of
  them corpus test functions, in 92.8 s. That count includes the 15
  binary-backed functions, which ran no cases. The build plus run took
  1 min 48 s.
- A rerun with `--nocapture` listed exactly those 15 `skipping` lines.
- The per-case counts come from a scratch example in the hjkl clone. It
  ran `run_oracle` over every in-process corpus file and tallied
  `CaseStatus` (`{"Pass": 888}`).
- The file split comes from matching each corpus path to the test that
  loads it in `tests/oracle.rs`.

**Known gaps.**

- `corpus/known_divergences.toml` is empty ("No known divergences remain").
- `docs/backlog.md` (3,521 lines) is the working list. §1.5 and §1.5b cover
  oracle divergences left open on purpose, mostly blockwise-visual text
  objects. §1.3 lists deferred items, including "Multicursor `lens` vector
  … gated behind unwired multicursor" and the Settings/Options collapse
  "staged for 0.1.0".
- `COVERAGE.md` marks scroll-only commands as "not oracle-comparable" and
  sneak as an intentional divergence.
- Seen here:
  - **Dot-repeat after a yank:** `d3w"ayy.` replays the yank in hjkl and
    deletes three more words in nvim (§4). No corpus case has a change,
    then a yank, then `.`. In the closest one, `Vyj.`
    (`tier2_round3_b1_visual_dot.toml:103`), there is no earlier change,
    so "replay the yank" and "do nothing" look the same.
  - hjkl has no `startofline` option and behaves as `startofline` on
    (Vim's default). Neovim defaults to off.
  - Columns are chars (§5).
  - `"+` and `"*` share one slot.
  - `".`, `":` and `"/` aren't readable registers.

**Spot check.** I checked the 20 commands asked for, plus five more. Each
was run through hjkl (the probe) and `nvim --clean --headless -l spot.lua`.
Both started from `"fn foo(a, b) {\n    let x = foo(a);\n    x + b\n}\nfoo end"`
with the cursor at (0, 4), and nvim was set to hjkl's defaults
(`ts=4 sw=4 sts=4 et ai si ic`). I compared buffer, cursor row and cursor
byte column. **All 25 are present.** All 25 buffers match, and 23 of 25
cursors match.

| # | Keys | Command | Same as nvim |
|---|---|---|---|
| 1 | `w` | word forward | yes |
| 2 | `wb` | word back | yes |
| 3 | `e` | word end | yes |
| 4 | `j^` | first non-blank | yes |
| 5 | `$` | line end | yes |
| 6 | `Ggg` | file bottom/top | cursor differs: hjkl (0,0), nvim (0,4); same with `startofline` |
| 7 | `fb` | find char | yes |
| 8 | `f(%` | match bracket | yes |
| 9 | `x` | delete char | yes |
| 10 | `dd` | delete line | yes |
| 11 | `yyjp` | yank line, put | yes |
| 12 | `J` | join lines | yes |
| 13 | `rZ` | replace char | yes |
| 14 | `~` | toggle case | yes |
| 15 | `>>` | indent | yes |
| 16 | `cwbar<Esc>` | change word | yes |
| 17 | `ci(z<Esc>` | change inner parens | yes |
| 18 | `oq<Esc>` | open line below | yes |
| 19 | `A;<Esc>` | append at end | yes |
| 20 | `ddu<C-r>` | undo, redo | cursor differs: hjkl (0,0), nvim (0,4); same with `startofline` |
| 21 | `x.` | dot repeat | yes |
| 22 | `/foo<CR>n` | search, next | yes |
| 23 | `*` | star search | yes |
| 24 | `D` | delete to end of line | yes |
| 25 | `2dw` | counted delete word | yes |

The nvim driver was a 12-line Lua script: set lines and cursor,
`nvim_feedkeys(…, "xt")`, then write lines and cursor to a file. That is
the harness design proposed for `fe-vim` in §1.

## 7. Maintenance

- **Releases.** `hjkl-engine` has 104 versions, the first on 2026-04-25.
  By month: April 25, May 50, June 5, July 13, August 7, September 4. The
  latest is 0.42.1 on 2026-09-21. That is **31 distinct `0.x` minor
  lines**, each a semver-breaking release pre-1.0.
- **Contributors.** `mxaddict` has 2,353 commits and `dependabot` 30. There
  are no other humans.
- **Issues and PRs.** 16 open issues and 250 closed. Only **1** issue was
  filed by anyone other than the maintainer. 57 PRs merged and 2 are open.
  The repo has 8 stars and 0 forks; it was created 2026-04-25.
- **MSRV.** `rust-version = "1.95"` on every crate, below our 1.98.1 pin.
  The crates and the oracle built and passed on 1.98.1. hjkl's own
  `rust-toolchain.toml` says `stable`; on this Mac that was 1.94.0, which
  fails the MSRV check.
- **Lockfile.** A fresh resolution of the five crates plus `hjkl-vim` is 81
  packages. Against our `Cargo.lock` that is **23 new entries**:
  - 17 new crates: 12 `hjkl-*`, plus `ahash`, `bstr`, `globset`, `ignore`
    and `libmimalloc-sys`, which compiles mimalloc's C source with `cc`.
  - 6 second versions: `ropey` 1.6.1 beside our 2.0.0-beta.1, `dirs` 7,
    `dirs-sys` 0.5, `libloading` 0.9, `cc` 1.6, `redox_users` 0.5.

  `hjkl-engine` pulls in a tree-sitter grammar registry and loader
  (`hjkl-lang` → `hjkl-bonsai`, with `libloading`) just to look up
  `commentstring` (`editor.rs:5273`).
- **"Pre-1.0 churn."** The docs say "the public surface may change in
  patch bumps" (`crates/hjkl-engine/src/lib.rs:13-14`). The
  generic-`Editor` trait extraction is still "planned" for 0.1.0
  (`buffer_impl.rs:10-24`, `crates/hjkl-editor/src/lib.rs:9-21`), and docs
  lag the code. `hjkl-engine/README.md` still says `ratatui` and `crossterm`
  are unconditional deps, but they aren't in the 0.42.1 manifest.

  The risk is high for adopting: a breaking change about every 5 days, from
  a single maintainer. For forking it doesn't matter, because a fork stops
  tracking upstream.

## 8. Fit with our command model

**Can hjkl's output map onto `editor.operate` / `editor.move` under
`vim.mode` predicates?** Its *vocabulary* maps well:

- `LastChange::OpMotion { op, motion, count, register }` and
  `OpTextObj { op, obj, inner, register }` are almost exactly
  `editor.operate { operator, target: Motion | TextObject, count, register }`.
- `Motion` plus a visual-mode flag is `editor.move { motion, extend, count }`.
- `RangeKind` is the inclusive/exclusive/linewise class our targets need.

Its *output* doesn't map. `dispatch_input` runs the operator itself
(`apply_op_with_motion` and friends in `crates/hjkl-vim/src/vim/`) and
reports only buffer deltas (§4). To dispatch through a `fe-command`
registry, the reducer would have to stop at the semantic tuple and return it,
and the operators would have to run as our commands over `EditorModel`.
That is a rewrite of `normal.rs` and `vim/*` (18k lines), so a fork. The
mode is readable (`vim_mode()`, `pending()`), so `vim.mode == normal`
predicates could be fed from it, but hjkl would still own the transitions.

**Can our keymap hand keys to it?** Mechanically, yes: translate a key event
to `hjkl_engine::Input` and call `dispatch_input`. But it swallows every key
in Normal, Visual and operator-pending modes, so our keymap must see a key
first and forward only what it doesn't bind.

An incomplete command lives inside hjkl in **three** separate places, and
`pending()` shows only one of them. The probe (§4) shows:

- after `d`: `pending() = Op { Delete, count1: 1 }`
- after `3`: `pending()` is unchanged, and `count() = 3`
- after `"a`: `pending() = None`, and `pending_register() = Some('a')`
  (`crates/hjkl-vim/src/editor_ext.rs:919`)

A host binding on the next key would therefore steal the `w` of `3w` or the
`p` of `"ap` unless the keymap checks all three. Worse, it would leave the
stale count or register to apply to some later command. Our keymap would
have to treat *any* of them being set as "vim owns the next key". Or it
would have to cancel all three itself, through hjkl's setters, whenever one
of its own bindings fires. Neither is a supported hjkl contract.

**Can `hjkl-keymap` coexist with ours, or would it replace ours in vim
contexts?** It isn't part of the engine. `hjkl-engine` and `hjkl-vim` don't
depend on it; only the app and `hjkl-which-key` do. It is a generic
per-mode chord trie: `Keymap<A, M>::feed(mode, KeyEvent) -> Pending |
Match(Binding<A>) | Ambiguous | Unbound(Vec<KeyEvent>)`, with leader,
timeout and a `condition` predicate per binding
(`crates/hjkl-keymap/src/keymap.rs:32-44,236`, `trie.rs:21-35`). That is
the same job as our keymap cards 4a/4b. They could coexist, but carrying
both makes no sense. Ours should do user bindings, timeouts and which-key in
every context. Vim's grammar should be a pending-input *handler* behind it,
not a second trie. `Unbound(keys)` replay is the one behaviour worth
copying as an idea: buffered keys go back to the handler when a prefix
turns out not to be bound.

## 9. Requirements vim puts on the command model

For card 1a or a follow-up. These hold whichever engine we choose.

1. **Pending-input routing.** The keymap resolves keys to invocations, but a
   mode handler can claim the next keys.
   - **One pending state.** The reducer keeps the whole incomplete command
     in one state value: count, `"{reg}`, operator, second count, `g`/`z`/
     `[` prefixes and `f`/`t` targets. `vim.pending` is true whenever any
     of these is set, including a bare count (`3`) and a completed register
     prefix (`"a`). This avoids hjkl's split across `pending()`, `count()`
     and `pending_register()` (§8).
   - **Routing.** While `vim.pending` is true, the handler owns every key.
     Raw keys go to `fe-vim`'s reducer:
     `Wait | Commit(invocation) | Cancel | Forward`. Keymap bindings don't
     fire.
   - **Forward.** On `Forward`, the key goes back to the keymap and the
     pending state is cleared. Vim discards a count or register that no
     command consumed, and so must we, so that it can't apply later.
   - **Explicit overrides.** A binding may be marked as allowed while
     pending, for example `<C-c>` or a global command palette. It fires
     only after the keymap tells the handler to cancel, so no prefix
     survives it. A binding never silently preserves a prefix.
   - **Unbound keys.** In vim normal mode, a key the keymap doesn't bind
     goes to the reducer. A binding prefix that turns out unbound replays
     its buffered keys to the handler.
2. **Mode as a context key.** `vim.mode` is one of `normal`, `insert`,
   `visual`, `visual_line`, `visual_block`, `operator_pending`, `replace`
   or `cmdline`. Also `vim.pending`, true while a command is incomplete
   (item 1), with `vim.pending_kind` (`operator`, `register`, `count`, `g`
   …) for which-key. All are available to `when` predicates such as
   `vim.mode == normal`.
   Entering and leaving a mode are commands, so they are recordable.
3. **Count and register as args.** `count: Option<u32>` keeps absent
   distinct from 1 (`G` vs `1G`, `p`). The reducer folds `2d3w` to 6 before
   committing. `register: Option<char>` covers `" 0-9 - a-z A-Z _ + *`.
   Registers are a service that non-vim commands such as copy and paste can
   also reach.
4. **Text objects as targets.**
   `target: Motion { kind, data } | TextObject { kind, inner } | Selection | Lines(n)`,
   each with a range class: exclusive, inclusive or linewise. Motion data
   includes the `f`/`t` char, the search pattern and the mark name. Targets
   resolve against `EditorModel` at execution, not when the key is pressed.
5. **Dot-repeat of the last mutating invocation.** Commands declare
   whether they are mutating and repeatable. The registry keeps the last
   repeatable invocation plus the text inserted by the insert session that
   followed it. `.` replays both, and a new count replaces the old one. A
   repeat of a numbered-register put increments the register, as vim does.
6. **Macros as invocation lists.** The recorder captures invocations for
   scripting and AI replay. For vim compatibility, `q{r}` must also store
   the *keys* as text in register `r`, so that `"rp`, editing and `@r` work.
   Replay feeds those keys through the same keymap and reducer path,
   deterministically: no timeouts, and `Host::now()` is injectable.
7. **Per-mode cursor shape and selection semantics.**
   - Normal mode: a block cursor that covers one grapheme, never past the
     last one.
   - Insert mode: a bar between graphemes. Replace and operator-pending
     modes: an underline.
   - Visual selections include the character under the cursor.
     `visual_line` extends to whole lines, and `visual_block` is a
     display-column rectangle, rendered as a `SelectionSet` (with `$`
     extending each row).
   - Leaving visual mode keeps `'<`/`'>`.
   - Vertical moves keep the goal column (`EditorModel.goals`) in display
     columns.
8. **Undo grouping of an insert session.** An insert session is one
   `fe_buffer` transaction (`Origin::User`), opened by the command that
   started it (`c`, `o`, `A` …) and closed by `<Esc>`. Cursor keys inside
   insert mode split it, as vim's `<C-g>u` does. `.` replays as one
   transaction. Undo restores the cursor to the change start, so
   transactions record the selection before them.
9. **Units.** Invocation args are byte offsets or anchors. Vim counts
   (`3l`, `5x`) step graphemes, and `|` and `curswant` use display columns.
   The reducer must never carry char indices.
10. **Marks and the jumplist as anchors.** These are `fe_buffer` anchors,
    so they survive AI edits and remote edits. Global marks carry a buffer
    id.
11. **Cancellation.** `<Esc>` or an invalid key in a pending state cancels
    with no side effects and no undo entry.
12. **Ex commands.** `:` lines parse to registry invocations with a
    `range` arg (`:'<,'>s/…/…/g`). `:normal {keys}` feeds keys through
    item 1.

## 10. Unanswered

- **The 161 binary-backed oracle cases.** These are in 15 suites:
  - ex: `:d`, `:g`, `:j`, line ops, `:u`, `:y`
  - `:s`, magic regex, bare `&` repeat, search-pattern ranges
  - abbreviations, put, `nvim_api_tier`, sneak fallback, round-3 ex

  They need a built `hjkl --nvim-api` app binary, which pulls in the whole
  TUI app. I didn't build it, so all 161 were skipped, not run. Even when
  run, they check against the corpus's authored expectations, not a live
  nvim (`tests/oracle.rs:113-170`). So hjkl's ex, substitute and
  abbreviation behaviour is unverified here.
- **Whether the other Vim-named helpers are ports or citations.**
  `op_motion.rs:149`, `operator.rs:78`, and the word helpers in
  `motions.rs` beyond their doc comments. I stopped reading per the clean
  room rule. It doesn't change the decision.
- **Performance of hjkl on large files.** Not measured: keystroke latency,
  memory per document, and the undo snapshot cost at 100 MB+. Not needed
  once adopt and fork are rejected.
- **`hjkl-ex` beyond its registry shape.** I didn't audit the 6.9k
  non-test lines command by command, or its 563 tests for Vim-derived code.
- **The lockfile count.** It comes from a fresh resolution beside our
  `Cargo.lock`, not from adding the dependency here (forbidden by the
  task). Unification with our lock could change it by a few entries.
- **How much of hjkl's corpus to import.** I didn't check case by case how
  many of the 1,049 test hjkl-only features (sneak, comment, reflow, folds,
  blame). That belongs to the first `fe-vim` card if it imports the corpus.
