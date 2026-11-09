---
task_uid: "01m6z3t3fswf5tan5x0ts5adc0"
task: "T1632"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe_syntax::language_name_for_path picks the language claiming a dotted extension (blade.php) for x.blade.php, ignoring ASCII case, over any built-in or registered language claiming the plain last extension (php), the longest compound match winning, and doesn't match a file named just blade.php; tests in crates/fe-syntax/tests/runtime_grammar.rs show each"
    required: true
    manual: false
  - id: "C2"
    text: "the grammar install offer (fe-workbench grammar_prompt's offer_for) offers the compound claimant for x.blade.php, with blade.php as its extension, and a test in grammar_prompt_tests.rs shows it"
    required: true
    manual: false
  - id: "C3"
    text: "wasm_parity's problems() treats a dotted claim as a whole: a directory claiming blade.php doesn't clash with a built-in or directory claiming php, shown by a fixture case; register_language's built-in clash check is unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "the doc comments on ExtensionGrammar's extensions (crates/fastedit/src/config.rs) and GrammarSpec's (crates/fe-syntax/src/runtime.rs), and extensions/grammars/README.md, say a compound extension is allowed and wins over the plain one"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-syntax -p fe-workbench -p fastedit passes"
    required: true
    manual: false
---

# Pick a language by a compound extension such as blade.php (over the plain php) in fe-syntax path detection, the grammar install offer and wasm_parity's clash checks

# Pick a language by a compound extension such as blade.php

Prerequisite for the Blade card. Laravel Blade files are `name.blade.php`.
Every path lookup takes only `Path::extension()` (`php`), so a manifest
claim of `blade.php` can never match, and the file goes to PHP:

- `crates/fe-syntax/src/language.rs` `language_name_for_path` (built-in
  `DEFS` by extension) and `crates/fe-syntax/src/runtime.rs`
  `name_for_file` (registered languages by extension);
- `crates/fe-workbench/src/grammar_prompt.rs` `offer_for` (which
  first-party grammar to offer for a file: it would offer PHP);
- `crates/fe-syntax/tests/wasm_parity.rs` `problems()` (its built-in clash
  check builds `sample.<extension>` and asks `language_name_for_path`,
  which would call `blade.php` the built-in php's when php is compiled in).

## Do

- An extension containing a dot (`blade.php`; no leading dot, at least one
  character either side) matches a file whose name ends, ignoring ASCII
  case, with `.` + it and is longer than it (`x.blade.php`, not
  `blade.php`). Plain extensions keep matching the last extension only.
- Precedence: a compound match beats a plain-extension match, whoever owns
  either (a registered Blade beats the built-in or registered PHP for
  `x.blade.php`); among compound matches the longest wins. Path tails and
  whole file names keep their current precedence over both.
- `register_language`'s built-in clash check (`language.rs`
  `built_in_clash`) keeps comparing claims as whole strings: `blade.php`
  doesn't clash with `php`.
- `offer_for` uses the same matching (the offer's extension, and its
  Never-for-.ext key, is `blade.php`).
- `wasm_parity.rs`: a dotted claim is checked against built-ins as a whole
  claim (a built-in claiming `php` is no clash for `blade.php`), and the
  cross-directory check treats `blade.php` and `php` as different claims.
- Docs: `ExtensionGrammar.extensions`' doc comment in
  `crates/fastedit/src/config.rs` and `GrammarSpec`'s in `runtime.rs` say
  "without the dot": say a compound one such as `blade.php` is allowed and
  wins over the plain extension. `extensions/grammars/README.md`: one
  sentence on compound extensions.
- Tests: fe-syntax (registered `zz.blade`-style fixture language claiming a
  compound extension beats a registered and a built-in plain one; case;
  `blade.php` alone isn't matched), the grammar prompt (offers the compound
  owner, not the plain one), and a wasm_parity fixture case.
- Out of scope: LSP server choice (`fe-lsp` by extension), file icons,
  shebang/first-line detection.

## Implementation notes

- The matcher is `fe_syntax::Language::file_has_extension(file_name, extension)`,
  an associated function because `lib.rs` (its re-exports) is outside the
  card's scope. `offer_for` uses it, picking the first catalog entry with the
  longest matching claim.
- "Longer than it" is read as a non-empty name before the claim's dot, as
  `Path::extension` reads `.bashrc`: `.blade.php` has no `blade.php`, as
  `blade.php` hasn't. A claim with an empty part (`.php`, `a..php`) matches
  nothing, as a dotted claim did before.
- Precedence in `language_name_for_path`: path tails, then built-in file
  names, then registered file names, then the longest compound claim (a
  built-in's on a tie, else the first registered), then, only when no
  compound claim matched, plain extensions as before.
- `wasm_parity`'s built-in check for a dotted claim asks `register_language`
  with a probe spec whose module doesn't exist: its clash check, which runs
  first and compares whole claims, answers, and nothing registers.
- The offer's "another language has the file" guard asks
  `Language::extension_would_take(path, extension)`: whether the entry's
  claim, registered now (after the rest, so losing ties), would be given the
  file. A compound offer is made over a plain or shorter compound owner, not
  over an equal or longer one or a file-name/path-tail owner (review 71mx7p F1).
