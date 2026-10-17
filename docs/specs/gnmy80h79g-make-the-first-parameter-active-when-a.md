---
task_uid: "01m55bwr4feaxnspgnmy80h79g"
task: "T139"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-lsp unit test signature_help::tests::active_default_tests::omitted_active_parameter_is_the_first (new crates/fe-lsp/src/signature_help_active_default_tests.rs, declared by a #[path] mod line at the end of signature_help_tests.rs, calling convert on lsp_types::SignatureHelp built from JSON) passes: one two-parameter signature with no activeParameter gives (active_signature, active_parameter) (0, Some(0)) and the signature's own active_parameter None; [f() with no parameters, the two-parameter one] with activeSignature 1 gives (1, Some(0)) and with none given (0, None); a lone signature with no parameters field gives None; activeParameter 1 gives Some(1) and 9 gives Some(1) (clamped as at the base); it fails at the base ((0, None))"
    required: true
    manual: false
  - id: "C2"
    text: "A given activeParameter, activeSignature and each signature's own activeParameter convert as at the base; the existing tests in signature_help_tests.rs and signature_help_label_tests.rs pass unchanged; the module doc says a missing activeParameter is 0"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited (the one added mod line in signature_help_tests.rs aside); cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Make the first parameter active when a server's signature help leaves activeParameter out, as the LSP spec and VS Code do, instead of none (gopls omits it for the first argument) (Mac T827)

Filed on argon from Mac T827 (card ~/coordination/argon-cards/zzj/T827.json rev 1; note zzj). Mac task uid 01m55bmmcqzb7ajen9fat5e5n2. Base: argon trunk e9d82a2 (= mac/main 76125d0 synced); the files it changes are byte-identical on Mac main 92c57c57. Title the host commit "... (Mac T827)".

## Problem

LSP 3.17 says `SignatureHelp.activeParameter`, when omitted, "defaults to 0 if the active signature has
parameters". VS Code's language client does the same. In the installed VS Code
(extensions/css-language-features/client/dist/node/cssClientMain.js, the signature help converter):
`Et.number(u.activeParameter) ? T.activeParameter = u.activeParameter : u.activeParameter === null ?
T.activeParameter = -1 : T.activeParameter = 0`. So VS Code highlights the first parameter.

fe-lsp gives no active parameter in that case. In `crates/fe-lsp/src/signature_help.rs` `convert`,
`clamp(help.active_parameter, count)` turns a missing value into `None`. gopls declares `ActiveParameter` with
`omitempty`, so it leaves the field out exactly when the first argument is active. At Mac main 92c57c57,
typing `f(` in Go therefore highlights nothing until the second argument.

fe-lsp doesn't advertise `noActiveParameterSupport`, so a server may not send `null` here. A missing value and
a `null` both deserialize to `None`, and both mean 0.

signature_help.rs and signature_help_tests.rs are byte-identical at mac/main ecdf8603 and Mac main 92c57c57,
so no sync is needed.

## Change

In `convert` in `crates/fe-lsp/src/signature_help.rs`, take a missing `activeParameter` as 0 before clamping
it against the active signature's parameters:

- With parameters, the result is `Some(0)`.
- With none, it stays `None`.

A value that is given keeps today's handling, clamped to the last parameter. A signature's own
`activeParameter` (`Signature::active_parameter`) stays as given: `None` when left out, so the help-level value
applies, as fe-gpui already reads it. Update the module doc's "Active ones" bullet.

The test goes in a new `crates/fe-lsp/src/signature_help_active_default_tests.rs`. It calls `convert`
directly, as `signature_help_label_tests.rs` calls `signature`. It is declared by
`#[path = "signature_help_active_default_tests.rs"] mod active_default_tests;` at the end of
`signature_help_tests.rs`, which is the only edit there.
