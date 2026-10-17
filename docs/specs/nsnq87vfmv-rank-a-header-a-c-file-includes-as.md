---
task_uid: "01m546794rvhnsgxnsnq87vfmv"
task: "T128"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-nav test cpp_includes::cpp_includes_rank_their_headers_imported passes: in a project where src/geometry.hpp and src/zzz/shapes.hpp both define area(int, int) and src/main.cpp #includes \"geometry.hpp\", go-to-definition on area(2, 3) answers [src/geometry.hpp Imported, src/zzz/shapes.hpp Project] with unambiguous true, and the same project in C (main.c, .h headers) still answers [src/geometry.h Imported, src/zzz/shapes.h Project] unambiguous; it fails at the base on the C++ half"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-nav passes with no existing test edited (tests/definition.rs unchanged); cargo clippy -p fe-nav --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Rank a header a C++ file #includes as Imported in search go-to-definition, as a C file's is (Mac T741)

Filed on argon from Mac T741 (card ~/coordination/argon-cards/zu/T741.json rev 1; note zu). Mac task uid 01m545c3rd7rzwpyamkk1pvws7. Base: argon trunk eb9c1dd (= mac/main de87d2b synced); the files it touches are the same on Mac main 9328d2ca. Title the host commit "... (Mac T741)".

## Problem

Search-based go-to-definition (fe-nav `SearchNavigation::definitions`) ranks a candidate `Tier::Imported`
when its file's module name is on one of the request file's import lines. If the first candidate's tier
beats the second's, the answer is `unambiguous` and the editor jumps straight to it. T105 set up the
import-line table (`import_statement` in `crates/fe-nav/src/search.rs`, about line 998) with
"c `#include`". C++ came in later (T113) and never got an arm: the table matches `"c"` only
(search.rs:1025), so a C++ file's `#include` lines count as nothing.

Reproduced at Mac main 9328d2ca and at mac/main de87d2b5, where `crates/fe-nav/src/search.rs` is the
same. `src/geometry.hpp` and `src/zzz/shapes.hpp` both define `int area(int w, int h)`. `src/main.cpp`
holds `#include "geometry.hpp"` and calls `area(2, 3)`. Go-to-definition on that call answers
`[geometry.hpp Project, zzz/shapes.hpp Project]` with `unambiguous: false`, so the user gets a picker.
The same project in C (`main.c` including `geometry.h`) answers `[geometry.h Imported, zzz/shapes.h
Project]` with `unambiguous: true` and jumps.

## Change

In `crates/fe-nav/src/search.rs` `import_statement`, read C++ (`"cpp"`, the name fe-syntax gives `.cpp`,
`.cc`, `.hpp` and the other C++ extensions) the way C is read: change the arm to `"c" | "cpp"`, so a line
whose `#` directive is `include` is one import line (`Statement::LINE`). No other language changes, and
ranking and `module_name` stay as they are.

Add the regression test in a new integration file, `crates/fe-nav/tests/cpp_includes.rs`, with its own
helpers (copied from tests/definition.rs's `write` and `indexed`, and that file left alone). The test is
`cpp_includes_rank_their_headers_imported`. It builds the two-header project above in a temp dir, once
for C (`main.c` with `.h` headers, a control that already passes) and once for C++ (`main.cpp` with
`.hpp` headers). Each time it asserts the included header comes first as `Imported`, the other header
is `Project`, and the answer is unambiguous.
