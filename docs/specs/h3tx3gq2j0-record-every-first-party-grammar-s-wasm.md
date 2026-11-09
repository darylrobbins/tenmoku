---
task_uid: "01m70e50ak7a39d9h3tx3gq2j0"
task: "T1657"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Evidence: gh release view grammars-2 --repo darylrobbins/tenmoku lists a wasm for every directory under extensions/grammars, and scripts/record-grammar-hashes.sh grammars-2 ran; its transcript is in the finish evidence"
    required: true
    manual: false
  - id: "C2"
    text: "extensions/grammars/pack.toml has base_url ending /releases/download/grammars-2 and a [languages.\u003cdir\u003e] entry with file, nonzero size and 64-hex sha256 for every directory beside it, written by the script (git diff touches only pack.toml and the tests)"
    required: true
    manual: false
  - id: "C3"
    text: "every_grammar_directory_has_a_recorded_pack_entry in crates/fastedit/tests/grammar_pack_recorded.rs passes, and pack_holds_its_release_and_recorded_entries in tests/grammar_pack_manifest.rs still passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fastedit passes"
    required: true
    manual: false
---

# Record every first-party grammar's wasm hash in pack.toml against a grammars-2 release so groovy, properties, kotlin and the rest can be fetched (interim until the registry replaces pack.toml, T1486)

# Record the first-party grammars' hashes against the grammars-2 release (interim)

User report (2026-11-09): Java ecosystem support is poor; Gradle files and
properties files aren't highlighted. Groovy (`.gradle`), properties
(`gradle.properties`, `.properties`) and Kotlin (`.kt`, `.kts`) already exist as
first-party WebAssembly grammars (`extensions/grammars/groovy`, `properties`,
`kotlin`), but `extensions/grammars/pack.toml` has a `base_url` and no
`[languages.*]` entry, so `crates/fastedit/src/grammar_install.rs` refuses every
fetch as not released (see docs/decisions/c1pt00h80c-grammar-fetch.md, "The hash
is recorded in the app"). This is an INTERIM unblock for the next app release.
The registry chain (T1483 -> T1484 -> T1485 -> T1486) replaces pack.toml's hashes
later; T1486 deletes pack.toml and scripts/record-grammar-hashes.sh, so do not
build anything here that outlives them.

## Which release: grammars-2 (lead override)

v0.1.2's tag can never hold the wasm: its grammars.yml run fails on four stale
goldens (astro, ejs, svelte, vue; fixed by T1656), so a rerun of that tag fails
the same way. Recording against the next app release (v0.1.3) would only help
the release after it, since pack.toml is compiled in. So, as grammars-1 was
made: after T1656 lands, the LEAD pushes a `grammars-2` tag on main, creates its
GitHub release and runs grammars.yml on that tag (workflow_dispatch from a tag
uploads to the tag's release). This card then runs
`scripts/record-grammar-hashes.sh grammars-2` (it moves `base_url` to it), so
the next app release (v0.1.3) fetches every first-party grammar.

## Human step first

Check `gh release view grammars-2 --repo darylrobbins/tenmoku --json assets -q '.assets[].name'`
lists a tree-sitter-*.wasm for every directory under extensions/grammars. If
not, block the card asking the lead to publish grammars-2. Do not push a tag or
create a release from this card.

## Do

- Run `scripts/record-grammar-hashes.sh grammars-2` (needs `gh`); commit the
  rewritten extensions/grammars/pack.toml. Don't edit entries by hand.
- Fix whatever `crates/fastedit/tests/grammar_pack_manifest.rs`
  (`pack_holds_its_release_and_recorded_entries`) and `grammar_install_tests`
  say about a filled pack.toml, if they assume the empty one.
- New test file `crates/fastedit/tests/grammar_pack_recorded.rs`, a new test
  binary: `every_grammar_directory_has_a_recorded_pack_entry` reads
  extensions/grammars/pack.toml and every directory beside it and asserts each
  has `[languages.<dir>]` with the wasm file its extension.toml names, a nonzero
  size no larger than `fe_syntax::MAX_GRAMMAR_BYTES`, and a 64-hex sha256. A
  directory may be listed in a `NOT_RELEASED` const with a one-line reason in
  the test; it starts empty.
- Header comment in pack.toml: one line saying the recorded release is grammars-2
  and that T1486 retires these entries.

Out of scope: any app behaviour, the in-app install prompt for groovy/properties
(already offered by file extension), the registry cards, a new release.
A user only gets the fetch in an app built after this lands (the shipped 0.1.2
has empty hashes), so note that in the finish summary.

Platform: Linux-OK (script needs network and gh; the test doesn't).
