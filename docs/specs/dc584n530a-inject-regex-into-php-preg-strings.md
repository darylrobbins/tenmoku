---
task_uid: "01m7c01g3f60hkrndc584n530a"
task: "T637"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features regex,php,ruby --test regex_injections_php_ruby passes: php_preg_pattern_body_is_a_regex_without_delimiters_or_flags, php_pattern_with_other_delimiters, php_non_preg_calls_not_injected, ruby_regex_literal_is_a_regex"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --test wasm_parity ruby passes with extensions/grammars/ruby/injections.scm changed; php and ruby goldens regenerated and highlights_match_golden_files passes"
    required: true
    manual: false
  - id: "C3"
    text: "Limit stated in php-injections.scm's header: escaped PHP strings only when their content has no backslash"
    required: true
    manual: false
---

# Inject regex into PHP preg_* strings (delimiters and flags trimmed) and Ruby regex literals (Mac T1826)

# Inject regex into PHP preg_* strings and Ruby regex literals

Depends on F (built-in `regex` grammar). Two languages, two shapes:

- PHP (native, `crates/fe-syntax/queries/php-injections.scm`, feature `php` gains `"regex"`): `preg_match`,
  `preg_match_all`, `preg_replace`, `preg_replace_callback`, `preg_split`, `preg_grep`, `preg_quote`-free (the
  pattern argument, first). A PHP pattern is `/body/flags` with any delimiter: the injection must cover only the
  body, so trim the delimiter and the trailing flags: use a `#match?`/`#gsub!`-style offset directive the injection
  engine supports (read `crates/fe-syntax/src/injection/` first for what it has, e.g. `injection.include-children`
  and `#offset!`; if no suitable directive exists, add the smallest one, `#trim-delimiters!`-like, with its test in
  `injection/tests.rs`; that file is in scope for this card only for that). Single-quoted strings (no escapes but
  `\\` and `\'`) are injected when the content has no backslash; double-quoted likewise (state the limit in the file
  header, same as G1).
- Ruby (wasm extension: `extensions/grammars/ruby/injections.scm`, delivered via the grammar registry's separate
  release; wasm unchanged, so no `pack.toml` edit): inject `regex` into `regex` literals (`/.../flags` content,
  `regex` node's `string_content` children; not the interpolations). The literal is source text, so it is exact.
  `%r{...}` likewise. Check the extension's header says what it dropped and update it. Native ruby
  (`ruby` feature, `Def` at language.rs ~860; if it has an `injections` file, mirror it there; if not, leave native
  ruby alone and say so). The `wasm_parity` arm for ruby compares highlights only unless the test says otherwise;
  check `cargo nextest run -p fe-syntax --test wasm_parity ruby` passes.

Goldens: `tests/golden/php.php.golden` and `ruby.rb.golden` regenerate (add a `preg_match` call / a regex literal to
the samples if absent); review that only regex spans change.

**Test.** New file `crates/fe-syntax/tests/regex_injections_php_ruby.rs`,
`#![cfg(all(feature = "regex", feature = "php", feature = "ruby"))]`: `php_preg_pattern_body_is_a_regex_without_delimiters_or_flags`
(`preg_match('/a+b/i', $s)`: `+` operator, the `/` delimiters and `i` flag are still `string`),
`php_pattern_with_other_delimiters` (`'#x(y)#'`, `'~a~'`), `php_non_preg_calls_not_injected`,
`ruby_regex_literal_is_a_regex` (`/\d+/i`, `%r{a|b}`). Base fails. Ruby's extension side is verified by the
wasm_parity run above; if native ruby is not built with an injection, the Ruby test is gated
`#[cfg(feature = "ruby")]` and skipped otherwise.

Conflicts: php-injections.scm; php/ruby goldens; extensions/grammars/ruby. Open tasks touching extensions/grammars:
T1486 (README, pack.toml, wasm_parity) and T1487 (extension.toml of every directory): neither lists injections.scm,
but T1486 lists wasm_parity.rs: this card does not edit wasm_parity. Linux-OK.
