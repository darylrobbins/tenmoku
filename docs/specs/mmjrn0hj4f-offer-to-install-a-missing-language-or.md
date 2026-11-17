---
task_uid: "01m6sq8qzxsr74chmmjrn0hj4f"
task: "T1485"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench's tests/grammar_prompt_packs.rs passes: offers_the_language_and_its_main_pack, no_pack_button_when_nothing_else_in_the_pack_is_missing, dont_ask_is_per_language_and_survives_a_new_process, old_per_extension_never_entries_still_silence_the_language, a_file_name_or_shebang_match_offers_like_an_extension, pack_install_installs_each_missing_language_and_rehighlights_their_tabs, a_pack_member_failing_names_it_and_installs_the_rest"
    required: true
    manual: false
  - id: "C2"
    text: "grammar_install_index_tests passes: install_from_an_entry_writes_every_file_and_the_loader_registers_it, any_file_hash_mismatch_writes_nothing, a_file_outside_the_allowed_set_is_refused, a_manifest_naming_another_language_or_wasm_is_refused, incompatible_entries_are_never_offered"
    required: true
    manual: false
  - id: "C3"
    text: "Evidence: the ignored grammar_install_index_tests::live_install_from_the_registry passes against the published index, and the decision file quotes its run"
    required: true
    manual: false
  - id: "C4"
    text: "offers() builds its catalog from the verified index: no non-test code reads pack.toml or the compiled-in extension files; every file is fetched through fe_net::grammar_fetch and written through stage_extension and publish"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-workbench/src/lib.rs gains at most one appended pub use line and no other change; the existing grammar_prompt tests pass, and the decision file lists any changed for the new meaning of Never; cargo nextest run -p fe-workbench -p fastedit passes; cargo clippy -p fe-workbench -p fastedit --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Offer to install a missing language or its pack on first open, from the verified index: Install <Language>, Install <Pack> pack, Not now, and a persisted Don't ask for <Language>; re-highlight open editors in place

# First-open install prompt: a language or its pack, from the verified index

Today `crates/fe-workbench/src/grammar_prompt.rs` offers the eight languages in
`pack.toml`, by file extension: Install (size), Not now, Never for .ext. The
download is the wasm alone, checked against the hash `pack.toml` records, and
the queries and manifest are compiled in. This card moves that prompt and that
install onto the signed index (card 3), so a language added to the registry
needs no app release:

- The offer comes from the cached, verified index. Its detection covers whole
  file names and shebangs as well as extensions, for languages that aren't
  installed (`grammar_registry::language_for`).
- The choices are **Install <Language> (size)**, **Install <Pack> pack
  (<n> languages, size)**, **Not now** and **Don't ask for <Language>**. The
  pack button appears only when the language's main pack (its first-listed)
  has at least one other compatible language not installed. Its size is the
  sum of the missing languages'.
- **Don't ask for <Language>** is per language, persisted in the state
  directory's `grammar_prompt.json`. Read it again before each offer, as Never
  is today, and write it under the same lock and rename. Migrate the old
  per-extension `never` entries: an extension listed there still silences the
  language whose entry claims it, and the file keeps them while it adds a
  `never_languages` array.
- It stays quiet: an info toast that never steals focus, once per language
  per launch. Not now and closing still silence the language until the next
  launch, and the one-at-a-time rules stay as they are. Nothing is fetched
  without a click, apart from card 3's index refresh.

Read first: `docs/decisions/pp118n3cn5-grammar-prompt.md`,
`docs/decisions/c1pt00h80c-grammar-fetch.md`, `grammar_prompt.rs` and its tests
(`src/grammar_prompt_tests.rs`, `tests/grammar_prompt.rs`), `grammar_install.rs`,
and card 3's `grammar_registry.rs` and decision file.

## Install from the index, through the existing path

Reuse what's there; don't write a second installer.

- `grammar_install`: a `Grammar` comes from an index entry, no longer from
  `Pack` plus compiled-in files. It carries the entry's `files` (name, size,
  sha256), and `url = url_template` with `{tag}` and `{file}` filled in. Fetch
  each file with `fe_net::grammar_fetch` (hash and size checked, granted hosts
  only), all before anything is written. Then `stage_extension` writes the
  fetched files (it loops over files already) and `publish` renames the
  directory into place, whole or not at all, as now.
- Refuse any entry file that isn't a plain name in the allowed set: the wasm
  the entry's `extension.toml` names, `extension.toml`, `highlights.scm`,
  `injections.scm`, `LICENSE`, `LICENSE-*` and `NOTICE`. Also check that the
  fetched `extension.toml`'s language and wasm match the entry. Nothing is
  written on any mismatch.
- A pack install is the languages one after another, one toast saying
  "Installing JVM pack (2 of 4)…". One language failing names it and carries
  on with the rest. Each language registers and re-highlights as it lands: run
  the existing `report_registered` and `rehighlight` for each, so open editors
  and diff views of every pack language pick it up in place.
- `GrammarOffer` gains `file_names`, `shebangs` and its packs; `GrammarOffers`
  gains the packs. Language matching in `offer_for` moves to a closure the
  offers supply (backed by `grammar_registry::language_for`), so fe-workbench
  stays free of index code, and the shebang needs the editor's first line.
  New public types in fe-workbench go in **one new appended `pub use` line** in
  `crates/fe-workbench/src/lib.rs`. T1423 makes appended lines there free;
  don't edit the existing `pub use grammar_prompt::{…}` line.
- `offers()` stops reading `pack.toml` and the compiled-in extension files. If
  that leaves `Pack`, `install_from_pack_dir` or T1433's `build.rs` table used
  only by tests, leave them for card 5 to delete and say so in the decision
  file. Don't delete `pack.toml` here.
- With no cached index (first launch offline, or automatic checks off),
  nothing is offered. Once a file of an unknown kind opens, an index fetch on
  that explicit path is fine only if `update.automatic_checks` is on. Say
  which you chose.
- The offline-failure hint names the pack tarballs
  (docs/release/language-pack.md), as today's hint names the language pack.

## Tests (new files)

`crates/fe-workbench/tests/grammar_prompt_packs.rs`, on the public API with
fake offers, as `tests/grammar_prompt.rs` does:

- `offers_the_language_and_its_main_pack`
- `no_pack_button_when_nothing_else_in_the_pack_is_missing`
- `dont_ask_is_per_language_and_survives_a_new_process`: a fresh `Prompts`
  that reads the file
- `old_per_extension_never_entries_still_silence_the_language`
- `a_file_name_or_shebang_match_offers_like_an_extension`
- `pack_install_installs_each_missing_language_and_rehighlights_their_tabs`
- `a_pack_member_failing_names_it_and_installs_the_rest`

`crates/fastedit/src/grammar_install_index_tests.rs` (declared from main.rs):

- `install_from_an_entry_writes_every_file_and_the_loader_registers_it`, using
  the real loader as `tests/extension_grammars.rs` does, with
  `crates/fe-syntax/tests/fixtures/tree-sitter-json.wasm` under a made-up
  language name
- `any_file_hash_mismatch_writes_nothing`
- `a_file_outside_the_allowed_set_is_refused`
- `a_manifest_naming_another_language_or_wasm_is_refused`
- `incompatible_entries_are_never_offered`
- `live_install_from_the_registry` (`#[ignore]`, network): installs one real
  language from the live index into a temp extensions directory, and the loader
  registers it. Quote its run in the decision file.

Update existing tests only where Never's meaning changed, and say which ones in
the decision file. `grammar_prompt_tests.rs` and `tests/grammar_prompt.rs` keep
their rules otherwise.

Decision file `docs/decisions/<lane>-grammar-prompt-packs.md`.

## Strings (lead call 2026-11-12)

This card now waits for T1560, which moves `grammar_prompt.rs`'s text into
`crates/fe-workbench/l10n/en-US/extensions.ftl` (keys `workbench-extensions-…`) and lists the file in
`l10n-allow.toml`'s `migrated` list. Every new user-facing string this card adds to
`grammar_prompt.rs` goes through that catalogue in the same way, so the l10n lint passes.
