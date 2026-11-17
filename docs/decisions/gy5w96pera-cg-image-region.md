# The 32 MB CG Image region (T1907)

**Owner: `app_icon::set()`.** `NSApplication setApplicationIconImage:` makes
AppKit render the 1024 px icon into a 32 MB CoreGraphics bitmap.

## Evidence

Dev build (`target/debug/tenmoku`, `TENMOKU_NO_IPC=1`, a scratch state dir,
never the user's app), launched with `MallocStackLogging=1`, 12 s idle:
`vmmap -v` shows one `CG Image 1246a4000-1266a8000 [32.0M]` region.
`malloc_history <pid> 0x1246a4000` gives its allocating stack:

```text
main.rs:679 (App::run callback)
 -> tenmoku::app_icon::set  app_icon.rs:27
 -> -[NSApplication setApplicationIconImage:]
 -> -[NSApplication _updateIconImageFromOriginal]
 -> -[NSImage lockFocusWithRect:...] -> NSNewBitmapBackingStore
 -> CGBitmapAllocateData -> mmap
```

Not the drag images, Sparkle or colour emoji (leads 2-4): the stack is the
icon call, at launch.

## Fix

A real bundle has `Contents/Resources/Tenmoku.icns` and an `Info.plist` with
`CFBundleIconFile`, so `should_set_icon(exe)` (`app_icon.rs`) is false only
for `X.app/Contents/MacOS/exe` with both present; `set()` returns early. An
unrelated `.app` ancestor, a bundle without the plist key or the icns, and
unbundled runs (cargo run, an installed binary) still set the icon. Tested
in `app_icon_tests.rs` (complete bundle, missing icns/key/plist, unrelated
`.app` directory, non-`.app` layout).

## Measured (footprint -p, debug builds, empty file, 12 s idle)

Bundle: `Real.app` with the real `crates/fastedit/macos/Info.plist` (version
substituted), a `Tenmoku.icns` made with `iconutil` from the 1024 px PNG, and
the dev binary at `Contents/MacOS/tenmoku` (the release bundle from
`scripts/bundle-macos.sh` was not built: cold release build on the shared
disk). Same bundle, binary swapped.

| | CG Image | Footprint |
|---|---|---|
| Bundle, before (icon set unconditionally) | 32 MB, 1 region | 102 MB |
| Bundle, after | none | 71 MB |
| Unbundled `target/debug/tenmoku`, before and after | 32 MB, 1 region | 112 MB |
