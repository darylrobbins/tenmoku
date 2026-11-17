# The first-open prompt from the signed index: a language or its pack (T1485)

Card 4 of the grammar registry. Card 3 (`xdwbnpg9a4-grammar-registry-client.md`)
fetches, verifies and caches the index; this card offers and installs from it.
The prompt's earlier records still hold where this one doesn't say otherwise:
`pp118n3cn5-grammar-prompt.md` (T426) and `c1pt00h80c-grammar-fetch.md` (T425).

## Where the catalog comes from

`grammar_registry::start` now hands the app every catalog it has, through a
`publish` callback that `main.rs` turns into `grammar_install::offers`:

1. At launch, the cached index, read and verified on a background thread,
   then published on the main thread. No cache (first launch, offline, or
   automatic checks off) publishes an empty catalog, so nothing is offered.
2. If `Policy` says a refresh is due, card 3's refresh, then the new catalog.

Both run in one task, one after the other, so a slow cache read can't land
after a refresh and put an older catalog back. The cache is read whatever the
policy says: it's a local file, and an index fetched under an earlier consent
is still a verified index.

**No fetch on first open.** The spec allowed an index fetch when a file of an
unknown kind opens, with `update.automatic_checks` on. This card doesn't: with
automatic checks on, card 3's launch refresh already fetches a missing cache
in the background at launch, and the refreshed catalog is published as soon as
it lands, so the window where a file of an unknown kind finds no catalog is
the first seconds of the first launch. A second fetch path would be a second
place the app reaches GitHub on its own for little gain.

`offers()` no longer reads `pack.toml` or the compiled-in extension files.
They, `Pack`, `PackGrammar` (what `Grammar` was), `install_from_pack_dir`,
`offers_with`, `install_first_party*`, `fetch_set`, `write_bundle` and T1433's
`build.rs` table (`FIRST_PARTY`) are now used only by `grammar_install_tests.rs`
and are marked as such; card 5 deletes them. `pack.toml` stays.

The download URL is `registry.toml`'s `url_template`, compiled in (card 3's
`Baked`), not the index's own copy. They're the same string (the publisher
checks it), and the compiled-in one can't be redirected by an index, whatever
signs it.

## The offer

The workbench gets `GrammarOffers.packs` (`GrammarPack`: key, name, languages)
and each `GrammarOffer` gets `shebangs` and `packs` (main pack first). The new
public types are in one appended `pub use` line in `lib.rs`.

**Detection** is a closure the app supplies, `GrammarOffers.detect`, which the
app backs with `Catalog::language_to_offer` and the editor's first line (up to
1 KB). That's card 3's `Catalog::language_for` with two of the prompt's rules
from before the index kept (review round 1, F3), since card 3's tests pin
`language_for` as it is: a whole file name matches in any ASCII case, still
before any extension (`GO.MOD` for `go.mod`), for a file no registered
language claims; and for a file one does claim, an entry's extension that the
registered claim wouldn't keep once installed
(`fe_syntax::Language::extension_would_take`) is offered, so `x.blade.php`
offers Blade with PHP built in.
Without one (the workbench's own tests' fake offers), the catalog's claims
decide as before, with the interpreter step added: a file no language has by
its name, whose `#!` program (`fe_syntax::shebang_program`) is an entry's
interpreter, or it with a version suffix, as card 3 matches. `installed` still
has the last word either way.

The buttons: **Install <Language> (size)**, **Install <Pack> pack (n
languages, size)**, **Not now**, **Don't ask for <Language>**. The pack button
shows only when the language's main pack has at least one other released
language that isn't installed and isn't under way in another prompt. Its count
and size are those languages', this one included. The list is taken when the
offer shows and checked again on the click: one that another prompt took up
meanwhile is left to it.

With four answers the toast shows the first, Install <Language>, and puts the
rest in its More menu, as it does for any question with more than two answers
(`Toast::overflow_from`); `toast.rs` is outside this card. Without the pack
button the order is as before (Install, then Not now and Don't ask in the
menu), so keyboard paths through the offer are unchanged.

A language's size is the sum of its entry's files. What it injects isn't in the
index (its `injections.scm` is one of its files), so an injection-only language
it brings (Blade's `php_only`) isn't counted, as T425's pack-based sizes did.
It's fetched with it all the same (below).

**New offers look again.** `set_grammar_offers` bumps a generation and
notifies every workbench, and each editor is looked at again when the
generation it was last looked at with is older (review round 1, F1). So a file
open before the cached or refreshed index arrived, or before a refreshed one
added its language, is offered for then. What's been answered still holds:
Not now, Don't ask, a prompt under way or an install.

## Don't ask

Per language, in `grammar_prompt.json` as `never_languages`, written under the
same lock and rename as before and read again before each offer. The old
`never` entries stay in the file and still count: an extension or file name
listed there silences every language claiming it, so Never for `.kt` from
before keeps Kotlin quiet for `.kts` too. `never` is written back as it was
read, and left out of a file that never had any.

So the T426 rule that a Never for one extension went on to ask about another
of the language waiting on the prompt is gone, and with it the waiting list
(`Busy::pending`): every answer is for the language now. Not now, closing and
one prompt per language are as they were.

**Existing tests changed for the new meaning** (`src/grammar_prompt_tests.rs`):

- `not_now_is_per_session_and_never_is_remembered`: the file holds
  `never_languages`, and `c.kts` no longer asks after Don't ask for Kotlin.
- `a_file_name_offers_its_language_and_never_is_for_the_name`: the buttons and
  the file (`never_languages: ["gomod"]`).
- `a_file_name_claim_beats_an_extension_claim`, `a_compound_extension_offers_its_owner`:
  the last button names the language, not the extension. Which entry was
  offered still shows in the message.
- `a_remembered_never_leaves_the_other_extension_to_ask` became
  `a_remembered_extension_silences_its_language`, and
  `never_goes_on_to_the_other_extension_and_not_now_does_not` became
  `dont_ask_and_not_now_silence_every_extension_of_the_language`.
- `offers_for_an_uninstalled_catalog_language`: the new button titles.
- `failure_is_one_readable_line`: the offline hint's new text.

Every `GrammarOffer` and `GrammarOffers` literal in the old tests gained the
new fields, empty, and nothing else.

## Installing from an entry

`Grammar` is now an index entry's: its language, its `files` (name, size,
SHA-256), and its release, `url_template` with `{tag}` filled in; `url(file)`
fills `{file}`. Fetching (`fetch_grammar`) goes:

1. Every file name must be plain (no separator, not hidden, not empty) and one
   of `extension.toml`, `highlights.scm`, `injections.scm`, `LICENSE`,
   `LICENSE-*`, `NOTICE` or a `.wasm`; the entry must have an `extension.toml`.
   Nothing is fetched otherwise.
2. `extension.toml` is fetched first. Its `id` must be
   `tenmoku.grammar_<language>`, it must have exactly one grammar, that
   grammar's language must be the entry's, and its wasm a plain `.wasm` name
   among the entry's files, the only one: any other wasm is a file outside the
   set. Nothing more is fetched otherwise.
3. The rest, in the entry's order. Each fetch is `fe_net::grammar_fetch` with
   the file's SHA-256 and its size as the limit (a wasm over
   `MAX_GRAMMAR_BYTES` is refused before), under `grants()`. Each file is
   checked against its entry again as it arrives, whatever the fetcher did,
   so a manifest is never parsed before its hash matches.

The write step (`write_entry_bundle`) checks it all again against the catalog
whatever came back from the fetch step: the same names in the same order, each
of its size and hash, the manifest as in 2. Then `write_staged` (what
`write_set`'s body was, now shared with the pack route's `write_set`) stages
each extension with `stage_extension`, which now takes a list of files, and
renames it into place with `publish`, all or nothing, as T425 does. A refusal
is `InstallError::Refused`, one line in the toast.

**Injection-only languages** (no extensions, file names or shebangs, such as
`php_only`) that the fetched `injections.scm` names are fetched with it, and
theirs in turn, unless their extension is already under the extensions
directory, whose `injections.scm` is then read instead. They're bundled and
written with it as T425's pack route does, and the install waits for them to
register (`await_injected`, the loop `offers_with` had) before highlighting the
language's open editors again.

The bundle the workbench carries between the two steps holds each language's
name and its files by name.

## Packs

A pack install runs in the prompt's task: for each language, the toast says
"Installing JVM pack (2 of 4)…", the download runs (the window closing stops
it, and the rest), and once it's in, the write is handed off to a task of no
window, as a single install's is. Each language then registers and its tabs
and diff sides highlight as it lands, through the same `report_registered` and
`rehighlight`, without a toast of its own unless it's slow to load. A language
failing shows "Couldn't install <Language> syntax: …" and the rest go on. At
the end: "Installed JVM pack", or "Installed 3 of 4 languages of the JVM pack".
A language another install brought meanwhile (one the pack's Blade injects) is
skipped.

To make this work the prompt's task is no longer the thing a write lives in:
`Prompts.busy` maps each language under way to its owner, a prompt or a write
with an id of its own, and the window that can stop it (none once its write
has started); the prompts' tasks are kept apart, each dropped with its window.
Only the owner takes a language off (review round 1, F4): a pack member that
failed is released at once, and if its file is opened again while the pack
goes on, the new prompt's claim survives the pack's end. A single install
hands its write off the same way.

**A write is done once it's written** (review round 1, F2). The app's install
step returns when the files are in place; the injection-only languages are
watched for on a task of their own, which highlights the language's editors
again once they've registered. So one that never registers holds up neither
the language's own registration report nor the rest of a pack. The pack.toml
route's `offers_with` still waits, as its tests pin; it's test-only now.

`rehighlight_editor` also takes a tab whose path says nothing and whose first
line's `#!` now names the language, so a script detected by its shebang
highlights in place.

## The offline hint

An offline failure names the offline tarball with the language: its main
pack's `Tenmoku-language-pack-<pack>.tar.gz`, or `Tenmoku-language-pack.tar.gz`
for a language in no pack (`docs/release/language-pack.md`).

## Carried finding F5 (T1484): readers and commits

`Cache::load` and `Cache::usable_age` now hold the commit's lock file shared
for the whole read, main pair and kept pair alike, so a commit, which holds it
exclusively, can't complete between a reader's index and its signature, nor
remove the kept pair under it. The commit's own reads stay unlocked, as it
holds the exclusive lock. With no cache directory there's nothing to read and
no lock is taken.

A reader waits for the lock at most 2 s (`READ_LOCK_WAIT`, polling
`try_lock_shared`), then reads unlocked, as before, and logs it. A commit holds
the lock for milliseconds, so a longer hold is a launch stuck mid-commit, and a
blocking wait would hang the launch with it: T1484's
`a_commit_waits_for_another_launchs_lock` holds the lock as such a launch and
expects `load` to answer, and hung with a plain `lock_shared`. A reader that
times out is no worse off than before this card: while that commit hasn't
finished its kept pair is still there.
`grammar_registry::cache_read_tests::a_commit_waits_for_a_reader_between_its_two_files`
pauses a reader between the two files (`Cache::load_with`), checks the
exclusive lock can't be had, starts a commit of a newer serial on another
thread, sees it not finish, and then reads the old pair whole; the commit then
lands. It's in `grammar_registry.rs`, since `grammar_registry_tests.rs` isn't in
this card's scope.

## Tests

`crates/fe-workbench/tests/grammar_prompt_packs.rs` drives a real workbench with
fake offers. Toast text isn't public, so `Workbench::grammar_prompt_toasts`
(`#[doc(hidden)]`, as `Pane::find_label` is) gives the window's notifications
still open, from its inbox: each message and, for an offer, its buttons'
titles; buttons are clicked by their debug
selectors as `tests/grammar_prompt.rs` does.

`crates/fastedit/src/grammar_install_index_tests.rs` installs the JSON fixture
grammar from a made-up entry through the real loader, and checks the refusals
with a fake fetch over a map of URL to bytes.

## Scope

`crates/fe-workbench/tests/a11y_reach_chrome.rs` builds a `GrammarOffer` and
`GrammarOffers` literally, so it was added to the scope (respec) for the new
fields, empty. Its `toasts::space_presses_a_later_answer` also changed for the
new meaning of Never: the file it checks now names the language (`kotlin`),
not the extension (`kt`).

## Live run

`live_install_from_the_registry` (ignored: it needs the network) fetches the
published index and its signature through the real client under the app's
grants, verifies them with the committed key, and installs the smallest
language that isn't built in and claims files, through `fetch_file`
(`fe_net::grammar_fetch`) into a temporary extensions directory, where the real
loader registers it.

Run on 2026-11-16 at the lane head:

```text
$ cargo nextest run -p fastedit --run-ignored only live_install_from_the_registry --no-capture
    Starting 1 test across 31 binaries (386 tests skipped)
       START [         ] (1/1) fastedit::bin/tenmoku grammar_install_index_tests::live_install_from_the_registry
running 1 test
installed erb (9001 bytes, 5 files) from index serial 1794839167 to /var/folders/1q/qsg5wyqs56z3d808y1c2c3r80000gn/T/.tmpZZZkhS/extensions/tenmoku.grammar_erb; registered
test grammar_install_index_tests::live_install_from_the_registry ... ok
        PASS [   2.531s] (1/1) fastedit::bin/tenmoku grammar_install_index_tests::live_install_from_the_registry
     Summary [   2.549s] 1 test run: 1 passed, 386 skipped
```
