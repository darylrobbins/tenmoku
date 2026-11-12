---
task_uid: "01m77sakj9bh7m33czyrdmvr4b"
task: "T329"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-lsp exposes a typed Unavailable reason (buffer changed, too large, not open, free text), and lsp_highlights.rs, lsp_format.rs and lsp_format_on_type.rs match its kind; no code in crates/fe-workbench/src compares an Unavailable reason with \"buffer changed\" or \"not open on a language server\""
    required: true
    manual: false
  - id: "C2"
    text: "New test an_unserved_buffer_shows_its_words_in_a_fake_locale (crates/fe-workbench/tests/lsp_reasons_fake_locale.rs) fails at the base and passes with the fix"
    required: true
    manual: false
  - id: "C3"
    text: "LspError's Display is byte-equal in en-US; cargo nextest run -p fe-lsp and -p fe-workbench pass"
    required: true
    manual: false
---

# Highlights and format compare fe-lsp's Unavailable reasons against English: give LspError::Unavailable a typed reason and match its kind (T1561 gap) (Mac T1730)

# Match fe-lsp's Unavailable reasons by kind, not English text (T1561 gap)

Since T1536 (i18n X1), fe-lsp resolves `LspError::Unavailable`'s document reasons with `tr!`
(`crates/fe-lsp/src/event.rs`: `buffer_changed()`, `too_large()`, `not_open()`), but fe-workbench still
compares the reason against en-US text:

- `lsp_highlights.rs` `answer_of`: `why == "not open on a language server"` (the word-occurrence fallback for
  a buffer no server serves);
- `lsp_format.rs` `failure`: `why == "buffer changed"` (a format the user typed over is dropped, not shown as
  an error toast);
- `lsp_format_on_type.rs`: `why == "buffer changed"` (dropped silently).

In any other locale these never match: an unserved buffer shows no occurrence highlights, and a stale
format shows its reason as an error. T1561's worker found this outside its scope.

**Fix in fe-lsp.** Give `Unavailable` a typed reason that callers match on: keep the one variant (every
existing `Unavailable(_)` arm keeps its meaning) and make its payload a public type with the three kinds
(buffer changed, too large, not open) and free text for the rest (feature ids such as `"rename"` and
`FEATURE` consts, other reasons). Resolve the three kinds' text when displayed, so `Display` stays
byte-equal in en-US. Keep `not_open()`/`buffer_changed()`/`too_large()` as the constructors, returning the
typed reason, and give the type `From<&str>`/`From<String>` and `PartialEq<str>`/`PartialEq<&str>`, so the
call sites and feature-id comparisons in other fe-lsp files (`document.rs`, `manager.rs`, `server.rs`,
`hierarchy.rs`, `document_highlight.rs`, …) and fe-workbench (`lsp_rename.rs`, `lsp_links.rs`,
`lsp_lenses.rs`) compile unchanged. Re-export the type from `lib.rs`. fe-workbench's three sites match the
kind. Leave `manager.rs` alone (its `Unavailable("…")` doc examples still read right; it's in T1585's scope).

**Tests.** `crates/fe-workbench/src/lsp_format_tests.rs` (`a_changed_buffer_is_no_error`) and
`lsp_highlights_tests.rs` (`only_an_unserved_buffer_falls_back_to_the_words`) build reasons from English
text. Regatta copies `*_tests.rs` onto the base for the regression run, so they must compile against base
APIs: delete those two tests there and put typed versions in `#[cfg(test)]` modules inside `lsp_format.rs`
and `lsp_highlights.rs` (not in a `*_tests.rs` or `tests/` file). Update fe-lsp's
`src/tests/admission.rs` and `src/tests/catalogue.rs` for the type.

Regression test: new integration test `crates/fe-workbench/tests/lsp_reasons_fake_locale.rs` (its own
binary: the locale is process-wide), public API only, the pattern of `project_text_fake_locale.rs`: a
`qps-ploc` `Resources` defining `lsp-unavailable-not-open` (and the other two) with fake text, switched to
with `kiln::platform::locale::set_locale`. A workbench rooted at a temp dir (so it starts its LSP manager;
the root untrusted, so no server runs) opens a file with `foo bar foo`; the caret rests on `foo`
(advance the clock past 250 ms and draw); the active editor's `DecorationKind::Highlight` decorations are
the two `foo`s. Start with `assert_eq!` on the count so the base run fails on an assertion. Check by hand
that it fails at base (if the workbench has no manager, `ask_highlights` falls back anyway and it won't).

Platform: **Linux-OK**.

## Implementation notes

- The type is `fe_lsp::Unavailable` (`BufferChanged`, `TooLarge`, `NotOpen`, `Other(String)`), the payload of
  `LspError::Unavailable`; its `Display` resolves a kind's key as it's shown. `PartialEq<str>`/`<&str>` hold
  for free text only: a kind equals no string in any locale, so a leftover English comparison fails in en-US
  too rather than only elsewhere.
- Six constructions outside the listed scope built the payload from a `String` (`tr!(…).to_string()`), which
  can't compile against a typed payload unchanged: `code_action.rs` (2), `code_lens.rs`, `completion.rs` and
  `manager.rs` (2, `lsp-not-absolute`) gained `.into()` and nothing else. `manager.rs`'s doc examples are
  untouched.
