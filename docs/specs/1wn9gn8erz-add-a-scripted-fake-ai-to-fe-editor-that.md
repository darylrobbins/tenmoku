---
task_uid: "01m4rh2bczb9xf7w1wn9gn8erz"
task: "T501"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test scripted_ai::parses_every_verb: every verb in the spec's table parses to its Step, with \\n \\t \\\\ unescaped, blank and # lines skipped; an unknown verb, a missing or non-numeric argument, and a bad row:col each give a ScriptError whose Display starts with the 1-based line number; DEMO_SCRIPT parses and uses every verb"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test scripted_ai::demo_session_rebases_anchors (the M8 exit test): playing DEMO_SCRIPT on DEMO_TEXT step by step, after every type step the stream's region (EditStream::range) still spans exactly the text pushed so far, the live suggestion's remaining text is unchanged by typing before its anchor, and a pending hunk's HunkView::range still spans its old text; the final text equals the expected literal; touched() covers exactly the streamed region's rows and the accepted hunk's rows, as EditorModel::decorations returns them"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test scripted_ai::undo_grouped_by_origin: after the session's stream with typing on both sides of it, repeated EditorModel::undo removes only the user's edits (typing, the accepted suggestion as one step, the accepted hunk) and leaves every streamed byte; one undo-ai then removes the whole stream as one group and redo-ai restores it, with the text checked after each"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test scripted_ai::bad_steps_change_nothing: push and finish with no stream, stream with one open, a push after the user edits inside the target first (Conflict), a push after undo-ai (Undone), accept with no live suggestion, accept-hunk out of range, on a stale hunk and on an already accepted one, and a row past the end each return StepError, leave the text and touched() unchanged, and never panic"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-editor and cargo clippy -p fe-editor --all-targets -- -D warnings pass; fe-editor gains no dependency; crates/fe-editor/src/lib.rs changes by the one pub mod line; nothing outside this card's scope changes, apart from this lane's own docs/specs file"
    required: true
    manual: false
---

# Add a scripted fake AI to fe-editor that plays ghost text, a streamed edit, a proposal and touched-row decorations from a script, with a headless test of the M8 exit criterion

## Problem

The plan's M8 row (`~/.claude/plans/glittery-foraging-graham.md` line 112) is "AI seams: `ProposedChange` diff
blocks, ghost text, decoration providers, streaming edits, demo host", with the exit criterion "An AI transaction
applied during simulated typing rebases its anchors correctly, and undo stays grouped by origin". The user approved
(2026-10-12) one demo host "that drives ALL the AI seams together in a real window against a scripted fake AI (no
network, no model), as the reference for how an embedding host uses fastedit's AI APIs", scripted so it is
reproducible.

The engines are on main, each tested alone: `ProposedChange` (`crates/fe-buffer/src/proposed.rs`, with
`accept_as`), `EditStream` (`crates/fe-buffer/src/stream.rs`, tested in `crates/fe-buffer/tests/ai_stream.rs`),
`GhostText` (`crates/fe-editor/src/ghost_text.rs`) and decoration providers (`crates/fe-editor/src/decorations.rs`,
`EditorModel::add_decoration_provider`). Nothing plays them together in one session, and nothing proves the exit
criterion across all of them at once. The single-seam examples `crates/fe-gpui/examples/stream_demo.rs` and T260's
`proposal_demo` each read ad hoc commands.

This card is the GPUI-free half: a scripted fake AI as a library module plus the headless end-to-end test. The next
card wires it into a window.

## Change

New module `crates/fe-editor/src/scripted_ai.rs`, declared `pub mod scripted_ai;` in `crates/fe-editor/src/lib.rs`
(that line only). GPUI-free; no new dependency. Its module docs are the script reference (the grammar below, one
example per verb).

### Script

`pub fn parse(script: &str) -> Result<Vec<Step>, ScriptError>`. One step per line; blank lines and lines starting
with `#` are skipped; `<text>` is the rest of the line after one space, with `\n`, `\t` and `\\` unescaped. Rows and
columns are 1-based, columns in chars. `ScriptError` carries the 1-based line number and a message (`Display`:
`line N: …`). `pub enum Step` has one variant per verb:

| Line | Does |
|---|---|
| `wait <ms>` | nothing headless; the host pauses that long |
| `caret <row>:<col>` | `EditorModel::set_cursor` there (clamped to the row) |
| `type <text>` | the user types: `EditorModel::insert` once per char (a `\n` through `newline`) |
| `suggest <text>` | a `GhostText` at the caret, `Origin::Ai { agent: "script", session: "suggest-<n>" }` |
| `accept` / `accept-word` / `accept-line` / `dismiss` | `GhostText::accept` / `accept_word` / `accept_line` on the model / drop the suggestion |
| `stream` | `EditStream::begin` on the primary selection, `Origin::Ai { agent: "script", session: "stream-<n>" }` |
| `push <text>` | `EditStream::push` of that chunk |
| `finish` | `EditStream::finish` |
| `propose <first>-<last> <text>` | `ProposedChange::from_text` over rows `first..=last` (their whole text, line breaks included) with `<text>`, `Origin::Ai { agent: "script", session: "propose-<n>" }` |
| `accept-hunk <n>` / `reject-hunk <n>` | the latest proposal's `n`th hunk (1-based, text order): `accept_as(…, Origin::User)` (T258's decision: accepting is the user's edit) / `reject` |
| `undo` / `redo` | `EditorModel::undo` / `redo` (the user's origin only) |
| `undo-ai` / `redo-ai` | `buffer.undo(|o| o.is_ai())` / `redo`, as `stream_demo` does |

Also `pub const DEMO_SCRIPT: &str` and `pub const DEMO_TEXT: &str`, `include_str!` of the new files
`crates/fe-editor/src/scripted_ai/demo.script` and `crates/fe-editor/src/scripted_ai/demo_text.txt`: a 20–40 row Rust
snippet and a script against it, commented step by step, that exercises every verb at least once and, in this
order: suggests text and types part of it before accepting the rest; starts a stream, `type`s before the stream's
region and after it between `push`es, then finishes; proposes a change to rows above the streamed region, `type`s
above it, then accepts one hunk and rejects another; ends with `undo` steps and an `undo-ai`/`redo-ai` pair. The
next card's window plays these two by default.

### Driver

`pub struct ScriptedAi` holds the live suggestion (`Option<GhostText>`), the open stream (`Option<EditStream>`), the
proposals as `Rc<RefCell<ProposedChange>>` (shared, so a view can show them, as T255's `set_proposals` takes them),
and a touched-rows `Rc<RefCell<DecorationSet>>`.

- `ScriptedAi::new(model: &mut EditorModel) -> Self` registers the touched-rows set with
  `EditorModel::add_decoration_provider`.
- `step(&mut self, model: &mut EditorModel, step: &Step) -> Result<Report, StepError>` runs one step. `Report`
  (`Display`, one line, for the host to print) says what happened: bytes pushed and the stream's region, the
  transaction id of an accept or undo, a proposal's hunk count, the pause of a `wait`. A step that can't run returns
  `StepError` (`Display`) and changes nothing: `push`/`finish` with no open stream, `stream` with one open,
  `StreamError::Conflict` or `Undone` (the stream is then dropped), an accept with no live suggestion, a hunk index
  out of range, a stale or already-resolved hunk (`ProposalError`), a row past the end. Never a panic.
- Touched rows: after each `push` the set covers the stream's region (`EditStream::range`, spliced in place with
  `DecorationSet::splice`); after an accepted hunk it also covers the hunk's landed range (`HunkView::range` of the
  accepted hunk). Each is one `Decoration::new(range, DecorationKind::RowBackground, Tint::Accent)`. A host that
  accepts a hunk itself (a button in the window) reports it with
  `note_accepted(&mut self, model: &EditorModel, hunk: HunkId)`, which marks it the same way.
- Accessors: `ghost_text() -> Option<&GhostText>`, `proposals() -> &[Rc<RefCell<ProposedChange>>]`,
  `touched() -> Rc<RefCell<DecorationSet>>`.

## Tests

New file `crates/fe-editor/tests/scripted_ai.rs` (public API only), running steps on an `EditorModel` built from
`DEMO_TEXT` or small literal texts, comparing exact buffer text, ranges and `EditorModel::decorations(..)` output.

## Out of scope

Any window, GPUI or fe-gpui change (next card); timers (`wait` is reported, not slept); changes to the engines.

## Implementation notes

- `ScriptedAi::stream() -> Option<&EditStream>` was added beside the spec's accessors, so a host (and the C2
  test) can read the open stream's region with `EditStream::range`.
- Touched rows follow undo and redo. Anchors can't follow text that is deleted and then restored: after
  `undo-ai`/`redo-ai` the stream's mark collapsed to an empty range on one row. `ScriptedAi` now subscribes to
  the buffer and records the transactions behind its marks (each stream's, each accepted hunk's). An undo of
  one of those transactions removes its mark, and a redo marks where it lands again, mapped to the current
  snapshot. This is checked at the start and end of every step, so undos the host makes itself are picked up
  at the next step. `note_accepted` takes the buffer's last edit as the accept, so a host calls it right
  after accepting.
- Typing groups by word (`TYPING_PAUSE`, new word), so the user's undo takes typed ` beta` back in two steps
  (`beta`, then the space). C3 checks each step's text.
- A `push` of an empty chunk is written `push ` with a trailing space, which editors tend to strip, so the
  tests don't rely on it. `Step::Push(String::new())` works from code.
- The parser also rejects an unknown escape (`\q`) or a trailing lone `\`, and an argument given to a verb
  that takes none, each with its line number.
