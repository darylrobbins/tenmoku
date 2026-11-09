---
task_uid: "01m6z3t59bn2p6wsak9s8q2ygw"
task: "T1633"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "wasm_parity's problems() allows a directory with no extensions or file names only when another directory's injections.scm names it as injection.language, with fixture cases for both outcomes, and every_extension_is_well_formed passes on the real tree"
    required: true
    manual: false
  - id: "C2"
    text: "grammar_install leaves an extension-less first-party language out of the offers; installing a first-party language by any route (the prompt's install, install_first_party, the language pack) also installs the released, not yet installed extension-less first-party languages its injections.scm names, the offered size including them, and a failing dependency fails the install with nothing half written; grammar_install_tests show each with fixture catalogs; past the 60 s registration deadline a dependency that has not registered keeps the existing warn-and-keep-polling behaviour, and once every required language registers the install completes and refreshes the tabs of every host waiting on it (a grammar_install_tests case with two hosts sharing a dependency that registers after the deadline, asserting eventual highlighting in both)"
    required: true
    manual: false
  - id: "C3"
    text: "extensions/grammars/README.md documents injection-only languages"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-syntax -p fastedit passes"
    required: true
    manual: false
---

# Add injection-only first-party grammars: php_only (for Blade), allowed with no extensions when another grammar injects it, and installed alongside the language that injects it

# Injection-only first-party grammars: php_only, installed with what injects it

Prerequisite for the Blade card. Blade injects PHP code that has no
`<?php` tag (`{{ $x }}`, `@if($a)`) as `php_only`, tree-sitter-php's
second grammar; the first-party `php` grammar parses such text as HTML.
`php_only` claims no file extension, and today a first-party directory must
(`wasm_parity.rs` `check_manifest`: "its grammar has no extensions, which
the install offer is for"), and only extensions lead to an install
(`crates/fe-workbench/src/grammar_prompt.rs`), so nothing would ever
install it.

## Do

- `extensions/grammars/php_only/`, a normal grammar directory: source
  tree-sitter-php 0.24.2 (the version `php/` pins), `src = "php_only/src"`,
  `grammar = "php_only"`, in the crates.io form (`registry`, `crate`,
  `version`, `sha256`, `src`, `grammar`) since php_only isn't built in;
  `extensions = []`; `highlights.scm` and `injections.scm` from the crate's
  php queries (php_only shares php's node names; credit them as `php/`
  does), `parity_needs = ["sql"]`; licence; a sample of tagless PHP and its
  golden file; a THIRD_PARTY.md WebAssembly row.
- `wasm_parity.rs`: a directory may claim no extension or file name only if
  another directory's `injections.scm` names it in `injection.language`;
  else the current problem stands. Fixture cases for both.
- Install (`crates/fastedit/src/grammar_install.rs`): an extension-less
  first-party language is never offered on its own (left out of the
  catalog's offers); installing a first-party language, by every route that
  installs one (the prompt's install, `install_first_party`, the language
  pack), also installs the extension-less first-party languages its
  `injections.scm` names, under the same consent, when released and not
  already installed. The offer's size includes them. A dependency that
  fails to install fails the install with its error, leaving nothing half
  written. Tests in `grammar_install_tests.rs`.
- `extensions/grammars/README.md`: injection-only languages.
- Out of scope: uninstalling (a dependency stays installed), changing
  `php/`, any fe-syntax change (an injected language that isn't installed
  already stays plain).

If the lead prefers not to add this plumbing, Blade's card instead drops
its php_only injections (directives, HTML and JavaScript still colour; PHP
expressions stay plain) and this card isn't filed.

## Rev 2 (lead, 11-09): php_only moves to the Blade card

The new wasm_parity rule allows an extension-less directory only when another
directory's `injections.scm` injects it, and nothing does until Blade (T1640).
So `extensions/grammars/php_only/` and its THIRD_PARTY.md row move to T1640,
which lands Blade and php_only together. This card ships the wasm_parity rule
(fixture cases for both outcomes, the real tree still passing), the install
plumbing (tested with fixture catalogs in `grammar_install_tests.rs`, not the
real php_only) and the README section. Ignore the first bullet of "Do" above.
