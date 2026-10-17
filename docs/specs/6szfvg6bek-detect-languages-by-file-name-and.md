---
task_uid: "01m53hnm9pfphpnp6szfvg6bek"
task: "T121"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test language_file_names::login_shell_ruby_build_and_upper_case_names_are_detected passes: .zprofile .zlogin .zlogout .bash_login .bash_logout .bash_aliases .envrc PKGBUILD APKBUILD and x.ksh are bash; x.gemspec x.rake config.ru x.podspec Podfile Vagrantfile Brewfile Guardfile Capfile Fastfile are ruby; makefile and x.Mak are make; README.MD SCRIPT.SH Main.RS GEMFILE give markdown, bash, rust, ruby; notes.txt, zprofile.bak and .zprofilex give None; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax and -p fe-nav pass with no existing test edited; cargo clippy -p fe-syntax --all-targets -- -D warnings is clean; no grammar, query, feature or Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Detect languages by file name and extension in any ASCII case, and from zsh and bash login files, Ruby build files and .mak, as VS Code does (Mac T724)

## Problem

`fe_syntax::language_name_for_path` (`crates/fe-syntax/src/language.rs`, the `listed().find(..)` on
`def.file_names.contains(&file_name)` and `def.extensions.contains(&ext)`) compares the file name and
extension byte for byte against short tables, so at 32c938f9:

- **Case:** `Makefile` is make but GNU make's own `makefile` is nothing; `README.MD`, `SCRIPT.SH`,
  `Main.RS`, `photo.JSON` and `GEMFILE` are nothing. VS Code lower-cases both sides
  (`languagesAssociations.ts`: `filenameLowercase`, `extensionLowercase`), and macOS's default
  filesystem is case-insensitive, so upper-case names are common there.
- **Shell startup files:** bash has `.bashrc .bash_profile .zshrc .zshenv .profile`, but not
  `.zprofile`, `.zlogin`, `.zlogout`, `.bash_login`, `.bash_logout` or `.bash_aliases` (every zsh login
  shell reads `~/.zprofile` and `~/.zlogin`), nor `.envrc` (direnv), `PKGBUILD` or `APKBUILD`, which VS
  Code all opens as shell script; `.ksh` isn't an extension.
- **Ruby build files:** ruby has `rb`, `Gemfile` and `Rakefile` only; VS Code also knows `.gemspec`,
  `.rake`, `.ru` (config.ru), `.podspec`, and `Podfile`, `Vagrantfile`, `Brewfile`, `Guardfile`,
  `Capfile` and `Fastfile`.
- **Make:** `.mak` isn't an extension.

Each opens as plain text: no highlighting, folds, outline or comment toggling.

## Change

- `crates/fe-syntax/src/language.rs` `language_name_for_path`: compare the file name and the extension
  to the tables ignoring ASCII case (`eq_ignore_ascii_case`); the tables stay as written. Same order of
  checks as today, so the first listed language still wins.
- Add to the tables (and nothing else): bash `file_names` += `.zprofile`, `.zlogin`, `.zlogout`,
  `.bash_login`, `.bash_logout`, `.bash_aliases`, `.envrc`, `PKGBUILD`, `APKBUILD`; bash `extensions` +=
  `ksh`; ruby `extensions` += `gemspec`, `rake`, `ru`, `podspec`; ruby `file_names` += `Podfile`,
  `Vagrantfile`, `Brewfile`, `Guardfile`, `Capfile`, `Fastfile`; make `extensions` += `mak`.
- New test file `crates/fe-syntax/tests/language_file_names.rs` (gated like `shebang_languages.rs` on the
  features it uses), one test `login_shell_ruby_build_and_upper_case_names_are_detected` asserting
  `language_name_for_path` for every name added above (each in a directory, e.g. `home/.zprofile`),
  `makefile`, `README.MD`, `SCRIPT.SH`, `Main.RS`, `GEMFILE` and `x.Mak`; and that `notes.txt`,
  `zprofile.bak` and `.zprofilex` stay `None`. It fails at the base.

`detect_language_name` (`crates/fe-syntax/src/detect.rs`) already tries the path first, so it gains
these for free; don't change it. No grammar, query, feature or Cargo change.
