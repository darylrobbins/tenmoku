---
task_uid: "01m519tmvqhacsg9ej4stp03wx"
task: "T103"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_title_is_painted_in_its_colour_active_or_not keeps its name and still passes on Linux, painting real glyphs"
    required: true
    manual: false
  - id: "C2"
    text: "On macOS the test creates no real gpui platform off the main thread (the real-glyph path is cfg-gated to Linux or runs from a harness = false main-thread test), and an assertion of the drawn title colour, active and inactive, Some(c) and None, still runs on macOS"
    required: true
    manual: false
  - id: "C3"
    text: "The dirty-dot assertions are kept"
    required: true
    manual: false
---

# Keep kiln-shell's tab_title_color test from creating the real Mac platform off the main thread, still checking the drawn title colour on macOS (Mac T715 macOS fix)

Filed on argon from the Mac lead's note zk (2026-10-15): Mac host stack 10 failed its macOS gate on batch 15. This fixes argon T87 (Mac T715) on macOS. Base: argon trunk 6a499d8 (= mac/main b39ea29 + batch 15). Title the host commit "... (Mac T715 macOS fix)".

## Problem

On macOS, `kiln-shell::tab_title_color a_title_is_painted_in_its_colour_active_or_not` panics at gpui's macOS
platform.rs:210: "Mac platform not created on main thread". T87 made the test paint real glyphs through
`gpui_platform::current_platform(true).text_system()` (tab_title_color.rs:208) with new kiln-shell dev-deps
(kiln-platform test-support, and gpui_platform, with wayland and x11 on Linux). On macOS that creates the real Mac platform
on a test thread, which gpui forbids.

## Change

Keep the test's name, and keep it checking the colour the title is drawn in, active and inactive, for Some(c) and the
default None, with the dirty-dot assertions. Don't create the real platform off the main thread on macOS. Either:
- gate the real-glyph painting path to Linux with `cfg(target_os = "linux")`, keeping an assertion of the drawn colour
  that runs on macOS too (for example through the test platform's text system or the render path's recorded runs); or
- run the real-glyph check from a `harness = false` main-thread test.
Move the gpui_platform dev-dep to Linux only if nothing else needs it on macOS. On Linux the test must still paint real
glyphs as now.

## Out of scope

kiln-shell's production code.
