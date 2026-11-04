---
task_uid: "01m6ezg2m2n31zz69kyvqjps9b"
task: "T1336"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test mac_speller::flags_a_misspelling_and_suggests (cargo nextest run -p fastedit) passes on macOS: with language en_US, is_misspelled(\"teh\") is true, is_misspelled(\"the\") is false, and suggest(\"teh\") contains \"the\""
    required: true
    manual: false
  - id: "C2"
    text: "Test mac_speller::auto_language_and_unknown_language_do_not_panic (cargo nextest run -p fastedit) passes on macOS: language \"auto\" uses the system's automatic language identification, and a language tag the system lacks reports every word correct instead of failing"
    required: true
    manual: false
  - id: "C3"
    text: "Test mac_speller::works_off_the_main_thread (cargo nextest run -p fastedit) passes on macOS: is_misspelled and suggest called from a background thread return the same answers as from the main thread"
    required: true
    manual: false
  - id: "C4"
    text: "The test file is #![cfg(target_os = \"macos\")] and the crate still builds with cargo check --workspace on Linux (this card's code is absent there; main.rs falls back to NoSpeller)"
    required: true
    manual: false
  - id: "C5"
    text: "Cargo.lock gains no new [[package]]: only extra features on the objc2-app-kit and objc2-foundation dependencies already under [target.'cfg(target_os = \"macos\")'.dependencies]; cargo deny check licenses bans sources passes"
    required: true
    manual: false
  - id: "C6"
    text: "No word is learned into the system-wide dictionary: the binding never calls learnWord or ignoreWord:inSpellDocumentWithTag, since the Tenmoku dictionary (f9a) owns that"
    required: true
    manual: false
---

# Bind the macOS system spell checker (NSSpellChecker) as the app's Speller, in the app crate and compiled only on macOS

## Problem
The first release is macOS-only and should use the system's dictionaries, languages and user-learned words. This is the only macOS-specific spelling code; f9a defines the `Speller` trait and the other cards test with a fake.

## Change
New crates/fastedit/src/mac_speller.rs (`#[cfg(target_os = "macos")]`) implementing `fe_workbench::spell::Speller` with `NSSpellChecker.sharedSpellChecker`: `checkSpellingOfString:startingAt:language:wrap:inSpellDocumentWithTag:wordCount:` for is_misspelled, `guessesForWordRange:inString:language:inSpellDocumentWithTag:` for suggest, a `NSSpellChecker::uniqueSpellDocumentTag` for the app, `setAutomaticallyIdentifiesLanguages` for "auto". Add the needed `NSSpellChecker` feature flag to objc2-app-kit (and NSString/NSRange if required) in crates/fastedit/Cargo.toml. NSSpellChecker is not documented as thread-safe: serialise calls behind one Mutex and, if a call on a background thread misbehaves, marshal to the main thread with the existing NSThread helper (see crates/fastedit/src/app_icon.rs for the cfg and objc2 style). In main.rs, one line: on macOS set the Workbench's speller to MacSpeller; elsewhere leave NoSpeller. Skip tests if the spell server is unavailable only with a printed reason (CI macOS runners have it).

## Lead note after review round 2 (rev 2)

Two findings stand; fix both in this task.
- F2 (C2): "auto" must use the system's automatic language identification, as Change says: setAutomaticallyIdentifiesLanguages(true) and the checker's own identification for checking and guesses, not a loop over userPreferredLanguages with identification turned off. An explicit tag still turns identification off and uses that tag. The C2 test must show auto accepting a correctly spelled word in a second installed language (e.g. "bonjour" or "Haus") as well as English, without naming the language; skip with a printed reason if no second language is installed. If the system's identification really cannot classify single words, show that in the finish note with the probe output instead of substituting another algorithm.
- F4: with harness = false the tests/mac_speller.rs binary must build and exit 0 on Linux too: give it a non-macOS fn main() {} and keep the macOS tests behind cfg(target_os = "macos").

## Implementation notes

- One process-wide `Mutex` covers tag allocation and every checker call (windows each build a `MacSpeller`).
- `"auto"` turns on `setAutomaticallyIdentifiesLanguages` and checks through `checkString:range:types:options:inSpellDocumentWithTag:orthography:wordCount:` (the legacy check with a nil language uses the selected language, not identification); suggestions use the language that call identified. Probe on this Mac (preferred en-CA, fr-CA): "maintenant", "fenêtre", "pourquoi" are wrong in en_US, right in fr and accepted by auto; "qzxjv" is flagged. Single-word identification has quirks: "teh" is accepted by auto (identified as another language), so the C2 test never asserts it.
- `tests/mac_speller.rs` is `harness = false` with a small libtest-compatible `main` (`--list`, `--exact`, filters), so the tests run on the process's real main thread and the main-thread test asserts that.
