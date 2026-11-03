---
task_uid: "01m6ffyb57ymzzs0pp118n3cn5"
task: "T426"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "grammar_prompt_tests::offers_for_an_uninstalled_catalog_language: opening a.kt in a workbench with a test catalog containing kotlin (ext kt, 2.8 MB) and no registered Kotlin shows one toast \"Syntax highlighting for Kotlin isn't installed\" with actions Install (2.8 MB), Not now and Never for .kt; the tab is open and editable at once"
    required: true
    manual: false
  - id: "C2"
    text: "grammar_prompt_tests::nothing_downloads_without_the_click: until Install is invoked the injected fetcher has been called zero times, including after Not now, after closing the toast and after opening ten more .kt files"
    required: true
    manual: false
  - id: "C3"
    text: "grammar_prompt_tests::install_fetches_registers_and_rehighlights: invoking Install calls the fetcher once with the catalog entry, then the installer; when the language registers (fixture grammar), every open tab of that language is re-highlighted (its syntax generation advances and a highlighted row has a non-default colour) and the toast is replaced by \"Installed Kotlin syntax\""
    required: true
    manual: false
  - id: "C4"
    text: "grammar_prompt_tests::failure_is_one_readable_line: a fetch error (offline, hash mismatch) toasts one line naming the language and the reason, adds a \"pack\" hint for offline, and leaves the tab plain; asking again later is possible"
    required: true
    manual: false
  - id: "C5"
    text: "grammar_prompt_tests::not_now_is_per_session_and_never_is_remembered: Not now silences that language until the next launch; Never for .kt is written to the state directory (grammar_prompt.json beside recent.rs's file, not settings.toml) and a fresh Workbench reading that file does not offer again for .kt but still offers for another catalog language"
    required: true
    manual: false
  - id: "C6"
    text: "grammar_prompt_tests::no_offer_when_it_would_not_help: no toast for a language already registered or built in, for a file with no catalog match, for an unreleased catalog entry (empty sha256), or while another install of the same language is running (one toast, one fetch)"
    required: true
    manual: false
  - id: "C7"
    text: "tests/grammar_prompt.rs (fe-workbench integration): two tabs open at once (a.kt, b.kt) produce one toast and one install; closing the workbench mid-fetch drops the task without panic"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p fe-workbench -p fastedit passes; cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Offer to install syntax highlighting when a file opens in a language that has a first-party wasm grammar but is not installed: a toast with Install (size), Not now and Never for .ext, fetching on the click only (Mac T1321)

Second half of card 5 in docs/grammar-size-audit/README.md ("What the user sees opening such a file"). Builds on the fetch and install card (fe-net grammar_fetch, fastedit grammar_install, extensions/grammars/pack.toml).

Design calls (record in docs/decisions/<lane>-*.md):
1. The "banner" is a toast with three actions, not a new in-editor widget. The only editor banner today (pane.rs viewer.set_banner) is text with no actions, and a new pane-chrome widget is a rendering change that cannot be pixel-checked on the Linux host. Toasts (crates/fe-workbench/src/toast.rs, as T1309's update toast uses) already carry actions and run under TestAppContext. A pane-embedded banner can replace it later without touching the catalog or installer.
2. The catalog is pack.toml's languages with their extensions (read from the embedded extensions/grammars/<lang>/extension.toml). The workbench receives a `GrammarOffers` value (catalog, `installed: Fn(&str)->bool`, `install: Fn(lang)->Task<Result<..>>`) from the app, like the app-supplied extension servers/snippets: fastedit's main.rs wires the real one (fe-net client with grants net:github.com and net:objects.githubusercontent.com, grammar_install), tests wire fakes. fe-workbench therefore gains no network code and no dependency on the installer.
3. Trigger: when a buffer opens (and on file rename/save-as) and fe_syntax::language_for_path finds no language but the catalog matches the extension. Skipped for a language already installed or built in, so while the eight are still built in (before the default-features card) this never fires in the app.
4. Consent: Install is the only thing that calls the fetcher. The toast shows the size from pack.toml. Not now is per session; "Never for .ext" is stored per extension in the state dir.
5. After the extension is written, the kiln loader registers it on its background thread; this card re-highlights open tabs when fe_syntax reports the language registered (poll/subscribe to the existing registration; if none exists, add the smallest generation counter check in grammar_prompt.rs, not in fe-syntax, and block the card rather than widening scope if fe-syntax must change).
6. Plain-text editing, search and LSP meanwhile: nothing here blocks opening.
Files in scope that are shared with other work: crates/fe-workbench/src/lib.rs (T1309 also adds a mod line there; trivial overlap).

## Implementation notes

See `docs/decisions/pp118n3cn5-grammar-prompt.md`. Where the code differs from this spec:

- Grants come from T425's `grammar_install::grants()`, which adds `release-assets.githubusercontent.com`; the two hosts in design call 2 are out of date.
- Kotlin is built into test builds, so C3 and C4's plain tabs use a catalog entry named Kotlin with a fixture language and extension (`ktfix`), and "syntax generation advances" is the view's `syntax()` going from none to that language, as `highlight_epoch` is private to fe-gpui.
- `GrammarOffers` has `fetch` and `install` where design call 2 has `install`: a write can't be called back, so a window closing cancels only the download.
- `crates/fe-gpui/src/{diff_view,view,pane_item}.rs` and `crates/fe-workbench/src/{git_diff,git_log_menu}.rs` joined the scope at review: a language detected for a view with no path must not read as picked by hand.
