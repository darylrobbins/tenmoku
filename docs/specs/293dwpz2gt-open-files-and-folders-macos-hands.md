---
task_uid: "01m6c4d01y6zmwy5293dwpz2gt"
task: "T1272"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "An open-documents event with a folder path opens a window on that folder, reusing a live window already on it"
    required: true
    manual: false
  - id: "C2"
    text: "An open-documents event with a file opens it in the window whose root contains it, else a new window on its parent folder"
    required: true
    manual: false
  - id: "C3"
    text: "A cold launch by open-documents event shows the opened path, with no extra empty window"
    required: true
    manual: false
  - id: "C4"
    text: "Info.plist declares CFBundleDocumentTypes for folders and text/source/data files with LSHandlerRank Alternate, checked by scripts/test-bundle-macos.sh"
    required: true
    manual: false
  - id: "C5"
    text: "Launching with a path in argv behaves as before"
    required: true
    manual: false
---

# Open files and folders macOS hands Tenmoku.app (Finder Open With, open -a Tenmoku <path>, drop on the Dock icon), not only argv

Tenmoku.app takes its folder or file only from argv (`main.rs` reads `std::env::args_os()` into
`fe_workbench::resolve_launch`). macOS delivers files opened from Finder, `open -a Tenmoku <path>`,
"Open With" and drag-onto-Dock-icon as open-document Apple Events, which the app ignores today: it
starts (or comes forward) and drops the path. A release app has to handle them.

Do:
- Register the app's handler for opened URLs/paths (GPUI's `on_open_urls` on the application, or
  the kiln wrapper around it; add a kiln seam only if kiln hides it). Each path is resolved like the
  argv path: a folder opens a window on it (`open_window_on`, reusing a live window already on that
  root), a file opens in the window whose root contains it, else in a new window on its parent.
- Paths that arrive before the first window opens (cold launch via `open -a Tenmoku file`) open
  instead of the empty launch window, not in addition to it.
- `crates/fastedit/macos/Info.plist`: declare `CFBundleDocumentTypes` (public.folder, public.text,
  public.source-code, public.data; role Editor, rank Alternate so Tenmoku never steals defaults)
  so Finder offers Tenmoku in Open With and accepts drops on the Dock icon.
- argv launches behave as today.

Not in scope: a `tenmoku` CLI shim that talks to a running instance; URL schemes.
Tests: the path-to-window routing as a headless test in a new test file; Info.plist keys checked
by extending scripts/test-bundle-macos.sh. A person checks Finder/Dock behaviour later.
