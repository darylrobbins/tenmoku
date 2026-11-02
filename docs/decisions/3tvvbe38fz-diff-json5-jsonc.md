# Diff, JSON5 and JSONC (T1299)

- `diff` is crates.io `tree-sitter-diff` 0.1.0, `json5` is vendored at `248b856` (`vendor/tree-sitter-json5`, shaped like
  T1282's gitignore crate; ABI 15, no scanner, not regenerated), each behind a default Cargo feature.
- Size: 33,088 bytes of release binary, measured on a small example linking `fe-syntax` (release profile, macOS arm64)
  built with all default features and again without `diff` and `json5`; the spec's estimate was about 36 KB.
- Theme colours: each theme's own green and red. The Catppuccin and VS Code themes use the colours of their
  `change_added` and `change_deleted`. Tenmoku's change colours are muted for the gutter, so the Tenmoku themes use
  their `string` green and their `markup.heading` red. VS Code's own `markup.inserted` and `markup.deleted` aren't taken:
  Dark Modern's are a pale green and an orange, which doesn't read as green and red.
- The crates.io package of `tree-sitter-diff` has no `LICENSE` file, and its manifest's `repository` names
  `tree-sitter/tree-sitter-diff`. Its source is `the-mikedavis/tree-sitter-diff`, whose `LICENSE` is MIT and which
  `THIRD_PARTY.md` links. Generating `THIRD_PARTY_LICENSES` for a release has to take the text from there.
- A trailing comma in JSON is still an `ERROR` node, as `tree-sitter-json` has no rule for it. `tests/data_formats.rs`
  pins that highlighting survives it: comments and every key keep their colours, including keys after the comma.
- Limits of the diff grammar, accepted, both checked on a `git format-patch` sample. It reads a line by its first
  characters, so a removed line whose text starts `-- ` (a SQL or Lua comment) parses as `---` and an error: only the
  `---` is painted and the rest stays plain. In a `git format-patch` file the `---` separator and the signature's `-- `
  are deletions, and the mail headers and body are plain context. `comment` nodes stay plain, and diff has no comment
  tokens, since toggling a comment in a patch would change the patch.
- Left default: the parse-limit ratio (40) for `diff` and `json5`, though `json` has 30. No indent or outline query for
  `json5`, which the card doesn't ask for: it gets no outline, and a new row's indent copies the row before.
- With the `json5` feature off, a `json5` fence is now plain instead of JSON, as dropping the alias means.
- Outside the card's listed scope: `crates/fe-syntax/tests/fence_language_vscode_names.rs` compiles the languages its
  fences inject before it runs, and its `json5` case now needs `json5` compiled. It gains that language and its feature
  in the `cfg`; the case itself is unchanged.
- `cargo deny check licenses bans sources` passes, which is what the gate's `deny` stage runs. `cargo deny check` as a whole
  fails on `advisories`, for RUSTSEC entries against existing dependencies (ctutils, pulldown-cmark, wasmtime and
  wasmtime-wasi). None is against a crate this card adds, and the only `Cargo.lock` change here adds the two grammar crates
  and removes nothing. T1301 owns clearing those six advisories, including whether Tenmoku reaches each one, so this card
  doesn't bump them: the bare `cargo deny check` stays red until T1301 lands, and C5 (rev 2) asks only for the gate's check.
- The vendored json5 binding has no unit test: the one in the gitignore crate's shape names `tree_sitter`, which this
  standalone crate doesn't depend on, so it can't compile. `tests::each_language_highlights_a_snippet` and the golden test
  in `fe-syntax` load the grammar.
