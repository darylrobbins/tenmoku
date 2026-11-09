---
task_uid: "01m6z3ssbg1221mt9qqw992dfx"
task: "T1631"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "scripts/build-grammar-wasm.sh builds each distinct module once per run, or gives every job its own temp file, so two directories with the same grammar export never share a temp path; a run naming two such directories builds and checks both deterministically"
    required: true
    manual: false
  - id: "C2"
    text: "wasm_parity's problems() reports two directories whose grammar export is the same but whose source keys differ, naming both; a fixture case shows a shared export with an identical source passing and a differing rev failing"
    required: true
    manual: false
  - id: "C3"
    text: "extensions/grammars/README.md says one grammar may back several languages: one directory each, the same source, their own queries and samples"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-syntax --test wasm_parity passes"
    required: true
    manual: false
---

# Let two first-party grammar directories share one tree-sitter export (ERB and EJS): build-grammar-wasm.sh builds each module once with no shared temp file, and wasm_parity requires them to pin the same source

# Two first-party grammar directories may share one tree-sitter export

Prerequisite for the ERB/EJS card. tree-sitter-embedded-template is one
parser for several languages that differ only in their injections (its
`tree-sitter.json` lists erb, ejs and etlua with `injections-erb.scm`,
`injections-ejs.scm`, ...). fastedit's rule is one language per directory
(`crates/fe-syntax/tests/wasm_parity.rs` `check_manifest`: exactly one
`[[contributes.grammars]]`), so ERB and EJS are two directories with the
same `grammar = "embedded_template"`, hence the same module
`tree-sitter-embedded-template.wasm`. Two things break today:

1. `scripts/build-grammar-wasm.sh` `build()` writes `temp=$wasm.$$.tmp`
   then `mv "$temp" "$wasm"`, run as background jobs. `$$` is the parent
   shell's PID in every job, so two jobs for the same module share one temp
   path: one `mv` can find the file gone and fail the run (a whole build in
   `.github/workflows/grammars.yml` runs `ejs`, `elixir`, `elm`, `erb` in
   adjacent slots).
2. Nothing checks that directories sharing a module build it from the same
   source: two pins would silently overwrite each other's module.

## Do

- Script: build each distinct module once per run (the languages sharing an
  export reuse it), or at least give every job its own temp file
  (`mktemp "$wasm.XXXXXX"`, or `$BASHPID`); either way the run must be
  deterministic. Both languages are still checked by
  `every_extension_highlights_its_sample`.
- `wasm_parity.rs` `problems()`: directories whose `grammar` (so module
  name) is the same must have the same source keys (`git`, `rev`, `src`; or
  `registry`, `crate`, `version`, `sha256`, `src`; or `crate`, `version`,
  `src`); report the pair otherwise. Add a fixture case to the test that
  exercises `problems()` with zz-languages (as the parity_needs cases do):
  two directories sharing an export and a source pass, a different `rev`
  fails naming both.
- `extensions/grammars/README.md`: a short paragraph: one grammar may back
  several languages (one directory each, same source, own queries and
  sample), with embedded-template's ERB/EJS as the example.
- Nothing else: no app, installer or pack change is needed (each language
  installs its own copy under `tenmoku.grammar_<language>/`;
  `scripts/package-language-pack.sh` and `scripts/record-grammar-hashes.sh`
  work per language, and the release has one asset for both). Check that
  claim by reading those two scripts and `crates/fastedit/src/grammar_install.rs`;
  if one does need a change, block saying which.
