---
task_uid: "01m79wx3vwp7rgnmjfcshsa5sm"
task: "T602"
rev: 2
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "Picker::new(d, \"literal\", …) and Button::new(\"literal\") no longer compile (a trybuild or doc compile_fail test in kiln-l10n or kiln-ui)"
    required: true
    manual: false
  - id: "C2"
    text: "No reader of CommandMeta.title is left outside label()'s fallback (rg check recorded in the decision file); cargo nextest run --workspace passes"
    required: true
    manual: false
---

# Finish the i18n switch: no CommandMeta.title reader left, Text's transitional From impls removed (i18n Z1) (Mac T1565)

# Finish the switch: no title readers left, Text's transitional From impls go (Z1)

Spec: card **Z1** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9 and § 8 "Layer 1", as written: check that
no `CommandMeta.title` reader is left; remove `Text`'s transitional `From`
impls (including `From<SharedString>`), turning every remaining argument of
`Picker::new` and `Button::new` into `text!` or `Text::verbatim` (tests and
examples: `Text::verbatim`). The typed-command path is the resolver card's,
already landed.

Scope is wide by design (every crate's src, tests and examples); it runs last
and alone.

Platform: **Linux-OK**.

## Implementation notes

- `Text` has no `From` impl for any string now. The other `impl Into<Text>`
  sinks (`TextInput::new`, `TextField::placeholder`, `TrustPrompt::new`)
  lose the conversions too, so their callers moved as well.
- `tr!` arguments became `text!` with the same key and arguments, so
  placeholders now resolve as they draw. The Commit button's label
  (`CommitButton.label`, `text()`) and a chat History row's label are keyed
  `Text` up to the draw.
- Text a model already holds as resolved strings (toast choices and actions,
  the setup sheet's options, a vim pick's title) and symbols (`+`, `@`,
  `−`) are `Text::verbatim`. Those are layer 2's (section 8).
- C1: `kiln/crates/kiln-l10n/tests/text_from_strings.rs` stops compiling if
  a string converts into `Text` again (an ambiguity check, run by nextest).
  `Button::new` (kiln-ui) and `Picker::new` (kiln-shell) carry
  `compile_fail,E0277` doc tests, which `cargo test --doc` runs.
- C2: the check is in the decision file, § 9 "Z1's check". An rg over the
  files naming the four types, and a field rename checked by the compiler,
  found no reader outside `label()`. The cross-surface test is
  `crates/fe-workbench/tests/command_labels_fake_locale.rs` (G-1, already
  landed).
