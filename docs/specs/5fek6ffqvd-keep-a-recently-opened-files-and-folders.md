---
task_uid: "01m4rqek6zqzjb0k5fek6ffqvd"
task: "T56"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "recent: record_file and record_folder move an entry to the front without duplicates and trim to 50 files and 20 folders"
    required: true
    manual: false
  - id: "C2"
    text: "recent: load of a missing file is empty; of an invalid file is empty with the error returned; save then load round-trips"
    required: true
    manual: false
  - id: "C3"
    text: "recent: two threads each calling record 50 times on one file end with all 100 entries recorded (subject to the cap), none lost"
    required: true
    manual: false
  - id: "C4"
    text: "recent: existing() leaves out paths that are gone, and remove and clear persist"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-config and cargo clippy -p fe-config --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Keep a recently opened files and folders list in fe-config: most recent first, capped, deduped, written atomically and safely from several windows (Mac T512)

Filed on helium from Mac T512 (~/coordination/helium-cards/T512.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 2, Open Recent): helium is Linux. Where the card says Finder, read the file manager; test drops through GPUI's external-paths drop events in a gpui test, not a real drag. Paths, config locations and the recent list follow fe-config's existing per-platform rules (XDG on Linux).

## Problem

fastedit keeps no list of recently opened files or folders, so there's no Open Recent (day-one audit 2026-10-12;
user: "card the day-one ones"). This card is the persisted list; the next shows it.

## Change

New module `crates/fe-config/src/recent.rs`, exported from `crates/fe-config/src/lib.rs`; no GPUI.

- `RecentList { folders: Vec<PathBuf>, files: Vec<PathBuf> }`, most recent first, at most 20 folders and 50 files.
- `recent_path() -> Option<PathBuf>`: `<state_dir>/recent.toml` (`paths::state_dir`).
- `RecentList::load(path)`: a missing file is an empty list; an unreadable or invalid one is an empty list with
  the error returned beside it for the caller to log (never a panic).
- `record_folder(path)`, `record_file(path)`: move to the front (deduped by path), trimmed to the cap.
  `remove(path)`, `clear()`.
- `record(path_of_list, entry) -> io::Result<RecentList>`: read, record, write back through a temp file and
  rename, under an advisory lock on `<path>.lock`, so two windows or two processes recording at once both land.
- `existing(&self)`: the entries whose paths still exist, for display; stored entries are dropped only by the cap,
  `remove` or `clear` (VS Code keeps them too).

## Tests

New file `crates/fe-config/tests/recent.rs` (temp dirs).
