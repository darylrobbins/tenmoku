---
task_uid: "01m7c01ghzwctxqat8p9ynpep7"
task: "T638"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --test wasm_parity every_extension_highlights_its_sample --run-ignored only passes for elixir, julia, vue, svelte, vim and starlark, each sample/golden showing a regex highlighted as regex where its injection was restored"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-syntax --test wasm_parity every_extension_is_well_formed passes; each restored file's header says what is restored and what stays dropped"
    required: true
    manual: false
---

# Restore the regex injections elixir, julia, vue, svelte, vim and starlark extensions dropped (Mac T1827)

# Restore the regex injections the wasm extensions dropped

Depends on F (built-in `regex` grammar) and G1 (the `re.*` shape). When no regex language existed, several
extensions' `injections.scm` dropped their regex patterns. Restore them, now that the language is built in:

- `extensions/grammars/elixir/injections.scm` (header line 3: "the heex, regex and vue injections are dropped"):
  restore the `~r` sigil injection; heex and vue stay dropped.
- `extensions/grammars/julia/injections.scm` (header line 3): the `r"..."` prefixed-string regex injection.
- `extensions/grammars/vue/injections.scm` (header ~line 10): `<input pattern>`'s regex injection only if the
  query shape stays reasonable; comment injection stays dropped.
- `extensions/grammars/svelte/injections.scm` (header line 13): regex entry, leave comment/scss entries as dropped.
- `extensions/grammars/vim/injections.scm` (header line 4): the regex injection (vim `=~` patterns?) only where the
  original upstream pattern exists; do not invent new ones; if the original injects `vim` regex-flavoured
  strings that the `regex` grammar would mis-colour, keep it dropped and say so.
- `extensions/grammars/starlark/injections.scm` (header line 2): `re.*` strings (starlark has no `re`; the dropped
  upstream pattern was for strings passed to `re.*`): restore it as upstream wrote it.

For each: edit the query, edit its header to say what is restored and what still is not, no wasm change so no
`pack.toml` edit; these ship via the grammar registry's separate release. Where the original upstream pattern
exists in the extension's pinned source (`grammar-source.toml`), copy it verbatim; do not invent.

`wasm_parity`'s ignored `every_extension_highlights_its_sample` goldens (`extensions/grammars/<lang>/test/*.golden`)
for those six regenerate where a sample contains a regex; add one regex line to a sample only if the extension
already has a test directory.

**Test.** The extension goldens are the regression tests (they fail at base: the regex stays `string`).
`cargo nextest run -p fe-syntax --test wasm_parity every_extension_highlights_its_sample --run-ignored only`
passes for elixir, julia, vue, svelte, vim, starlark; `every_extension_is_well_formed` passes. No new test file;
no `--regression-test` name (the ignored test cannot be named as a regression test).

Conflicts: T1487 (blocked) lists `extensions/grammars/*/extension.toml` only, not the injection files; T1486 lists
wasm_parity.rs, not edited here. Linux-OK where the wasm toolchain runs (lane host decides).
