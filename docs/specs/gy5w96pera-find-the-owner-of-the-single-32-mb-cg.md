---
task_uid: "01m7j3fvktzezbx9gy5w96pera"
task: "T1907"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003clane\u003e-cg-image-region.md and the hand-in name the owner of the CG Image region with evidence (the allocating stack from malloc_history or Instruments, or a bisect that removes it), measured on a launched dev or bench build (and the bundle if it differs), never the user's running app"
    required: true
    manual: false
  - id: "C2"
    text: "If avoidable, it is freed or downsized: the footprint CG Image line before and after (bundled and unbundled) is in PERF.md ## Fixed memory, and a unit test of the decision (e.g. crates/fastedit/src/app_icon_tests.rs for skipping the icon in a bundle) passes; if unavoidable, PERF.md ## Fixed memory gains a row for it with that evidence"
    required: true
    manual: false
---

# Find the owner of the single 32 MB CG Image region in a running Tenmoku (the Dock icon NSImage set even in a bundle first), then free or downsize it, or document it in PERF.md Fixed memory

# What is the 32 MB "CG Image" region? Name its owner, then free or document it

## Why

The user asked: "Is there a way to track more finely where our memory is being
spent at least by module?" and said "Yes please" to this card.

`footprint -p` on the user's running Tenmoku (335 MB, the bundled app) showed
CG Image 32 MB in ONE region. None of Tenmoku's own drawing goes through
CoreGraphics images: GPUI rasterises glyphs into Rust `Vec`s
(gpui-pre-apple `text_system.rs` ~L465, a bitmap context over our bytes),
and `RenderImage`s (image viewer, kiln file icons, markdown images) are
decoded by the `image` crate into malloc and uploaded to Metal atlases. So a
CG Image region is most likely AppKit / ImageIO decoding something on our
behalf. 32 MB is e.g. a 2048×4096 or 4096×2048 RGBA bitmap, or 8 × a
1024×1024 RGBA one.

## Leads, most likely first

1. **The Dock icon, set twice in a bundle.** `crates/fastedit/src/app_icon.rs`
   `set()` decodes the 1024×1024 RGBA PNG (`assets/icon/fastedit-1024.png`)
   into an `NSImage` and calls `setApplicationIconImage`; it is called
   unconditionally from `crates/fastedit/src/main.rs` L995. Its comment says
   it is for unbundled runs, yet the bundle already has
   `Contents/Resources/Tenmoku.icns` (made by `scripts/bundle-macos.sh`
   L263 from the same PNG). AppKit may keep the decoded 4 MB bitmap plus
   renditions it draws for the Dock, Cmd-Tab and alerts.
2. **Drag images**: gpui-pre-macos `window.rs` ~L2316 builds
   `NSWorkspace.iconForFileType` images for an external drag from the project
   tree; a file icon is a multi-resolution image that decodes up to 1024 px.
   Check whether the user's session dragged a file out.
3. **Sparkle** (`crates/fastedit/src/sparkle.rs`): the updater framework loads
   the app icon for its windows; check whether its startup touches it.
4. **Colour emoji**: CoreText decodes Apple Color Emoji `sbix` PNGs through
   ImageIO when an emoji is shaped/rasterised (status bar, chat, a file with
   emoji). Large strikes are 160 px, so 32 MB in one region is less likely.
5. Setup wizard preview (T1342) and window proxy icons: no `NSImage` or
   represented-URL use found in `crates/` or `kiln/` (rg on main), so only if
   1–4 fail.

## How to find it

With a launched dev or bench build (`cargo run -p fastedit` or
`target/bench/tenmoku`, plus the bundle via `scripts/bundle-macos.sh` if the
region only appears bundled), never the user's own running app:

- `footprint -p <pid> -v` (or `vmmap -v <pid> | rg -i 'CG image'`) for the
  region's address, size and when it appears (check at launch, after the first
  frame, after opening a file, after a drag);
- `MallocStackLogging=1` launch, then `malloc_history <pid> <address>` on the
  region's start address for the allocating stack (CG Image regions are VM
  allocations, which full stack logging records), or Instruments' Allocations
  / VM Tracker (`xctrace record --template Allocations --launch -- …`);
- bisect by disabling a candidate (e.g. skip `app_icon::set()` with an env
  var in a local experiment) and comparing the CG Image line.

## Fix or document

- If it is avoidable (e.g. lead 1: don't set the icon when running from a
  `.app` bundle whose Info.plist names an icon, or set a smaller rendition),
  free or downsize it, with a unit test of the decision (e.g. a pure
  `fn should_set_icon(exe: &Path) -> bool` tested in
  `crates/fastedit/src/app_icon_tests.rs`) and the CG Image line before and
  after (footprint, bundled and unbundled) in PERF.md `## Fixed memory`.
- If it is AppKit's own and unavoidable, add a row for it to PERF.md
  `## Fixed memory`'s table with the evidence (stack or bisect result).
- Either way the hand-in names the owner, with the stack or the bisect as
  evidence, in `docs/decisions/<lane>-cg-image-region.md`.
