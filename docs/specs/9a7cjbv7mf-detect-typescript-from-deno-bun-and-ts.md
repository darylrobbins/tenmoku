---
task_uid: "01m53yvdce3pp6gh9a7cjbv7mf"
task: "T124"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test shebang_runtimes::typescript_runtimes_and_posix_shells_pick_their_language passes: deno (also via env -S with arguments), bun and ts-node shebangs give typescript; ash, mksh, yash, pdksh and dtksh give bash; bunx, deno-x, flash, tcsh and fish give None; detect_language_name on an extensionless path with a deno shebang gives typescript; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax passes with no existing test edited; cargo clippy -p fe-syntax --all-targets -- -D warnings is clean; no grammar, query, feature or Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Detect TypeScript from deno, bun and ts-node shebangs and bash from ash, mksh, yash, pdksh and dtksh ones, as VS Code does (Mac T733)

## Problem

`fe_syntax::language_name_for_shebang` (`crates/fe-syntax/src/detect.rs`, the `INTERPRETERS` table)
knows python, sh, bash, zsh, dash, ksh, node, nodejs, ruby, lua, php and make. At c5e93ed6 an
extensionless script whose `#!` line runs a TypeScript runtime or one of the other POSIX shells opens as
plain text (no highlighting, folds, outline or comment toggling):

- `#!/usr/bin/env -S deno run --allow-read`, `#!/usr/bin/env bun` and `#!/usr/bin/env ts-node` give
  `None`. VS Code opens all three as TypeScript (typescript-basics `firstLine`
  `^#!.*\b(deno|bun|ts-node)\b`).
- `#!/bin/ash` (busybox, Alpine), `#!/bin/mksh`, `#!/usr/bin/yash`, `#!/bin/pdksh` and `#!/bin/dtksh`
  give `None`. VS Code's shellscript `firstLine` lists ash, mksh, yash, pdksh and dtksh beside bash, zsh,
  sh, ksh and dash, and their syntax is the POSIX sh the bash grammar parses.

`env -S` splitting and version suffixes already work (`env_command`, `is_version_suffix`), so only the
table is missing names.

## Change

- `crates/fe-syntax/src/detect.rs` `INTERPRETERS`: add `("deno", "typescript")`, `("bun", "typescript")`,
  `("ts-node", "typescript")`, and `("ash", "bash")`, `("mksh", "bash")`, `("yash", "bash")`,
  `("pdksh", "bash")`, `("dtksh", "bash")`. Nothing else changes: matching stays an exact base name plus
  an optional dotted version suffix, so `bunx`, `deno-x`, `flash` and `bash5x` stay undetected. Don't add
  csh, tcsh or fish (not sh syntax, so the bash grammar would misparse them).
- New test file `crates/fe-syntax/tests/shebang_runtimes.rs`, gated like `shebang_languages.rs` on
  `feature = "typescript"` and `feature = "bash"`, one test
  `typescript_runtimes_and_posix_shells_pick_their_language` asserting `language_name_for_shebang` gives
  `typescript` for `#!/usr/bin/env deno`, `#!/usr/bin/env -S deno run --allow-read`, `#!/usr/bin/env bun`,
  `#!/usr/local/bin/bun` and `#!/usr/bin/env ts-node`; `bash` for `#!/bin/ash`, `#!/bin/mksh`,
  `#!/usr/bin/yash`, `#!/bin/pdksh` and `#!/bin/dtksh`; `None` for `#!/usr/bin/env bunx`,
  `#!/usr/bin/env deno-x`, `#!/bin/flash`, `#!/bin/tcsh` and `#!/usr/bin/env fish`; and that
  `detect_language_name(Some(Path::new("bin/tool")), "#!/usr/bin/env deno\n")` is `typescript`. It fails
  at the base.

No grammar, query, feature or Cargo change; language.rs is not touched.
