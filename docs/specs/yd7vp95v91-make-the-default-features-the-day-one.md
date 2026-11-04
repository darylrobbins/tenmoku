---
task_uid: "01m6ffybfpbe7peryd7vp95v91"
task: "T427"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "default_built_in_set::default_features_are_the_day_one_set: the \"default\" list in crates/fe-syntax/Cargo.toml (read from the file) equals the built-in set the README names (rust, python, javascript, typescript, json, toml, go, c, bash, html, css, yaml, markdown, java, lua, xml, dockerfile, make and the small git/ini/just/diff/json5/csv formats already in default) and names none of cpp, ruby, php, csharp, scala, swift, kotlin, sql; the eight feature definition lines stay"
    required: true
    manual: false
  - id: "C2"
    text: "default_built_in_set::moved_languages_are_not_built_in: under default features fe_syntax::language_by_name returns None for each of the eight and for each of their extensions (kt, kts, cs, scala, swift, cpp, cc, hpp, sql, php, rb); the test is cfg-gated so it compiles out when any of the eight features is on"
    required: true
    manual: false
  - id: "C3"
    text: "default_built_in_set::moved_languages_return_with_their_feature: with --features cpp,ruby,php,csharp,scala,swift,kotlin,sql each of the eight resolves again (the test is cfg(all(feature = ...)) and runs in the full-feature run)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-syntax passes with default features and with --all-features (the eight samples in src/tests/golden.rs are skipped, not failed, when their feature is off); scripts/check-syntax-features.sh passes unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run --workspace passes with default features: no test that names a moved language fails with defaults. Files in scope are gated or ported per the spec's lead decision; a failure in a file outside scope blocks the card rather than widening it"
    required: true
    manual: false
  - id: "C6"
    text: "Tests gated in fe-nav, fe-workbench and fe-gpui still pass with their forwarding features on (cargo nextest run -p \u003ccrate\u003e --features \u003cthe features that crate forwards\u003e), and no language feature was added to any crate's dev-dependencies"
    required: true
    manual: false
---

# Make the default features the day-one built-in set: drop Kotlin, C#, Scala, Swift, C++, SQL, PHP and Ruby from fe-syntax's default, so they load as first-party wasm extensions, and gate every test that names them on its feature (Mac T1322)

Card 4 of docs/grammar-size-audit/README.md ("Proposed follow-up cards"). Lands after T1305 (the wasm grammars for all eight moved languages exist: T1304 Kotlin/C#/Scala/Swift, T1305 C++/SQL/PHP/Ruby) and after the install prompt card, so main never builds an app where eight languages go plain with no way to get highlighting back.

In crates/fe-syntax/Cargo.toml delete the "cpp", "ruby", "php", "csharp", "scala", "swift", "kotlin" and "sql" lines of `default` (the eight feature definitions and optional dependencies stay, so `--features` and scripts/check-syntax-features.sh still build each). fe_syntax::register_language refuses a language with a built-in of the same name or extension (Clash), so a language is built in or an extension, never both: this is the switch that lets the first-party extensions under extensions/grammars/ register. Put a one-line comment on each of the eight Definitions in language.rs saying it is an extension by default.

Outline, definitions and indent queries stay compiled into fe-syntax per language (about 100 KB, README "On-demand path"); with the language not built in they are simply unused until it registers. Check, do not assume, that nothing panics when an outline/indent lookup finds no built-in language for an extension-registered one.

Most tests naming a moved language already carry cfg(feature); check every file in scope and add the missing gates (a whole-file `#![cfg(feature = ...)]` where the file is about one language). New test file crates/fe-syntax/tests/default_built_in_set.rs holds the criteria's tests; read the default list from Cargo.toml with include_str! and plain string parsing (no new dev-dependency). Existing tests are gated, never deleted or weakened; wasm_parity.rs keeps testing the wasm path. Record the result (binary size, if measured; otherwise only the default list change) in docs/grammar-size-audit/README.md. Do not run the release builds there (that is C2, skipped). Design decisions go in docs/decisions/<lane>-*.md.

Headless on Linux: everything here is plain cargo tests.

## Lead decision on the C5 block (helium core, 2026-11-04)

Attempt 1 blocked, as C5 told it to: with the eight off default, 14 tests outside the original scope fail (fe-nav definition sql_* and punctuated_names_are_found_by_the_fallback_before_the_index_is_built, cpp_includes_rank_their_headers_imported; fe-workbench spell_ranges_tests x4, grammar_prompt_tests a_registered_or_built_in_language_is_not_offered and no_offer_when_it_would_not_help; fe-gpui tests::injected_languages_compiled_meanwhile_draw_again, minimap_tests a_cold_fence_seen_only_in_the_minimap_gets_its_colours). The scope is widened to their files only. No production code outside fe-syntax changes.

For each failing test, decide by what it is about:
- The moved language is the subject (SQL definitions, C++ includes, C++ raw strings and escapes): keep the test unchanged and gate it with cfg(feature), as the fe-syntax tests are. The crate gets a forwarding feature of the same name (for example sql = ["fe-syntax/sql"]) under [features].
- The moved language is incidental (a cold fence, an injected language compiled meanwhile, a language that merely has to be built in): port the test to a language still in the default set, keeping every assertion and what it proves. For a test that needs a cold language, pick one in the default set that nothing has loaded yet.

Do NOT add language features to these crates' dev-dependencies. Feature unification would turn the moved languages on for the whole of a cargo nextest run --workspace, which compiles out C2's test and hides exactly what this card changes.

The list above is what attempt 1 saw on its base. Re-run after merging main: if a test in another file now fails, block again rather than widening.
