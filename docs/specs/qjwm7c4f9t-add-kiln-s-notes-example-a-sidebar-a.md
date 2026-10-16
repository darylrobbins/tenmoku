---
task_uid: "01m50ewhaw3vwac5qjwm7c4f9t"
task: "T97"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln/crates/kiln/examples/notes has the four files and no TOML, at most 110 lines without blank and comment-only lines, and main.rs is kiln::app(dev.kiln.Notes, Notes).run()"
    required: true
    manual: false
  - id: "C2"
    text: "kiln tests/notes.rs: secondary-n, File \u003e New Note and the palette each add a note and open its tab; the status reads 1 note, then 2 notes; secondary-b toggles the sidebar; clicking a sidebar row opens or activates that note"
    required: true
    manual: false
  - id: "C3"
    text: "notes.rs: changing [notes] font_size in settings.toml reaches the open note's view, and notes and layout restore on a second start with the same paths"
    required: true
    manual: false
---

# Add kiln's notes example: a sidebar, a keyed menu command, a status item, a setting and persistence in about 100 lines and no TOML, tested headless (Mac T687)

Filed on argon from Mac T687 (card ~/coordination/argon-cards/T687.json rev 1; note zh). Mac task uid 01m4vbjfqac88bnye72rcgcs5v. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Argon deps: T96 (Mac T686). Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T687)".

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 2's exit; design record
`docs/decisions/kiln-app-layer.md`). The approval was argued on one small app, a notes app with a sidebar, a
command with key and menu, a status item, one setting and persistence: about 800 Rust and 65 TOML lines in 10
files on today's Kiln, and about 103 Rust lines in 4 files with no TOML on the new layer. Phase 2 ends when that
app exists and is tested.

## Change

Add `kiln/crates/kiln/examples/notes/` as the design sketched it:

- `main.rs`: `kiln::app("dev.kiln.Notes", "Notes").run()`.
- `notes.rs`: `#[kiln::model(persist)] Notes { list: Vec<Note>, next_id }` with `add`, `get` and `set_body`;
  `#[kiln::settings(section = "notes")] NotesSettings { font_size: f32 = 14.0 }`; `/// New Note`
  `#[kiln::command(key = "secondary-n", menu = "File")] fn new_note(ws, cx)` adding a note and opening it; and
  `#[kiln::status(side = Left)] fn note_count(cx) -> String` ("1 note", "3 notes").
- `sidebar.rs`: `#[kiln::panel(title = "Notes", icon = .., dock = Left, toggle_key = "secondary-b")] Sidebar`
  listing the notes with `cx.watch::<Notes>()`, the shown note selected, a click opening it.
- `note_view.rs`: `#[kiln::item] NoteView { id, body: Entity<TextArea> }` with `type State = u64`, its body
  editing the model through a change callback (no stored `Subscription`), its font size from
  `cx.setting::<NotesSettings>()`.

Use kiln-ui's text widget as it is (`TextField` or an existing multi-line form); if none fits, a single-line body
is acceptable. New tests go in `kiln/crates/kiln/tests/notes.rs`, through `kiln::testing`, including the example
the way `tests/host.rs` includes the host's files.
