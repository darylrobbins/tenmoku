---
task_uid: "01m4tjjncm2fnm6ywjarv424yk"
task: "T115"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test shebang_languages::ruby_lua_php_and_make_shebangs_pick_their_language: env ruby, ruby3.2 -w, env lua, env lua5.4, /usr/bin/php, env php8.2 and /usr/bin/make -f shebangs give ruby, lua, php and make"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test shebang_languages::an_extensionless_ruby_script_is_detected_from_its_shebang: detect_language_name for bin/rails with an env ruby shebang is Some(ruby)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test shebang_languages::near_miss_interpreters_stay_undetected passes (rubyx, lua-5, makepkg give None; env python3 is python), and the in-crate detect tests pass unchanged"
    required: true
    manual: false
---

# Detect Ruby, Lua, PHP and Make scripts from their shebang in fe-syntax, so an extensionless bin/rails or debian/rules gets its language (Mac T654)

## Problem

`language_name_for_shebang` (crates/fe-syntax/src/detect.rs) knows only the interpreters in `INTERPRETERS`:
python, sh, bash, zsh, dash, ksh, node and nodejs. Ruby, Lua, PHP and Make are default features of fe-syntax
and are in `language_names()`, but none has an interpreter entry, so a script that names one of them in its
`#!` line is never detected. fe-gpui's view calls `detect_language_name` for a path with no known extension,
so extensionless Ruby scripts (`bin/rails`, `bin/rake`, `bin/setup` in every Rails app), Lua and PHP CLI
scripts and Debian's `debian/rules` (`#!/usr/bin/make -f`) open as plain text. The detection (T25) and
these languages (T19) landed the same day and were never connected.

Checked 2026-10-13 at Mac e6988aba (detect.rs is identical on helium main, blob 5f2f16de):
`language_name_for_shebang("#!/usr/bin/env ruby\n")`, `("#!/usr/bin/env lua5.4\n")`, `("#!/usr/bin/php\n")`
and `("#!/usr/bin/make -f\n")` are all `None`, and
`detect_language_name(Some(Path::new("bin/rails")), "#!/usr/bin/env ruby\n...")` is `None`.

## Change

Add `("ruby", "ruby")`, `("lua", "lua")`, `("php", "php")` and `("make", "make")` to `INTERPRETERS` in
crates/fe-syntax/src/detect.rs. The existing version-suffix rule then covers `ruby3.2`, `lua5.4` and `php8.2`,
and the final `language_names()` filter still answers `None` for a language whose feature is off. No other
behaviour changes: a name that merely starts with one of these (`rubyx`, `lua-5`, `makepkg`) stays undetected,
and the in-crate shebang tests pass unchanged. Path-based detection (language.rs `file_names`) is out of scope.

## Tests

New file crates/fe-syntax/tests/shebang_languages.rs, gated on the ruby, lua, php, make and python features
(all default):

- `ruby_lua_php_and_make_shebangs_pick_their_language` (regression, fails at base): `#!/usr/bin/env ruby`,
  `#!/usr/bin/ruby3.2 -w` are ruby; `#!/usr/bin/env lua`, `#!/usr/bin/env lua5.4` are lua; `#!/usr/bin/php`,
  `#!/usr/bin/env php8.2` are php; `#!/usr/bin/make -f` is make.
- `an_extensionless_ruby_script_is_detected_from_its_shebang` (regression, fails at base):
  `detect_language_name(Some(Path::new("bin/rails")), "#!/usr/bin/env ruby\n...")` is `Some("ruby")`.
- `near_miss_interpreters_stay_undetected` (control, passes at base): `rubyx`, `lua-5` and `makepkg` give
  `None`; `#!/usr/bin/env python3` is still python.
