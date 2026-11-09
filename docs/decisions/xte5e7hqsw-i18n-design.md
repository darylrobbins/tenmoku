# UI string catalogue (i18n design)

## 1. Status

Status: **proposed**. Approver: the user (Daryl Robbins). Source card T1432,
written 2026-11-07, after the UI polish audit (T1429,
`docs/decisions/25w38k498b-ui-polish-audit.md`). Amended by T1506 to close
the three gaps T1432's review left open (section 11).

This card is a design only:

- No string was moved, keyed or translated.
- No file other than this doc changed. `deny.toml`, `Cargo.toml`,
  `Info.plist` and the scripts are untouched.
- No implementation card was filed. When this lands, the lead brings the doc
  to the user. The cards in [section 9](#9-migration-plan) are readied only
  after the user approves.

Approval has two parts:

1. The library and the shape of the API: sections 2–4.
2. The migration order: section 9.

**Decision:** status is `proposed`. Nothing is readied until the user approves
sections 2–4 and 9.

## Background: what is there today

Every user-facing string is an English Rust literal, or an English line in a
TOML, JSON or vimrc data file. Nothing is in a catalogue, and nothing selects a
locale. There is no NSLocale, preferredLanguages, CFLocale or `sys-locale`
call, and nothing reads `LANG`. `fe-git` forces `LC_ALL=C` on git (`cli.rs:67`)
so that it can match git's English output.

### Counts

The spec's count is `rg -o '"[A-Z][a-z][^"]{2,}"' <dir> --type rust -g
'!*tests*'`, run 2026-11-07 at 5686efb7. I then sorted each match by context
with a small script, kept in the card's scratchpad. The script drops:

- inline `#[cfg(test)] mod` blocks;
- comment and doc lines;
- log macros (`log::`, `tracing::`, `eprintln!`);
- `panic!`, `expect(`, `assert*` and attribute lines.

| Crate | rg total | Test modules | Comments | Log | Panic, expect, attribute | **UI candidates** |
|---|---:|---:|---:|---:|---:|---:|
| crates/fe-workbench/src | 1004 | 1 | 60 | 1 | 9 | **933** |
| crates/fe-vim/src | 192 | 6 | 5 | 0 | 38 | **143** |
| crates/fe-gpui/src | 112 | 6 | 6 | 0 | 0 | **100** |
| crates/fe-editor/src | 110 | 0 | 3 | 0 | 0 | **107** |
| crates/fe-config/src | 43 | 0 | 8 | 0 | 16 | **19** |
| crates/fe-chat-view/src | 30 | 2 | 0 | 0 | 0 | **28** |
| crates/fastedit/src | 16 | 2 | 0 | 0 | 0 | **14** |
| kiln/crates/kiln/src | 70 | 9 | 55 | 0 | 0 | **6** |
| kiln/crates/kiln-core/src | 49 | 0 | 6 | 0 | 4 | **39** |
| kiln/crates/kiln-shell/src | 37 | 0 | 10 | 0 | 0 | **27** |
| kiln/crates/kiln-platform/src | 21 | 2 | 0 | 0 | 0 | **19** |
| kiln/crates/kiln-plugin/src | 10 | 0 | 0 | 0 | 2 | **8** |
| kiln/crates/kiln-ui/src | 8 | 0 | 6 | 0 | 1 | **1** |
| kiln/crates/kiln-macros/src | 5 | 0 | 1 | 0 | 1 | **3** |
| kiln/crates/kiln-command/src | 7 | 0 | 0 | 0 | 7 | **0** |

**User-facing versus not.** I sampled 80 of the UI candidates at random, with a
fixed seed, across eight of these crates.

- **74 are user-facing:** titles, toasts, buttons, placeholders, prompts and
  status text.
- **6 are not:**
  - `debug_struct("Ledger")` and `"Text({:?} bytes)"`, both from `Debug` impls;
  - `"Application Support"`, a path component;
  - `"Unicode-3.0"`, a licence id;
  - `"Java"` and `"Dockerfile"`, language display names. These are proper
    nouns that stay as they are.

That is about 92% user-facing. In fe-workbench, about 860 of the 933 are UI
text. Ids are almost never capitalised (element ids are `"ai-changes-cancel"`,
command ids are `"git.unstage_file"`), so the regex rarely catches them.

**What the regex misses.** It undercounts the work, because it doesn't see:

- **Command titles taken from doc comments.**
  - `#[kiln::command]` takes a command's title from its first doc line
    (`kiln/crates/kiln-macros/src/command.rs:133–150`).
  - fe-workbench has **297** such commands, 246 of them in `workbench_commands.rs`.
  - Examples: `/// Git: Unstage File` over `#[command(id = "git.unstage_file")]`.
- **Lowercase text.**
  - fe-vim's which-key table: 81 entries such as `(ch('#'), "partial word backward")`, `which_key.rs:94`.
  - fragments such as `" ({chars} selected)"` and `"recording @{register}"`.
- **Data files:**
  - `crates/fe-keymap/menus/fastedit.toml`: 131 menu and item titles;
  - `kiln/crates/kiln/src/setup/menus.toml`: 12;
  - `crates/fe-config/settings.schema.json`: 249 descriptions, generated from the doc comments on `Settings`;
  - `crates/fe-config/settings-template.toml`: 321 comment lines;
  - `crates/fastedit/presets/lazyvim.vimrc`: 11 `fegroup` names.
- **Multi-line strings:** `crates/fastedit/src/cli.rs:10` `USAGE` is one literal of about 25 lines.

The other crates have little:

- fe-lsp 19, fe-syntax 17, fe-git 9, fe-markdown-view 6, fe-chat 6, fe-net 4,
  fe-nav 4, fe-chat-anthropic 3.
- Most of fe-syntax's are file and language names (`"Cargo.lock"`,
  `"Gemfile"`), which stay as they are.
- fe-git's are matchers on git's own stderr (`"Authentication failed"`,
  `remote.rs:79`), which must stay as they are.

All told, the migration moves about **2,300 messages**: about 2,050 literals
and doc titles, plus the 249 generated settings descriptions. Section 9 breaks
this down per card.

**Decision:** these counts are the baseline for the estimates in section 9. Each
migration card recounts its own files when it is readied.

## 2. Library

### Options measured

I built each option as a scratch crate:

- rustc 1.98.1 (this repo's toolchain);
- the workspace's release profile: `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`;
- a synthetic catalogue of 1,500 messages and 90 KB of FTL. 10% of the messages use a plural selector and 10% take a string argument.

Method:

- I ran `cargo tree -e normal,build --format '{p} | {l}'` and `cargo deny check licenses` against a copy of each deny file's `[licenses]` table. I used the repo's `deny.toml` and **also** `kiln/deny.toml`, because kiln keeps its own, shorter list: it has no MIT-0, 0BSD, BSL-1.0 or Unlicense.
- Each timing is `Instant` inside the binary: five to ten runs on this Mac.
- The Mac was busy (load average about 60), so treat the timings as ranges, not as `--perf` numbers.

| | **Fluent** (`fluent-bundle` 0.16.0 + `unic-langid` 0.9.6 + `fluent-langneg` 0.13.1) | `fluent-templates` 0.15.1 | `rust-i18n` 4.2.4 | `gettext-rs` 0.8.0 (GNU gettext) | `gettext` 0.4.0 (pure Rust `.mo` reader) |
|---|---|---|---|---|---|
| Crates pulled in (normal and build deps, all targets) | 20 | 45 | 71 | 22 | 9 |
| New to our Cargo.lock | 8: fluent-bundle, fluent-syntax, fluent-langneg, intl_pluralrules, intl-memoizer, type-map, unic-langid, unic-langid-impl | 13: Fluent's 8, fluent-templates and fluent-template-macros, plus unic-langid's macros (proc-macro-hack, unic-langid-macros, unic-langid-macros-impl), which it turns on. Its ignore, globset, flume and walkdir are already locked | 16, including serde-saphyr, granit-parser, globwalk, base62, triomphe, normpath and simdutf8 | 4: gettext-rs, gettext-sys, locale_config and temp-dir, **plus the GNU gettext 1.0 C sources** | 8: gettext plus `encoding` 0.2 and its six `encoding-index-*` tables |
| Passes `deny.toml` unchanged | yes | yes | yes | **yes, but cargo-deny is blind here**: see below | yes (the index tables are CC0-1.0, which is allowed) |
| Passes `kiln/deny.toml` unchanged | yes | yes | yes | yes, with the same blind spot | yes |
| Loading | embedded with `include_str!`, parsed at run time: **0.24–0.69 ms** to parse 1,500 messages into a bundle | embedded by a proc macro, parsed lazily on the first lookup: 0.37–1.18 ms | compiled into code by a proc macro: first `t!` in 0.14–0.44 ms | `.mo` files read at run time from a `bindtextdomain` directory | `.mo` read at run time |
| Formatting | 0.13–0.49 µs a message (1,500 in 0.19–0.73 ms) | as Fluent | not measured separately | C call | hash lookup |
| Plurals | CLDR cardinal and ordinal for every CLDR locale (`intl_pluralrules`), as selectors inside the message | as Fluent | **none**: `%{count}` interpolation only | `ngettext` with each `.po`'s `Plural-Forms` formula (not CLDR categories) | `ngettext`, as gettext |
| Gender and other selects | selectors on any string argument | as Fluent | none | none (msgctxt only) | none |
| Numbers and dates | numbers as plain `to_string`, with no grouping (`FluentNumber::as_string`); the `set_formatter` hook takes a custom formatter | as Fluent | none | none (C locale) | none |
| Bidi isolation of arguments | built in: FSI/PDI around each placeable, on by default (`set_use_isolating`) | as Fluent | none | none | none |
| Binary size over an empty binary that embeds the same 90 KB of text | **+236 KB** (674,672 against 438,608 bytes) | +271 KB | +36 KB | not built (it needs `./configure` of GNU gettext) | +0.2 KB with an empty catalogue |
| GPUI-free | yes | yes | yes | yes | yes |
| Licence of the crate itself | Apache-2.0 OR MIT | MIT/Apache-2.0 | MIT | MIT (the bundled C is LGPL) | MIT |

**Licence of every crate each option pulls in.** These lists are complete for
the versions and features chosen above. They come from `cargo tree -e
normal,build --target all --format '{p}|{l}'` on each scratch crate, so they
include build-time crates, proc macros and other platforms' crates. Crates
already in our Cargo.lock are listed with their licences too; † marks a crate
new to the lock. A fresh resolve picked a few newer patch releases than the
lock holds: smallvec 1.16.5 against 1.16.2, tinystr 0.8.5 against 0.8.4, syn
3.0.9 against 3.0.6, zerovec 0.11.9 against 0.11.8, yoke 0.8.4 against 0.8.3,
unicode-ident 1.0.27 against 1.0.26. I checked the locked versions' manifests;
each declares the same licence.

`cargo deny check licenses` passed for every option against a copy of each
deny file's `[licenses]` table, once with `deny.toml` and once with
`kiln/deny.toml`.

**Fluent (recommended: `fluent-bundle` 0.16.0, `unic-langid` 0.9.6 without `macros`, `fluent-langneg` 0.13.1)**: 20 crates, 8 new to the lock (†). Passes `deny.toml` and `kiln/deny.toml` unchanged.

- (MIT OR Apache-2.0) AND Unicode-3.0: unicode-ident 1.0.27
- Apache-2.0 OR GPL-2.0-only: self_cell 1.3.0
- Apache-2.0 OR MIT: fluent-bundle 0.16.0 †, fluent-langneg 0.13.1 †, fluent-syntax 0.12.0 †, intl-memoizer 0.5.3 †, rustc-hash 2.1.3
- Apache-2.0/MIT: intl_pluralrules 7.0.2 †
- MIT OR Apache-2.0: displaydoc 0.2.7, proc-macro2 1.0.107, quote 1.0.47, smallvec 1.16.5, syn 3.0.9, thiserror 2.0.21, thiserror-impl 2.0.21, unic-langid 0.9.6 †, unic-langid-impl 0.9.6 †
- MIT/Apache-2.0: type-map 0.5.1 †
- Unicode-3.0: tinystr 0.8.5
- Unlicense OR MIT: memchr 2.8.3

**`kiln-l10n`'s other dependencies (`inventory` 0.3, `unicode-bidi` 0.3, `gpui-pre-shared-string` 0.3.9)**: 23 crates, 0 new to the lock (†). Passes `deny.toml` and `kiln/deny.toml` unchanged.

- (MIT OR Apache-2.0) AND Unicode-3.0: unicode-ident 1.0.27
- Apache-2.0: gpui-pre-shared-string 0.3.9
- Apache-2.0 OR MIT: equivalent 1.0.3, indexmap 2.14.2
- MIT: schemars 1.2.3, zmij 1.0.23
- MIT OR Apache-2.0: dyn-clone 1.0.20, hashbrown 0.17.1, inventory 0.3.25, itoa 1.0.18, proc-macro2 1.0.107, quote 1.0.47, ref-cast 1.0.27, ref-cast-impl 1.0.27, rustversion 1.0.23, serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, serde_json 1.0.154, smol_str 0.3.7, syn 3.0.9, unicode-bidi 0.3.18
- Unlicense OR MIT: memchr 2.8.3

**ICU4X add-on (`icu_decimal` 2.3.0 with `icu_locale_core` 2.3.0)**: 26 crates, 3 new to the lock (†). Passes `deny.toml` and `kiln/deny.toml` unchanged.

- (MIT OR Apache-2.0) AND Unicode-3.0: unicode-ident 1.0.27
- MIT: synstructure 0.14.0
- MIT OR Apache-2.0: displaydoc 0.2.7, proc-macro2 1.0.107, quote 1.0.47, smallvec 1.16.5, stable_deref_trait 1.2.1, syn 3.0.9
- Unicode-3.0: fixed_decimal 0.7.3 †, icu_decimal 2.3.0 †, icu_decimal_data 2.3.0 †, icu_locale_core 2.3.0, icu_locale_fallback 2.3.0, icu_locale_fallback_data 2.3.0, icu_provider 2.3.1, litemap 0.8.3, potential_utf 0.1.6, tinystr 0.8.5, writeable 0.6.4, yoke 0.8.4, yoke-derive 0.8.4, zerofrom 0.1.8, zerofrom-derive 0.1.8, zerotrie 0.2.5, zerovec 0.11.9, zerovec-derive 0.11.7

**`fluent-templates` 0.15.1**: 45 crates, 13 new to the lock (†). Passes `deny.toml` and `kiln/deny.toml` unchanged.

- (MIT OR Apache-2.0) AND Unicode-3.0: unicode-ident 1.0.27
- Apache-2.0 OR GPL-2.0-only: self_cell 1.3.0
- Apache-2.0 OR MIT: fluent-bundle 0.16.0 †, fluent-langneg 0.13.1 †, fluent-syntax 0.12.0 †, intl-memoizer 0.5.3 †, rustc-hash 2.1.3
- Apache-2.0/MIT: flume 0.11.1, intl_pluralrules 7.0.2 †
- MIT: spin 0.9.9
- MIT OR Apache-2.0: bstr 1.13.1, crossbeam-deque 0.8.8, crossbeam-epoch 0.9.21, crossbeam-utils 0.8.23, displaydoc 0.2.7, lock_api 0.4.14, log 0.4.35, proc-macro-hack 0.5.20+deprecated †, proc-macro2 1.0.107, quote 1.0.47, regex-automata 0.4.18, regex-syntax 0.8.11, scopeguard 1.2.0, smallvec 1.16.5, syn 2.0.119, syn 3.0.9, thiserror 2.0.21, thiserror-impl 2.0.21, unic-langid 0.9.6 †, unic-langid-impl 0.9.6 †, unic-langid-macros 0.9.6 †, unic-langid-macros-impl 0.9.6 †, windows-link 0.2.1, windows-sys 0.61.2
- MIT/Apache-2.0: fluent-template-macros 0.15.1 †, fluent-templates 0.15.1 †, type-map 0.5.1 †
- Unicode-3.0: tinystr 0.8.5
- Unlicense OR MIT: aho-corasick 1.1.5, globset 0.4.21, ignore 0.4.34, memchr 2.8.3, winapi-util 0.1.11
- Unlicense/MIT: same-file 1.0.6, walkdir 2.5.0

**`rust-i18n` 4.2.4**: 71 crates, 16 new to the lock (†). Passes `deny.toml` and `kiln/deny.toml` unchanged.

- (Apache-2.0 OR MIT) AND BSD-3-Clause: encoding_rs 0.8.44
- (MIT OR Apache-2.0) AND Unicode-3.0: unicode-ident 1.0.27
- Apache-2.0 OR MIT: autocfg 1.5.1, equivalent 1.0.3, indexmap 2.14.2, multiversion_no_op 1.0.0 †, nohash-hasher 0.2.0 †
- MIT: base62 2.2.6 †, globwalk 0.8.1 †, rust-i18n 4.2.4 †, rust-i18n-macro 4.2.4 †, rust-i18n-support 4.2.4 †, winnow 0.7.15, zmij 1.0.23
- MIT OR Apache-2.0: annotate-snippets 0.12.17 †, anstyle 1.0.14, arc-swap 1.9.3, base64 0.23.1, bstr 1.13.1, cfg-if 1.0.5, crossbeam-deque 0.8.8, crossbeam-epoch 0.9.21, crossbeam-utils 0.8.23, either 1.19.0, encoding_rs_io 0.1.8, glob 0.3.4 †, granit-parser 1.3.0 †, hashbrown 0.17.1, itertools 0.11.0, itoa 1.0.18, log 0.4.35, normpath 1.5.2 †, num-traits 0.2.19, proc-macro2 1.0.107, quote 1.0.47, regex 1.13.1, regex-automata 0.4.18, regex-syntax 0.8.11, rustversion 1.0.23, scopeguard 1.2.0, serde 1.0.229, serde-saphyr 1.3.0 †, serde_core 1.0.229, serde_derive 1.0.229, serde_json 1.0.154, serde_spanned 0.6.9, simdutf8 0.1.5 †, siphasher 1.0.4, smallvec 1.16.5, stable_deref_trait 1.2.1, syn 2.0.119, syn 3.0.9, toml 0.8.23, toml_datetime 0.6.11, toml_edit 0.22.27, toml_write 0.1.2, triomphe 0.1.17 †, unicode-width 0.2.3, windows-link 0.2.1, windows-sys 0.61.2
- MIT/Apache-2.0: arraydeque 0.5.1 †, bitflags 1.3.2, core_detect 1.0.0 †
- Unlicense OR MIT: aho-corasick 1.1.5, globset 0.4.21, ignore 0.4.34, memchr 2.8.3, winapi-util 0.1.11
- Unlicense/MIT: same-file 1.0.6, walkdir 2.5.0
- Zlib: foldhash 0.2.0

**`gettext-rs` 0.8.0**: 22 crates, 4 new to the lock (†). Passes `deny.toml` and `kiln/deny.toml` unchanged.

- Apache-2.0: temp-dir 0.1.16 †
- MIT: block 0.1.6, gettext-rs 0.8.0 †, gettext-sys 0.27.0 †, locale_config 0.3.0 †, malloc_buf 0.0.6, objc 0.2.7, objc-foundation 0.1.1, objc_id 0.1.1
- MIT OR Apache-2.0: cc 1.11.1, find-msvc-tools 0.1.15, lazy_static 1.5.1, libc 0.2.191, regex 1.13.1, regex-automata 0.4.18, regex-syntax 0.8.11, shlex 2.0.1
- MIT/Apache-2.0: winapi 0.3.9, winapi-i686-pc-windows-gnu 0.4.0, winapi-x86_64-pc-windows-gnu 0.4.0
- Unlicense OR MIT: aho-corasick 1.1.5, memchr 2.8.3

**`gettext` 0.4.0**: 9 crates, 8 new to the lock (†). Passes `deny.toml` and `kiln/deny.toml` unchanged.

- CC0-1.0: encoding-index-japanese 1.20141219.5 †, encoding-index-korean 1.20141219.5 †, encoding-index-simpchinese 1.20141219.5 †, encoding-index-singlebyte 1.20141219.5 †, encoding-index-tradchinese 1.20141219.5 †, encoding_index_tests 0.1.4 †
- MIT: encoding 0.2.33 †, gettext 0.4.0 †
- Unlicense OR MIT: byteorder 1.5.0

**GNU gettext's libintl** has no crate, so it isn't in these lists. gettext-sys
builds it from its bundled C sources, under LGPL-2.1-or-later (below), and no
deny file can see it.

No option needs `deny.toml` widened. Widening it is the one human path in
regatta.toml.

### What GNU gettext's LGPL means here

`gettext-sys` 0.27 bundles `gettext-1.0.tar.xz`, a 10 MB archive. On macOS it
always builds that archive: there is no system libintl, and `gettext-system`
only finds Homebrew's. The build runs `./configure` and links
`cargo:rustc-link-lib=static=intl` (`build.rs:282, 350`).

LGPL-2.1 on a statically linked library obliges us to let users relink
Tenmoku against a modified libintl. That means one of two things:

- ship our object files;
- ship `libintl.dylib` inside the bundle as a separately signed and notarised
  library, with its source offer and notice in `THIRD_PARTY.md`.

The same obligation would pass to every kiln app, and kiln is meant to be
Apache-2.0 with nothing attached. The owner's rule
(regatta.toml, 2026-10-05) is that a licence which could stop Tenmoku being
sold needs a person. LGPL doesn't stop selling. But cargo-deny can't see it,
so the gate would let it through silently, which is worse.

GNU gettext also brings process-global state: `setlocale`, `textdomain`, and
`LC_NUMERIC` changing C's number parsing under GPUI and the font stack. Its
extraction tools (`xgettext`, `msgfmt`) are GPL build-time tools.

**Rejected.**

The pure-Rust `gettext` crate avoids the LGPL. But:

- its last release was 2019, and it drags in `encoding` 0.2, a crate that is no
  longer maintained;
- it has gettext's weaknesses: the English text is the key, plurals are a
  formula per language, and there is no selector or isolation.

**Rejected.**

### Why not rust-i18n or fluent-templates

**rust-i18n** has the smallest code (+36 KB) and compile-time embedding, but
**no plural rules**. Every count would show "1 files". Most of its 15 new crates
(a YAML parser, globbing) serve its proc macro at compile time. It also has no isolation.

**fluent-templates** is Fluent with a loader:

- It parses every locale lazily on the first lookup (0.37–1.18 ms on the main
  thread).
- It adds two crates and keeps `ignore` and `flume` as run-time dependencies.
- It fixes the layout (one `locales/` directory).

We need two layers: kiln's strings and the app's. Kiln may not know the app's
files exist. That layering is about 200 lines over `fluent-bundle`, so the
loader isn't worth its rigidity.

### ICU4X

Already in the lock, through `icu_collator` 2.3 in `fe-editor/src/line_sort.rs`
and `icu_properties` in `case_transform.rs`:

- icu_collator, icu_collator_data;
- icu_collections, icu_normalizer(_data), icu_properties(_data);
- icu_locale_core, icu_locale_fallback(_data), icu_provider;
- tinystr, zerovec, yoke, litemap, writeable, potential_utf.

Not in it: `icu_plurals`, `icu_decimal`, `icu_datetime`.

Fluent brings its own CLDR plural rules (`intl_pluralrules`), so `icu_plurals`
isn't needed. `icu_decimal` 2.3 plus `fixed_decimal`, three new Unicode-3.0
crates, added +85 KB with `icu_plurals` in the scratch build. It formats
1234567 as `1.234.567` for `de`. It plugs into Fluent through
`FluentBundle::set_formatter`. It isn't needed until a shipped locale groups
digits differently from en-US, which shows `1234` today.

### Startup and size against PERF.md

- **Startup.**
  - PERF.md § Startup puts `main` at 7.4 ms and the window and renderer at
    142 ms. The 1M-line target is "interactive in < 500 ms".
  - Parsing en-US costs 0.24–0.69 ms for 1,500 messages, so about 0.4–1 ms for
    the real 2,300. A second locale adds the same again.
  - Done on a thread spawned at the top of `main`, beside T8's file read, and
    joined at the app callback (60 ms), it costs nothing on the critical path.
- **Size.** About +240 KB of code and about +130 KB of text a locale, against a
  binary of tens of MB.

### Headless crates and kiln's boundary

**Headless crates.** fe-vim, fe-editor and fe-config must stay GPUI-free
(`scripts/check-no-gpui.sh` checks every crate outside `UI_CRATES`).

- `fluent-bundle` is GPUI-free.
- The thin layer over it must be GPUI-free too. It returns
  `gpui::SharedString` from `gpui-pre-shared-string` 0.3.9, which is already in
  the lock. That crate is GPUI-free and Apache-2.0, has an
  `impl From<Arc<str>> for SharedString`, and is among the utility crates the
  script's regex allows.

**Kiln's boundary.** kiln may not name fastedit
(`scripts/check_kiln_boundary.py`), and its crates must be Apache-2.0.

- So the layer lives in kiln as a new crate, **`kiln-l10n`**. It is GPUI-free
  and depends only on Fluent's crates, `inventory` (which kiln already uses
  for command records), `gpui-pre-shared-string` and `unicode-bidi`. The last
  three are already in the lock.
- kiln's crates and fastedit's crates each register their own resources with
  it. Neither names the other.
- The host supplies the locale, not a lookup function.
- A host-supplied lookup, `Fn(&str) -> String`, would have to be threaded
  through every kiln widget. It also couldn't reach headless crates, so the
  shared catalogue is simpler.

**Decision:** Mozilla Fluent through `fluent-bundle` 0.16. It brings
`unic-langid` 0.9 without the `macros` feature and `fluent-langneg` 0.13.1,
eight new crates that are all Apache-2.0/MIT and pass both deny files
unchanged. A new GPUI-free kiln crate, `kiln-l10n`, wraps it. `icu_decimal` is
added through `set_formatter` only when a launch locale needs grouping.
gettext, rust-i18n and fluent-templates are rejected.

## 3. Catalogue layout

**Where the files live.** Each crate owns its strings, next to its code, so a
card's scope globs stay inside the crate it touches:

```text
crates/fe-workbench/l10n/en-US/workbench.ftl
crates/fe-workbench/l10n/en-US/git.ftl        # a crate may split by area
crates/fe-vim/l10n/en-US/vim.ftl
kiln/crates/kiln-shell/l10n/en-US/kiln-shell.ftl
kiln/crates/kiln-shell/l10n/de/kiln-shell.ftl # a translation, later
```

- **Directory names are BCP 47 tags** (`en-US`, `pt-BR`, `zh-Hans`), as Fluent
  and `unic-langid` parse them. The bundle step maps them to Apple's `.lproj`
  names (section 7).
- **Keys** are Fluent identifiers prefixed by the crate's short name, so no two
  crates collide:
  - `workbench-git-switch-failed`;
  - `vim-e492`;
  - `kiln-shell-picker-delete-forward`;
  - `editor-…`, `gpui-…`, `config-…`, `keymap-…`, `fastedit-…`.
- **Command titles** use `command-<id>`, with the id's `.` written as `-`. So
  `command-git-unstage_file` belongs to the crate that registers the command.
- **Settings descriptions** use `settings-<dotted path with - for .>`, for
  example `settings-workbench-preview_tabs`.
- **Template comments** use `settings-template-…`.
- Both come from one function, `setting_key` (section 4).

**How they are embedded.** Each crate lists its files in one macro call:

```rust
// crates/fe-workbench/src/l10n.rs
kiln_l10n::resources! {
    "en-US" => ["../l10n/en-US/workbench.ftl", "../l10n/en-US/git.ftl"],
}
```

- The macro is a `macro_rules!` in `kiln-l10n`. It expands to `include_str!`
  of each file, so cargo rebuilds when a file changes, plus an
  `inventory::submit!` of the crate's `Resources`.
- There is no build.rs and no proc macro.
- A test in the crate (section 8) fails if a `.ftl` on disk isn't listed.

**How resources are loaded.** `kiln_l10n` builds one catalogue for the process:

- The catalogue is lazy. It is built on the first lookup from every
  `Resources` that `inventory` collected in the binary. That covers all of the
  app's crates and kiln's.
- It holds one `fluent_bundle::concurrent::FluentBundle` per locale in the
  negotiated chain. `concurrent` is `Send + Sync`, so the background warm-up
  and lookups from any thread work.
- **Each bundle holds its own locale's resources only**, from every crate.
  en-US's resources are never added to another locale's bundle. Fluent
  resolves a nested message, term or attribute reference only inside the
  bundle that formats the message. So a translated message's references must
  resolve in its own locale, and section 8 checks exactly that.
- It sits behind a `RwLock<Arc<Catalogue>>`.
- `inventory` records are per binary, so test binaries see only the crates
  they link. fe-vim's unit tests find fe-vim's strings without any host.

**Fallback.**

- **Each lookup walks the chain:** the negotiated locales, then `en-US`. A
  message missing in `de` comes from `en-US`.
- **A key missing from en-US too is a bug.** The tests catch it (section 8).
  At run time it shows the key itself, `workbench-git-pull`. It logs one
  `log::warn!` for each key, deduplicated by a `HashSet`, and never panics.
- **A formatting error is logged once per key.** Examples are a missing
  argument or a cyclic reference. Fluent shows the placeable as `{$name}`.
- **A locale with no resources is dropped from the chain.** Asking for `fr`
  when nothing ships `fr` leaves the system's other preferences, then `en-US`.
  It logs once and raises a settings diagnostic that names the language
  actually chosen (section 6).
- **Duplicate keys across resources are errors.** `add_resource` returns them,
  and a test in `crates/fastedit` loads every resource the app links, so they
  fail CI. At run time the first one wins and the error is logged once.

**Caching.** A frame may resolve a few hundred labels. At about 0.2–0.5 µs a
message that is about 0.1 ms a frame. So the catalogue caches messages that
take no arguments as a `SharedString` per key, then about 20 ns per lookup.
Each locale change bumps a generation and clears the cache.

**Decision:** one catalogue for each crate, at `<crate>/l10n/<bcp47>/<name>.ftl`.
Files are embedded by an explicit `kiln_l10n::resources!` list
(`include_str!`) and collected through `inventory` into one lazy process
catalogue, with no build.rs. Keys are prefixed by crate. A lookup falls back
per message to `en-US`. A key or locale that is still missing shows the key
and logs once, never a panic.

## 4. Fetching strings per surface

The API, all in `kiln-l10n` and all GPUI-free:

```rust
/// A message now, in the active locale: a cached SharedString for a bare key.
tr!("workbench-git-pull")                     // -> SharedString
tr!("workbench-git-switch-failed", name = branch)

/// A message resolved later (at render, menu build, palette open): a key and
/// its arguments, or verbatim text from a user or a plugin.
pub enum Text { Key(Cow<'static, str>, Args), Verbatim(SharedString) }
impl Text { pub fn resolve(&self) -> SharedString }
text!("workbench-nav-definitions")            // -> Text
Text::verbatim(plugin_title)                  // -> Text

/// Text that is deliberately not translated (ids, file names, proper nouns):
/// the lint's in-code allow marker; expands to the literal.
verbatim!("Cargo.toml")
```

**Rule of thumb.** A string that is shown once and thrown away, such as a
toast or a dialog, resolves at once with `tr!`. A string that is stored and
shown again stores a `Text`, or something that names its key, and resolves when
it is drawn:

- menu items carry a `title_key`;
- picker placeholders and button labels store a `Text`;
- command titles derive their key from the command id through `label()`.

Text that is rebuilt on every render, such as status items, just takes `tr!`
output.

### GPUI views and kiln widgets

Views call `tr!` inside `render`. A locale change re-renders every window, and
each render looks the strings up again, so nothing caches a stale string.

```rust
// before: crates/fe-workbench/src/shortcuts_editor.rs:933
.child(heading("Keybinding").flex_none().w(KEYS_WIDTH))
// after
.child(heading(tr!("workbench-shortcuts-column-keybinding")).flex_none().w(KEYS_WIDTH))
```

**Stored text.** Two constructors that store a label change their parameter
to `impl Into<Text>`:

- `Picker::new`'s placeholder, `&str` today (`kiln-shell/src/picker.rs:160`,
  64 call sites);
- `Button::new`'s label, `impl Into<SharedString>` today
  (`kiln-ui/src/controls.rs:102`, 81 call sites).

The transitional `From` impls on `Text` keep every call site compiling:
`&str`, `&String`, `String`, `SharedString` and `Cow<str>` all convert to
`Text::Verbatim`.

**Rebuilt text stays a `SharedString`.** Struct fields that are rebuilt on
every render keep their `SharedString` type and are filled with `tr!` output:

- `StatusItem.text` and `.tooltip` (`kiln-shell/src/status.rs:34`), which
  fe-workbench builds in `StatusBar::items` (`status_bar.rs:193`) each time
  the bar draws.

Changing those field types would break every struct literal for no gain.

```rust
// before: crates/fe-workbench/src/nav.rs:190
Picker::new(delegate, "Definitions", fe_ui::ui_theme(cx), cx)
// after
Picker::new(delegate, text!("workbench-nav-definitions"), fe_ui::ui_theme(cx), cx)
```

**On a locale change**, kiln-platform takes these steps (section 6):

1. calls `kiln_l10n::set_locales`;
2. sets an `ActiveLocale` GPUI global;
3. calls `menus::refresh(cx)`;
4. calls `cx.refresh_windows()`.

This is the path `keys::set_keymap` already takes
(`kiln-platform/src/keys.rs:313–321`). Views that compute text outside
`render` follow the global with `cx.observe_global::<ActiveLocale>`, as they
follow `ActiveUiTheme` today (`markdown_preview.rs:180`).

### Native menus (kiln-platform) and kiln's drawn menu bar

**Bundled menu files** name a key in `title_key` instead of a literal `title`.
User menu files keep literal `title`s.

```toml
# before: crates/fe-keymap/menus/fastedit.toml
[[menus.items]]
title = "Hide Tenmoku"
command = "app.hide"
# after
[[menus.items]]
title_key = "keymap-menu-hide-app"   # en-US: keymap-menu-hide-app = Hide { $app }
command = "app.hide"
```

**Item titles.**

- `CommandItem` (`kiln-core/src/menu.rs:88–103`) gains
  `title_key: Option<String>`, and `title: String` stays as it is. A bundled
  item's `title` is filled with the key's en-US text at parse time, so every
  reader of `title` keeps working.
- **The field breaks three struct literals**, so K1's scope includes them:
  - `kiln-core/src/menu.rs:635`;
  - `kiln/src/contributions.rs:1739`;
  - `kiln/src/contributions.rs:1877`.

  No field is `#[deprecated]`, because the `vet` stage runs clippy with
  `-D warnings`.
- `CommandItem::label(&self) -> Text` returns `Text::Key` for a keyed item and
  `Text::Verbatim(title)` otherwise.
- **`SystemItem`, the OS-filled submenu such as Services**
  (`kiln-core/src/menu.rs:113`), is keyed the same way.
  - It gets the same `title_key: Option<String>` and `label()`.
  - Its built-in default, `"Services"` at `menu.rs:601`, becomes the kiln key
    `kiln-core-menu-services`.
  - `menus.rs:145` passes `system.label().resolve()` to
    `MenuItem::os_submenu`.
  - Its one struct literal (`menu.rs:600`) is in K1's scope.
  - K1 adds a fake-locale test that the bundled Services item, and a keyed
    command item, come out in the fake locale in the built GPUI menus and in
    the drawn bar.
- kiln's `{app}` substitution (`defaults.rs:94–106`) becomes a `$app` argument
  that kiln passes to every menu title.
- `menus::refresh` resolves the labels where it builds GPUI's menus today:

  ```rust
  // before: kiln/crates/kiln-platform/src/menus.rs:133–145
  MenuItem::action(command.title.clone(), action)
  // after
  MenuItem::action(command.label().resolve(), action)
  ```

- The same applies to `kiln-shell/src/menu_bar.rs:329`.

**Menu names are also ids today.** `MenuBar::menu(name)` and
`#[kiln::command(menu = "File")]` look menus up by their display names, so a
menu gets a stable `id` (`file`) beside its `title_key`.

**Rebuilding.** Menus rebuild on a locale change through `menus::refresh`, the
same path a keymap change takes.

### Command palette titles and categories (kiln-command)

**Titles.** A command's title is the key `command-<id>`, derived from its id.
The doc comment stays as developer documentation.

```rust
// before: crates/fe-workbench/src/workbench_commands.rs:452–454
/// Git: Unstage File
#[command(id = "git.unstage_file")]
fn git_unstage_file(&mut self, cx: &mut Cx<Self>) {
// after: unchanged source; the title is git.ftl's
//   command-git-unstage_file = Git: Unstage File
```

```rust
// before: crates/fe-editor/src/column_select.rs:39–50
("column_select_left", "Column Select Left", Direction::Left),
// after: the title literal goes; editor.ftl has
//   command-column_select_left = Column Select Left
("column_select_left", Direction::Left),
```

**One command-label resolver, in kiln-command.** Four representations carry a
command's English title today, and each keeps it as the fallback:

- `CommandMeta` (`kiln-command/src/registry.rs:70–82`), the registry's;
- `TypedCommand` (`kiln-platform/src/command_actions.rs:71–80`), from the
  `#[kiln::command]` macro's `CommandRecord`s (`kiln/src/contributions.rs:295`)
  and held in the `TypedCommands` global (`command_actions.rs:89, 128–140`).
  Most of fe-workbench's commands are macro-declared, so this is most command
  text;
- `CommandSpec` (`kiln/src/contributions.rs:162`), kiln's contribution, which
  feeds the menu items `#[kiln::command(menu = …)]` places;
- fe-keymap's `CommandInfo` (`fe-keymap/src/shortcuts_model.rs:17`), the
  shortcuts editor's row source.

kiln-platform's own `CommandInfo` (`command_actions.rs:57`) is only what
`register_command` takes; its `title` becomes the `TypedCommand`'s fallback.

All of them resolve through one GPUI-free module, `kiln-command/src/l10n.rs`.
It takes a plain `&str` id, so a type that holds its id as a `String`, as
fe-keymap's `CommandInfo` does, uses it too:

```rust
// kiln/crates/kiln-command/src/l10n.rs
/// `command-git-unstage_file` for `git.unstage_file`: the id's `.` as `-`.
pub fn command_key(id: &str) -> String;
/// The title to show for `id`, in the active locale, in this order:
/// 1. the message `command-<id>`;
/// 2. the label kiln derived for `id` (`derive_label`), resolved now;
/// 3. `fallback`, the English title the command was registered with.
/// A miss is silent, with no log: plugin commands have no key by design.
pub fn command_label(id: &str, fallback: &str) -> SharedString;
/// `command-<id>`'s `.description` attribute, else `fallback`.
pub fn command_detail(id: &str, fallback: &str) -> SharedString;
/// Labels `id`, a command whose title is built from other text, such as a
/// panel's toggle, when it has no `command-<id>` of its own. Process-wide,
/// behind a `RwLock`, as the catalogue is.
pub fn derive_label(id: &str, label: Text);
```

`SharedString` is the type `tr!` returns. Each representation gets a
`label()` method that calls it, so no reader formats a key itself:

| Type | Method | Card |
|---|---|---|
| `CommandMeta` | `label()` = `command_label(id, &title)`; `detail()` = `command_detail(id, &description)` | I3 |
| `TypedCommand` | `label()` = `command_label(id, &title)` | I3 |
| `CommandSpec` | `label()` = `command_label(id, &title)` | I3 |
| fe-keymap `CommandInfo` | none: the shortcuts editor builds it already resolved, `title: typed.label().into()`, so fe-keymap doesn't change | W1 |

**No struct changes.** Every `title` field stays a `String` and stays the
English fallback, and `register(id, title, …)`, `register_command` and the
macro keep their signatures. So no construction site changes:

- `CommandMeta` is built by struct literal in five places:
  - `registry.rs:238`;
  - `kiln-plugin/src/commands.rs:585`;
  - `fe-workbench/src/markdown_preview.rs:74`;
  - `kiln-shell/tests/command_palette.rs:108`;
  - `fe-gpui/src/vim/which_key_tests.rs:299` and
    `which_key_chord_tests.rs:63`.
- `CommandSpec` by struct literal at `contributions.rs:332` and `:1070`, and in
  `kiln/tests/modules.rs:724` and `macro_command.rs:324, 328`.

**Every consumer moves to `label()`,** each in the card that owns its file, and
each resolves at the moment it shows the title:

| Consumer | Reads today | When it resolves | On a locale change | Card |
|---|---|---|---|---|
| The palette, `commands_in` (`kiln-shell/src/command_palette.rs:85–112`) | `typed.title` at `:93`, `meta.title` at `:98` | When it opens; it sorts and fuzzy-matches on the resolved text, so matching works in the user's language | The next open lists the new language; an open palette keeps its list, as an open toast does | I3 |
| Menu items from a contributed menu table, `table_menu_bar` (`kiln/src/contributions.rs:1737`) | `typed.title` | When kiln builds the menu bar, in `shipped` (`:1378`) | kiln observes I2's `ActiveLocale` and calls `resolve_again` (`:1566`), the path an extension's menu change takes: it rebuilds the bar and `menus::install`s it | I3 |
| Menu items `#[kiln::command(menu = …)]` places, `place_items` (`:1879`) | `CommandSpec.title` | The same | The same | I3 |
| Which-key's `command_title` (`fe-gpui/src/vim/which_key.rs:564–583`) | `meta.title` at `:568–570`, `typed.title` at `:580` | At every popup render | Nothing to rebuild: the next render resolves again | G1; Z1 checks it |
| The keymaps picker's `titles` (`fe-workbench/src/keymaps_picker.rs:81–98`) | `typed.title` at `:85`, `meta.title` at `:93` | When it opens (`show_keymaps`, `:408`) | The next open | W1 |
| The shortcuts editor's `commands` (`fe-workbench/src/shortcuts_editor.rs:1269–1290`) | `typed.title` at `:1276`, `meta.title` at `:1288`, into fe-keymap's `CommandInfo` | When it builds its `ShortcutsModel` (`build_model`, `:1015`) | W1 adds an `ActiveLocale` observer beside its `ChangedFiles` one (`:195`) that rebuilds the model as `follow_keymap` does (`:228–236`), so an open editor follows | W1 |

A reader that hasn't moved yet shows the registered English title, which is
harmless while only en-US ships. Z1 checks that none is left (section 9).

**Toggle titles.** `format!("Toggle {}", …)` builds a panel toggle's title
twice: as a `CommandSpec` for its View menu item (`contributions.rs:1072`) and
as a registered `CommandMeta` (`:1507`). I3 makes it the message
`kiln-command-toggle = Toggle { $title }`:

- where kiln registers the toggle, it calls
  `derive_label(id, text!("kiln-command-toggle", title = panel_title))`;
- both stored titles become `tr!("kiln-command-toggle", title = panel_title)`,
  a fallback `label()` doesn't reach while the derived label exists;
- so the palette, the menu and which-key all show the toggle in the active
  locale through `label()`, step 2 of `command_label`.

The panel's own title is a verbatim argument until K2 keys the dock headers
(P13).

**I3's test** (`kiln/crates/kiln/tests/l10n_command_labels.rs`): one
`#[kiln::command(menu = "File")]` command and one plugin-style registry command
with no key. Under a fake locale (below), the macro command's key resolves in
`commands_in` and in `cx.get_menus()`, first through `place_items`, then
through a contributed menu table (`Contributions::set_menu_table`). The
keyless one shows its registered title in both, with nothing logged.

**The cross-surface test, in Z1.** Once every consumer has moved, one test
follows one macro-declared command through all five surfaces:

- **Where.** `crates/fe-workbench/tests/l10n_command_labels.rs`, the only test
  in its binary. The catalogue's locale is process-wide (section 3), so no
  other test may run beside it.
- **The command.** The test file declares
  `#[kiln::command(context = "workbench", menu = "File")]` on a probe type,
  with one command, `/// Probe Labels`, `#[command(id = "workbench.probe_labels")]`.
  The macro records it for that binary.
- **The fake locale.** The test's own `kiln_l10n::resources!` lists
  `tests/fixtures/l10n/en-US/probe.ftl` with
  `command-workbench-probe_labels = Probe Labels`, and
  `tests/fixtures/l10n/qps-ploc/probe.ftl` with
  `command-workbench-probe_labels = ⟦Pröbe Läbels⟧`. These fixtures sit
  outside `l10n/`, so the root catalogue tests don't read them.
- **Set-up.** A workbench with the app's menus installed through kiln's app
  builder, `<leader>p` mapped to the command, and the shortcuts editor open.
- **Steps.**
  1. In en-US every surface shows `Probe Labels`.
  2. I2's `set_locale` switches to `qps-ploc`.
  3. Each surface shows `⟦Pröbe Läbels⟧`:
     - the palette, through `commands_in`, sorted by the marker;
     - the File menu, through `cx.get_menus()`, after the update's
       `resolve_again`;
     - which-key, through `command_title`, which G1 re-exports from fe-gpui
       for this;
     - the keymaps picker, opened after the switch;
     - the shortcuts editor, still open from before the switch, with no
       reopen.
  4. Switching back to en-US shows `Probe Labels` everywhere again, so no
     surface kept a stale resolved title.

**During the migration** a command whose key is missing falls back to its
registered literal or doc title, so crates migrate one at a time. Once a crate
is migrated, its lint (section 8) requires every command to have a key.

**Categories.** "Git:" is baked into the title today. There is no category
field, and the title message keeps it there, so translators can order it. A
separate `category` is not added here.

### Settings descriptions (settings.schema.json, settings-template.toml)

`settings.schema.json` is `schema_for!(Settings)` (`settings.rs:1236`). Its
descriptions are the doc comments, and a golden test guards it
(`src/tests/settings.rs:196–212`). So the doc comments stay the single source
of the en-US text.

**New golden.** A second golden test generates
`crates/fe-config/l10n/en-US/settings.ftl` from the schema and fails on drift.
`UPDATE_GOLDEN=1` regenerates both. Translators translate `settings.ftl`.

**One key function.** The generator, the settings UI and the checks share one
conversion, `fe_config::l10n::setting_key(kind, path)`:

- It joins `settings`, the kind and the dotted path, with each `.` written as `-`.
- `setting_key(Description, "workbench.preview_tabs")` is
  `settings-workbench-preview_tabs`.
- `setting_key(Template, "workbench.preview_tabs")` is
  `settings-template-workbench-preview_tabs`.

The mapping can't collide: all 134 property names in today's schema are
snake_case, none contains `-` or `.`, and the generator rejects one that does.

**The settings UI** looks up its description key and falls back to the
schema's description (`settings_model.rs:604`). Group titles are made from key
names today (`settings_model.rs:617–625`); they become keys
`settings-group-<name>`.

```rust
// before: crates/fe-config/src/settings_model.rs (description from the schema)
fn description(&self) -> Option<&str> { self.schema.description.as_deref() }
// after
fn description(&self) -> SharedString {
    let key = setting_key(KeyKind::Description, self.key_path()); // dots become -
    kiln_l10n::tr_or(&key, self.schema.description())
}
```

**`settings-template.toml` is keyed too.** The template is hand-written
(`fe-config/src/lib.rs:65`, `USER_SETTINGS_TEMPLATE`). Its comments paraphrase
the doc comments rather than repeat them.

**Five production paths write it** into a new or empty user settings file:

- `fe-workbench/src/persist.rs:65`;
- `fe-workbench/src/open.rs:439` (Open Settings);
- `fe-config/src/settings_edit.rs:113–116`;
- `fe-config/src/write.rs:16, 23, 34` (`write_setting`, `write_setting_str`,
  `write_settings`).

**Every `#` line is one of two kinds:**

- **Commented TOML.** The line is a comment whose remainder, after `# `,
  parses as a TOML table header, an array-table header or a `key = value`
  line. Examples:
  - `# font_size = 13.0`;
  - `# [[vim.remap]]` (`settings-template.toml:238`) and the example lines
    after it, `# mode = "n"`, `# from = "<leader>w"`, `# to = ":w<CR>"`;
  - `# [[vim.which_key_group]]` (`:247`);
  - `# [formatters.typescript]`, `# [formatters.rust]`, `# [formatters.python]`
    and `# [formatters.go]` (`:363–373`).

  These are never translated and are kept byte for byte, in order.
- **Prose:** any other comment line. A run of prose lines is a **block**. Only
  blocks are translatable.

**Keys.** A block's key comes from the TOML context it sits in:

- the active `[table]` line;
- or a commented table header seen since then, which opens its own context
  until the next header of either kind.

So:

- the intro is `settings-template-intro`;
- a block directly before a table header, active or commented, is that
  table's: `settings-template-vim-remap` for the block before
  `# [[vim.remap]]`;
- a block directly before a `key = value` line, active or commented, is that
  key's: `setting_key(Template, "editor.font_size")`, or
  `settings-template-vim-remap-mode` inside the commented `[[vim.remap]]`
  example;
- a dynamic segment such as `formatters.<language>` keys by the example's
  table: `settings-template-formatters-typescript` for the block before it.

A block followed by neither kind keys by its position:
`settings-template-<table>-note-<n>`. The generator errors on two blocks
mapping to one key.

**The file stays the en-US source.**

- A golden test generates `crates/fe-config/l10n/en-US/settings-template.ftl`
  from it, one message per block, and fails on drift. `UPDATE_GOLDEN=1`
  regenerates it, as the schema golden does.
- `setting_key` is shared with the schema generator, so a key and a
  description can't drift apart in their naming.

**Rendering.**

- `fe_config::user_settings_template() -> &'static str` renders the template
  in the active locale. It keeps every commented-TOML and active line, and
  replaces each block's lines with that locale's message, wrapped at 80
  columns and prefixed `# `.
- It renders once per locale and keeps the result (leaked: a handful of
  locales at most). So the existing `&'static str` signatures,
  `settings_edit.rs:113` and the `kiln_core::write_setting*` template
  parameters, don't change.
- For en-US it returns the constant itself, byte for byte, so
  `template_is_all_defaults` and `commented_out_with_defaults`
  (`tests/template.rs`) are unchanged.
- **The toggle writer resolves its template when it creates the file.** Today
  `persist.rs:65` hands `USER_SETTINGS_TEMPLATE` to
  `kiln_shell::persist::setting_writer` (`kiln-shell/src/persist.rs:170–178`),
  which captures the `&'static str` in its writer closure at startup. Handing
  it the rendered template the same way would keep the startup locale's
  template after a live locale change. So C1 changes the writer to take a
  host-owned provider instead of a string:

  ```rust
  // before: kiln/crates/kiln-shell/src/persist.rs:170–178
  pub fn setting_writer(path: SettingsPath, template: &'static str, cx: &App)
      -> Writer<SettingKey, bool>
  // after
  pub fn setting_writer(
      path: SettingsPath,
      template: impl Fn() -> &'static str + Send + Sync + 'static,
      cx: &App,
  ) -> Writer<SettingKey, bool> {
      Writer::new(cx, move |key: &SettingKey, value| {
          let path = path.get();
          // Called per write, at write time, never at startup.
          write_setting(&path, key, value, template());
      })
  }
  ```

  ```rust
  // before: crates/fe-workbench/src/persist.rs:65
  setting_writer(target.clone(), fe_config::USER_SETTINGS_TEMPLATE, cx)
  // after
  setting_writer(target.clone(), fe_config::user_settings_template, cx)
  ```

  - **When it resolves.** `setting_writer` never calls the provider. Each
    write calls it on the background executor, just before
    `kiln_core::write_setting`. That reads the template only when it creates
    the file, because the file is missing or empty (`kiln-core/src/write.rs:162–181`).
    So a created file gets the template of the locale active at that moment.
    On a write that edits an existing file the call is a cache lookup whose
    result goes unused.
    A write that was queued before a locale change and runs after it gets
    the new locale, which is the one the user now sees.
  - **Why it is safe off the main thread.** `user_settings_template` reads
    `kiln_l10n`'s process catalogue, a `RwLock` over `concurrent` bundles
    (section 3), never a GPUI global. Its per-locale cache makes each call a
    lookup.
  - **Why a closure, not a key.** kiln-shell can't name fe-config, and kiln
    must not know about the template's messages. The host owns the closure,
    and kiln only calls it.
  - The other four paths already call `user_settings_template()` at the
    moment they write, so they need nothing more.
- **All five paths call it** instead of `USER_SETTINGS_TEMPLATE`. The constant
  stays for tests.

**Consistency checks**, all in C1:

- every block has a key, and every key except `-intro` and `-note-<n>` names
  a real setting or table in the schema, including `vim.remap`,
  `vim.which_key_group` and `formatters.*`;
- the generated FTL matches the file;
- **a fake-locale test** renders the template with every message replaced by
  a marker and checks:
  - the sequence of commented-TOML lines is identical to en-US's, byte for
    byte;
  - each commented example block, uncommented by stripping `# `, parses as
    TOML with the same tables and keys as en-US's: `[[vim.remap]]` with
    `mode`, `from` and `to`, `[[vim.which_key_group]]`, and the four
    `[formatters.*]`;
  - the active defaults parse as before;
- **creation through Open Settings (`open.rs`) and through `write_setting`
  under the fake locale** produces the rendered template, not the English
  constant;
- **a live locale change reaches the toggle writer**:
  - **Where.** `crates/fe-workbench/tests/l10n_settings_writer.rs`, the only
    test in its binary, because the catalogue's locale is process-wide. Its
    fake locale is a `qps-ploc` fixture under `tests/fixtures/l10n/` with a
    marker for `settings-template-intro`, as in the command-label test
    (section 4).
  - **Steps.**
    1. Open a workbench in en-US and call `save_toggles_to` with a
       `settings.toml` path that doesn't exist yet. This initialises the
       writer.
    2. Switch to `qps-ploc` with I2's `set_locale`.
    3. Dispatch `view.toggle_status_bar`, then run until parked.
  - **It checks** that the file was created and that:
    - it starts with the `qps-ploc` intro marker, not the English intro;
    - its commented-TOML lines are en-US's, byte for byte;
    - it parses, and `status_bar.visible` reads back `false`.
  - **Before this design** it would fail: the writer captured the English
    template at step 1.
- **kiln-shell's own test** (`kiln/crates/kiln-shell/tests/persist.rs`) pins
  the timing without a locale. The provider returns whatever a `Mutex` holds:
  - `setting_writer` doesn't call it;
  - after the value changes, a write that creates the file uses the new
    value;
  - a write to an existing, non-empty file leaves its text untouched except
    the key.

**What stays English:** only the JSON schema, which is for taplo and other
tooling.

### Notifications and toasts

Toasts resolve at creation. They live for seconds, so one shown during a
locale change keeps its language.

```rust
// before: crates/fe-workbench/src/spell_commands.rs:347
Toast::error(format!("Can't add {word} to dictionary"))
    .detail(format!("It counts until you quit: {error}"))
// after
Toast::error(tr!("workbench-spell-add-failed", word = word))
    .detail(tr!("workbench-spell-add-failed-detail", error = error.to_string()))
```

The `log::warn!` beside it stays English, because logs are for us.

### Dialogs

NSAlert (`window.prompt`) and `PromptButton` take resolved strings. Answers are
matched by button index, never by label.

```rust
// before: crates/fe-workbench/src/plugin_prompt.rs:23
const BUTTONS: [&str; 2] = ["Allow", "Don't Allow"];
let answer = window.prompt(PromptLevel::Warning, &question, Some(&detail), &BUTTONS, cx);
// after
let buttons = [tr!("workbench-plugin-allow"), tr!("workbench-plugin-dont-allow")];
let answer = window.prompt(PromptLevel::Warning, &question, Some(&detail),
                           &buttons.each_ref().map(|b| b.as_ref()), cx);
```

### Vim messages

nvim translates its messages itself, through its `.po` files, and **keeps the
`E492:` code** in every language. Users search by the code.

So fe-vim keeps the code outside the catalogue and translates the text:

- The prefix is an id, not prose, so prepending it is not concatenating
  fragments.
- en-US entries for vim must stay byte-for-byte equal to nvim's English. The
  oracle tests compare them, and they run in en-US.

```rust
// before: crates/fe-vim/src/ex.rs:396–399
#[error("E492: Not an editor command: {0}")]
NotACommand(String),
// after: Display is written by hand
ExError::NotACommand(line) => write!(f, "E492: {}", tr!("vim-e492", command = line.as_str())),
//   vim.ftl: vim-e492 = Not an editor command: { $command }
```

Two other places format E-messages outside fe-vim:
`fe-workbench/src/git_commands.rs:260` and `fe-gpui/src/vim.rs:3061`. They
switch to functions that fe-vim exports, such as `fe_vim::messages::not_an_editor_command(&line)`.
So the keys stay inside fe-vim and those crates need no new dependency.

Summaries such as "3 substitutions on 2 lines" go through the same
`VimState.message` slot (`state.rs:363`). They become plural messages, with
nvim's English kept in en-US.

### Which-key group names and the LazyVim preset (T1395)

**Built-in key titles.** `WhichKeyLabel::Builtin(&'static str)` (`which_key.rs:63`)
holds a key instead of English and resolves when drawn.
`WhichKeyLabel::Command(id)` already resolves through the command catalogue,
so it follows the command titles for free.

```rust
// before: crates/fe-vim/src/which_key.rs:94 onwards
(ch('#'), "partial word backward"),
// after
(ch('#'), "vim-key-g-hash"),   // vim.ftl: vim-key-g-hash = partial word backward
```

**The LazyVim preset.** The bundled preset's `fegroup` names are LazyVim's own
English labels (`crates/fastedit/presets/lazyvim.vimrc:85–95`). The file is
unchanged. When the preset loader (`crates/fastedit/src/vim_remaps.rs:59`)
loads the **bundled** preset, it looks each name up as
`vim-group-<slug>`, falling back to the name as written.

```vim
" unchanged
fegroup <leader>f file/find icon=search
" vim.ftl: vim-group-file-find = file/find
```

**User groups stay as written.** That covers both `fegroup` lines in a user's
own vimrc and `[[vim.which_key_group]]`.

### Accessibility labels

VoiceOver reads `aria_label` in the app's language, so labels are ordinary
messages. The one production site today is the setup wizard's `node()`
(`setup_wizard.rs:567–585`):

```rust
// before: crates/fe-workbench/src/setup_wizard.rs:87
const SAMPLE_LABEL: &str = "Code sample";
self.node(div().id("sample"), Role::Image, SAMPLE_LABEL)
// after
self.node(div().id("sample"), Role::Image, tr!("workbench-wizard-code-sample"))
```

The A11Y cards in `qenw9xxdzk-accessibility-design.md` add their labels as
keys from the start. Section 10's rule applies to them.

### Plugins and extensions

**Extensions** use Tenmoku's own `extension.toml`. Nothing reads VS Code's
`package.nls.json` or `%key%` titles today. Stage 1 extensions contribute
grammars, languages, themes, LSP, snippets, keymaps and menus, and
`vmtnmkay62-extensions-and-plugins.md` § 8 lists localisation as out of scope.

**Plugin command titles** arrive as WIT `title: string`
(`kiln-plugin/wit/commands.wit:15`, passed through at
`kiln-plugin/src/commands.rs:582–590`). They become `Text::Verbatim`, already
in the plugin's own language.

```rust
// kiln/crates/kiln-plugin/src/commands.rs:582–590, unchanged
CommandMeta { id, title: command.title, description: String::new(), .. }
// meta.label() finds no `command-<id>` key and returns the plugin's own title
```

**Later, in the extensions stage that adds contributed settings and commands:**

- The WIT gains `locale: func() -> list<string>`, the negotiated chain, so a
  plugin can choose its own text.
- `extension.toml` strings may be `%key%`, resolved from the extension's
  `l10n/<bcp47>.ftl`.
- An adapted VS Code extension's `%key%` is resolved from its
  `package.nls.<locale>.json` when it is adapted, as VS Code does.

That is a future card of the extensions plan, not part of this migration.

### CLI `--help` and errors (crates/fastedit)

The CLI is parsed at the top of `main`, before settings load
(`crates/fastedit/src/cli.rs`). It uses the system locale only. The
`workbench.locale` setting doesn't reach it, because reading settings first
would delay startup.

The usage text becomes one multi-line Fluent message. Flag names inside it and
the `tenmoku:` program prefix stay verbatim, as vim's codes do. Isolation is
off for CLI output, because terminals may draw FSI/PDI.

```rust
// before: crates/fastedit/src/cli.rs:10, 68
const USAGE: &str = "\
Tenmoku, a code editor.
...";
CliError::UnknownFlag(flag) => format!("tenmoku: unknown flag {flag}\n\n{USAGE}"),
// after
CliError::UnknownFlag(flag) => format!("tenmoku: {}\n\n{}",
    tr!("fastedit-cli-unknown-flag", flag = flag.as_str()), tr!("fastedit-cli-usage")),
```

### Status bar

```rust
// before: crates/fe-gpui/src/pane_item.rs:1703–1709
let mut cursor = format!("Ln {}, Col {column}", row + 1);
if selections.len() > 1 {
    cursor += &format!(" ({} selections)", selections.len());
}
// after: one whole message per shape, no appended fragments
let cursor = if selections.len() > 1 {
    tr!("gpui-status-cursor-selections", line = row + 1, column = column, count = selections.len())
} else {
    tr!("gpui-status-cursor", line = row + 1, column = column)
};
```

**Decision:** one API, `tr!` / `text!` / `Text` / `verbatim!`, in GPUI-free
`kiln-l10n`, for every surface. Stored text is a `Text` resolved when drawn,
and transient text is resolved at once. Locale changes re-render through
`ActiveLocale`, `menus::refresh` and `refresh_windows`. Bundled menus use
`title_key` and command titles use `command-<id>`. Settings descriptions and
template comments get generated, golden-checked en-US `settings.ftl` and
`settings-template.ftl`, keyed through one `setting_key`. The template renders
in the user's locale. Vim translates its text but
keeps `E…:` codes verbatim, with en-US equal to nvim's English. Which-key
built-ins and the bundled preset's groups are keyed, and user groups stay as
written. Plugin titles are verbatim. The CLI follows the system locale.

## 5. Plurals and arguments

**Arguments are always named and passed whole.** Counts are numbers, and names
and paths are strings:

```fluent
workbench-git-changed-files = { $count ->
    [one] One file changed
   *[other] { $count } files changed
}
workbench-open-failed = Can't open { $path }: { $reason }
```

```rust
tr!("workbench-git-changed-files", count = files.len())
tr!("workbench-open-failed", path = display_path(&path), reason = error.to_string())
```

- **No concatenation.** A message is never built from fragments with `+=`,
  `format!` of two messages, or a sentence split around a value. The status
  bar's `" ({} selections)"` suffix becomes a whole message (section 4).
- **The only things allowed outside a message** are fixed codes and prefixes
  that are ids: `E492:` and `tenmoku:`.
- **Lists such as "a, b and c"** are passed as one pre-joined argument for now.
  ICU4X's `icu_list` is the answer if a locale needs it.
- **Plural selectors** use CLDR categories (`zero`, `one`, `two`, `few`,
  `many`, `other`) through `intl_pluralrules`. Every selector must have a
  `*[other]` default, which Fluent enforces at parse time.
- **Gender** or any other choice is a selector on a string argument. No code
  needs it today.
- **Numbers** format as `to_string` in every locale until `icu_decimal` is
  plugged in (section 2).
- **Paths** are passed already shortened by the caller. P8 adds `~`
  abbreviation and middle truncation, and that isn't the catalogue's job.

**Bidi isolation.**

- **What Fluent does by default.** It wraps every placeable in FSI (U+2068) and
  PDI (U+2069). The scratch build printed `⁨3⁩ Commit …`, with the marks around
  the 3.
- **Why it matters.** It stops an RTL file name in an LTR sentence, or an LTR
  path in a Hebrew sentence, from reordering the text around it.
- **How the renderer draws the marks.** The non-Latin audit
  (`yzegn08wk2-non-latin-text-audit.md`, G4) found that CoreText draws nothing
  for U+2066..U+2069 and applies the bidi algorithm. UI labels go through
  GPUI's text system, so on the Mac the marks are invisible and work as
  intended. On Linux, cosmic-text's handling isn't verified; I2's test checks
  it.
- **Where they would hurt.** They would be noise in:
  - every existing test that asserts English text;
  - text copied to the clipboard ("Copy details" on a toast);
  - text that enters an editor buffer, which G4's planned fix would draw as
    visible marks;
  - log lines and the CLI.

So `kiln-l10n` turns Fluent's own isolation off (`set_use_isolating(false)`)
and isolates at **interpolation**, never in the argument values.

- **Why not in the values.** Fluent matches selectors on the raw value:
  `FluentValue::matches` compares two strings with `a == b`
  (`fluent-bundle-0.16.0/src/types/mod.rs:186`), and the select expression
  resolves its selector before any formatting (`resolver/expression.rs:25–45`).
  If `Args` stored `gender = "\u{2068}male\u{2069}"`, `[male]` would never
  match, and every selector on a string argument would fall to its default.
- **The hook.** Fluent calls the bundle's formatter
  (`FluentBundle::set_formatter`, `types/mod.rs:223, 245, 267`) only when it
  writes a value into the output. A selector on the same argument still sees
  the raw value.
- **What `kiln-l10n` installs.** It installs one of two formatter functions per
  bundle, chosen by the bundle's locale direction. `set_formatter` takes a plain
  `fn`, hence two functions rather than one closure.
- **When it isolates.** The formatter returns `FSI + value + PDI` for a
  **string** value only when:
  - the bundle's locale is RTL, or
  - the value contains a strong RTL character (Unicode bidi class R or AL,
    from `unicode-bidi`, already in the lock).

  Otherwise it returns `None`, which keeps Fluent's own formatting. Numbers go
  through it too, to `icu_decimal` once that is plugged in, but are never
  isolated in LTR locales.

So en-US with ASCII arguments produces exactly today's strings: no test churn,
nothing on the clipboard. A Hebrew branch name is still isolated.

**I1's tests for this:**

- In an RTL test locale, `{ $gender -> [male] … *[other] … }` picks `[male]`.
- An argument used both as a selector and in the text
  (`{ $kind -> [file] File { $kind } … }`) selects on the raw value and is
  isolated where it is shown.
- A Hebrew string argument in en-US is isolated, and an ASCII one is not.
- A number is never isolated in an LTR locale.

Text that leaves the UI is passed through `kiln_l10n::strip_isolates`:

- clipboard copies of UI text;
- vim messages written into a buffer, as `:messages` would be;
- log lines;
- CLI output.

**Decision:** named arguments only, whole messages with CLDR plural
selectors, and no fragment concatenation apart from id prefixes (`E492:`,
`tenmoku:`). Bidi isolation is applied by `kiln-l10n`'s formatter, at
interpolation only, so selectors see raw values. It applies to string
arguments when the locale is RTL or the argument has strong RTL characters,
not by Fluent's blanket default. It is stripped where text leaves the UI. The renderer needs no
change on the Mac; Linux is checked in I2.

## 6. Locale selection

**The chain.** At startup, and on a settings change:

1. Read the user's preferred languages through kiln-platform's new
   `locale::preferred()`. It uses `sys-locale` 0.3.2 (MIT OR Apache-2.0), already
   in the lock through cosmic-text on Linux, so it is new to the Mac build
   only:
   - **macOS** calls `CFLocaleCopyPreferredLanguages`. That reads the
     `AppleLanguages` default, so a per-app language picked in System Settings ›
     General › Language & Region › Applications is honoured.
   - **Linux** reads `LANGUAGE`, then `LC_ALL`, `LC_MESSAGES` and `LANG`, the
     order gettext uses.

   kiln-platform is one of the two places allowed to branch on the OS
   (`check_kiln_boundary.py` `OS_DIRS`).
2. If `workbench.locale` isn't `auto`, put it first.
3. Negotiate against the shipped locales with
   `fluent_langneg::negotiate_languages(…, NegotiationStrategy::Filtering)`,
   always ending in `en-US`.

   For example, `de-AT, fr` against shipped `en-US, de` gives `de, en-US`.

**One fallback contract** for a `workbench.locale` that ships no translation:

- The override is dropped and negotiation goes on with the system's
  preferences, as `Filtering` naturally does.
- So `xx` on a German system with `de` shipped gives `de, en-US`. On a French
  system with no `fr`, it gives `en-US`.
- The diagnostic names the language actually chosen, never assuming English.

**The setting**, user-only. A project file can't set it: it uses the
`ProjectFilter` / `workspace_allowed` / `applied_project` path that
`workbench.icon_theme` uses.

```rust
// crates/fe-config/src/settings.rs, in WorkbenchSettings
/// The language of Tenmoku's own text: `auto` follows the system's preferred
/// languages; a tag such as `de` or `pt-BR` picks a shipped one. Menus and
/// panels change at once; macOS's own panels and the updater follow the
/// system (System Settings › Language & Region › Applications).
pub locale: LocaleSetting,   // "auto" | a BCP 47 tag
```

```json
"locale": {
  "description": "The language of Tenmoku's own text: …",
  "type": "string",
  "default": "auto",
  "enum": ["auto", "en-US"]
}
```

- **The schema's `enum`** is generated from the shipped locales, through a
  custom `JsonSchema` impl. It grows as locales ship, and editors complete it.
- **An unknown or unshipped tag** is accepted by serde and dropped from the
  chain, as above. It shows a diagnostic in the config problems list. The
  diagnostic is itself a key, worded "No `{ $requested }` translation; using
  `{ $chosen }`", where `$chosen` is the first locale of the negotiated chain.
- **I2's tests** pin all three cases:
  - `xx` with system `de` and shipped `de` gives `de`, and the diagnostic says
    `de`;
  - `xx` with system `fr` gives `en-US`, and the diagnostic says `en-US`;
  - a shipped override is used with no diagnostic.
- **The template** gets `# locale = "auto"`.

**Live, not on restart.** A change takes these steps:

1. Rebuild the bundles. The new locale's resources are parsed, about 0.5 ms.
2. Bump the generation.
3. `menus::refresh`.
4. `refresh_windows`.

What follows when:

- Panels, the palette, menus and the status bar change at once.
- Toasts and NSAlerts already on screen keep their language.
- AppKit-owned text does not change mid-session, because AppKit picks the
  app's language once, at launch. That covers the system items AppKit adds to
  menus, open and save panels, and Sparkle's alert (section 7). It follows the
  system or per-app language.
- The setting's description says this.

**What follows the locale and what doesn't:**

- **Keybinding labels.**
  - macOS shows symbols (⌘⌥⇧⌃) and stays unchanged.
  - On Linux, `kiln-core/src/label.rs` spells keys as words: `"Ctrl"`,
    `"Alt"`, `"Shift"`, `"Enter"`, `"Space"`, `"Esc"`, `"PageUp"` and so on
    (16 literals). They become `kiln-core-key-*` keys, as GNOME localises them
    ("Strg" in German).
  - Keymap files and their syntax (`secondary-s`, `ctrl-shift-p`) stay English
    ids.
  - The shortcuts editor's search matches both the shown label and the id.
- **Number formats.** Unchanged (section 2) until `icu_decimal` is plugged in.
- **Dates.** The Git log and Local History format their own dates
  (`local_history_view.rs` `stamp`). They stay as they are until a locale needs
  `icu_datetime` (open question Q4).
- **The vim keyboard layout.** Unchanged. fe-vim acts on the characters the OS
  delivers. The keyboard layout is an OS setting, independent of the UI
  language, and keymaps name keys, not words.
- **Spelling.** The speller's own `"auto"` language (`mac_speller.rs:18–56`) is
  independent. A UI in German doesn't mean prose files are German.

**Decision:** preferred languages come from `sys-locale` in kiln-platform
(macOS `CFLocaleCopyPreferredLanguages`; Linux `LANGUAGE`/`LC_ALL`/`LC_MESSAGES`/`LANG`),
with a user-only `workbench.locale` (`auto` or a BCP 47 tag) placed first. The
result is negotiated against the shipped locales and always ends in `en-US`.
An unshipped override is dropped, and its diagnostic names the language
actually chosen.
It applies live to Tenmoku's text; AppKit-owned text follows the system at
launch. Linux key-name labels are translated; macOS symbols, keymap syntax,
numbers, dates and vim keys are unchanged.

**I2's measurement (T531, 2026-11-08, Linux).** `tenmoku --perf` on a
26 KB file, release builds of the base (fe61cc26) and of T531, launched in
turn under Xvfb with Mesa's software Vulkan on helium (24 cores, shared
with other lanes' builds). Times are from the top of `main`:

| Runs (base, T531), load | Milestone | Base, median (min) | T531, median (min) |
|---|---|---:|---:|
| 15, 15; 25–48 | app callback | 22.8 ms (19.0) | 20.3 ms (16.9) |
| | window open, first frame drawn | 235.6 ms (209.6) | 243.5 ms (217.8) |
| | first frame presented | 292.1 ms (240.3) | 306.3 ms (250.8) |
| 25, 24; 75–94 | window open, first frame drawn | 579.9 ms (315.1) | 618.6 ms (329.4) |
| | first frame presented | 783.6 ms (398.4) | 767.0 ms (412.9) |
| 30, 30; 101–126 | window open, first frame drawn | 975.9 ms (520.1) | 984.2 ms (482.1) |
| | first frame presented | 1170.0 ms (683.9) | 1233.9 ms (667.1) |

The runs spread over hundreds of milliseconds on a machine shared with
other lanes, so the end-to-end numbers can't resolve a millisecond either
way: one batch has T531 faster to the presented frame, the others slower,
by more than locale selection could cost.

What locale selection adds before the first frame is two calls on the main
thread; the warm-up's own work runs on a thread of its own. `--perf` times
both in the release app, as it starts (`open: l10n on the main thread`):
the call to `kiln_platform::locale::warm()` at the top of `main`, which
spawns the thread, and the config service's first application of
`workbench.locale` (`preferred()`, `LocaleSetting::choose`, `set_locale`),
including any wait on the warm-up's lock. In the last batch's 30 T531
launches:

| Main-thread work | Median | p90 | Max |
|---|---:|---:|---:|
| `warm()`, spawning the thread | 67 µs | 90 µs | 1193 µs |
| applying `workbench.locale` | 29 µs | 41 µs | 49 µs |
| both | 99 µs | 123 µs | 1220 µs |

The one launch over a millisecond was a thread spawn that waited at a load
of about 120 on 24 cores; the other 29 took at most 0.2 ms in all.
Applying the setting, wait included, never took more than 49 µs: the
warm-up had finished by then, so `set_locale` found its chain and rebuilt
nothing. So the main thread does about 0.1 ms more before the first
frame, well under the 1 ms budget.

What this doesn't bound is the warm-up's thread competing with the main
thread for a core while it builds the catalogue: on helium's load that
effect drowns in the noise above. The end-to-end before/after on a quiet
machine, resolving 1 ms, is T533's.

## 7. Bundle

**Info.plist today** (`crates/fastedit/macos/Info.plist`):

- `CFBundleDevelopmentRegion` is `en`. There is no `CFBundleLocalizations` and
  no `.lproj`.
- `scripts/bundle-macos.sh` copies it, `sed`s `@VERSION@`, `plutil`s the SU
  keys, and writes only `Tenmoku.icns` into `Contents/Resources`.

**How AppKit picks the app's language.** At launch AppKit takes the user's
preferred languages and intersects them with the main bundle's localisations.
Those come from `.lproj` folders, or from the `CFBundleLocalizations` array
when no folders exist. The result is the app's language for everything AppKit
draws on Tenmoku's behalf:

- **the items AppKit inserts into our menus:**
  - Edit's Start Dictation, Emoji & Symbols and AutoFill;
  - Window's tiling and Bring All to Front items, once P4 makes Window the
    `windowsMenu`;
  - View's Enter Full Screen after P5;
- **the Services submenu's contents**, the services themselves, which their
  apps name;
- **open and save panels**, the colour and font panels, and the
  spelling panel.

With only `en` declared, all of these stay English on a German system, even
once Tenmoku's own strings are German.

**Tenmoku's own app-menu items are ours, not AppKit's.** "Hide Tenmoku", "Hide
Others", "Show All" and "Quit Tenmoku" are literals in
`fe-keymap/menus/fastedit.toml`. They are translated through the catalogue,
not through AppKit.

**So is the Services item's own title.** It is a kiln `SystemItem`
(`kiln-core/src/menu.rs:113`, defaulting to `"Services"` at `:601`).
kiln-platform passes `system.title` to GPUI's `MenuItem::os_submenu`
(`kiln-platform/src/menus.rs:145`), which sets the native item's title
explicitly. AppKit therefore doesn't translate it, so it is keyed like every
other bundled title (section 4).

**Sparkle.**

- The bundled Sparkle is 2.10.1. Its `Resources` has 37 `.lproj` folders: ar,
  ca, cs, da, de, el, es, fa, fi, fr, he, hr, hu, is, it, ja, ko, nb, nl, nn,
  pl, pt-BR, pt-PT, ro, ru, sk, sl, sv, th, tr, uk, vi, zh_CN, zh_HK, zh_TW,
  plus Base.
- Tenmoku uses the standard user driver, `SPUStandardUpdaterController`
  (`crates/fastedit/src/sparkle.rs:247`), so the update alert and permission
  prompt are Sparkle's nibs.
- A framework's localisation is limited to the main bundle's languages unless
  the app sets `CFBundleAllowMixedLocalizations`.
- **So Sparkle shows English today on every system.** Once Tenmoku declares
  `de`, Sparkle shows German on a German system.
- The appcast's release notes are English (`make-appcast.sh`). Sparkle can pick
  per-language notes (`sparkle:releaseNotesLink xml:lang`), but that is a
  release-process question, left out of scope.

**What changes, and only when a second locale ships** (card B1):

- **`CFBundleLocalizations`** is written by `bundle-macos.sh` from the shipped
  locale directories, mapped to Apple's names. `en-US` becomes `en`; `pt-BR`
  stays `pt-BR`; `zh-Hans` becomes `zh-Hans` (Sparkle's `zh_CN` matches it
  through Apple's fallback).
- **One `Contents/Resources/<name>.lproj/InfoPlist.strings` per locale**,
  holding only `CFBundleDisplayName` and `NSHumanReadableCopyright`.
  `.lproj` folders are the usual way a bundle declares a language, and Apple
  documents `CFBundleLocalizations` for apps that keep their strings elsewhere,
  as Tenmoku does. Shipping both covers either lookup; B1's human check
  confirms which one AppKit honours. The folders also let Finder show a
  localised name if we ever want one.
- **`CFBundleDevelopmentRegion` stays `en`.**
- **`CFBundleAllowMixedLocalizations` is not set.** That way Sparkle and AppKit
  never show a language Tenmoku's own strings aren't in.
- **Until then nothing in the bundle changes.** Declaring only `en` is today's
  behaviour.

**Decision:** no bundle change until a second locale ships. Then B1 makes
`bundle-macos.sh` write `CFBundleLocalizations` and a minimal `.lproj` with
`InfoPlist.strings` for each shipped locale. `CFBundleDevelopmentRegion` stays
`en` and `CFBundleAllowMixedLocalizations` stays unset. AppKit's inserted menu
items, the system panels and Sparkle's standard UI then follow automatically,
in exactly the shipped languages. A human checks that once on a system set to
the new language.

## 8. Keeping the catalogue complete

Three layers keep the catalogue complete:

1. **Types**, for kiln's widgets.
2. **A literal lint**, for everything else.
3. **A key check**, for the catalogue itself.

The two checks ship as a library in `kiln-l10n` behind a `lint` feature
(dev-dependency only). Each crate calls them from its own test:

```rust
// crates/fe-workbench/tests/l10n.rs (or the crate's tests/it.rs module)
#[test]
fn ui_text_is_in_the_catalogue() {
    kiln_l10n::lint::literals(env!("CARGO_MANIFEST_DIR"))
        .allow_list("l10n-allow.toml")
        .check();
}

#[test]
fn keys_used_here_exist() {
    kiln_l10n::lint::keys(env!("CARGO_MANIFEST_DIR"), fe_workbench::l10n::RESOURCES).check();
}
```

### Layer 1: types for kiln's label constructors

Two constructors take `impl Into<Text>` (section 4):

- `Picker::new`'s placeholder;
- `Button::new`'s label.

During the migration, `Text`'s `From` impls for `&str`, `&String`, `String`,
`SharedString` and `Cow<str>` let every card build alone. Card Z1 removes
them, so a `Text` comes only from `text!` or an explicit `Text::verbatim(..)`.
`tr!` returns a resolved `SharedString`, and `From<SharedString>` goes too, or
any `SharedString` would pass. So Z1 turns every `tr!` passed to these two
constructors into `text!`, which takes the same key and arguments.

After Z1 a label can't reach those two constructors without passing through
the catalogue or an explicit verbatim, however it is stored: a `const`, a
local or a struct field. `Picker::new(d, PLACEHOLDER, …)` with
`const PLACEHOLDER: &str = "files"` stops compiling.

**Everything else is layer 2's job:**

- toasts (`Toast::*` in fe-workbench takes `impl Into<String>`);
- `StatusItem` fields;
- command titles, which come from keys derived from ids and the registry test;
- GPUI's own sinks, such as `.child("…")` and `SharedString::from`.

Typing those would change struct fields and hundreds of call sites for little
more than R2 already gives.

### Layer 2: no new hard-coded user-facing literal

It walks the crate's non-test source with `syn` 2, which is already in the
lock (`full` and `visit`). It skips:

- `#[cfg(test)]` items, `tests/`, `benches/`, `examples/` and `build.rs`.

It reports a literal under any of four rules.

- **R1, anywhere.** The literal contains whitespace and a letter, or starts
  with an uppercase letter followed by a lowercase one.
  - This covers sentences, titles, and capitalised single words wherever
    they are stored. `const BUTTONS: [&str; 2] = ["Allow", "Don't Allow"]` is
    reported.
- **R2, UI sinks, whatever the case.** The literal contains any letter and is
  an argument of a UI sink:
  - `.child(`, `.children(`, `.tooltip(`, `.aria_label(`, `.placeholder(`;
  - `Toast::*`, `Button::new`, `PromptButton::*`, `Picker::new`;
  - `window.prompt`, `register(`, `SharedString::from`,
    `SharedString::new_static`, `StatusItem` fields.

  A sink argument that is a **path to a `const` or `static`** in the crate, or
  a **local bound directly to a literal** in the same function, is checked as
  if that literal were inline. So `.child("reload")` and
  `const RELOAD: &str = "reload"; … .child(RELOAD)` are both reported.
- **R3, format strings.** A `format!`, `write!` or `writeln!` format string
  that has a word of two or more letters outside its `{…}` placeholders is
  reported. That covers `"({chars} selected)"` and `"recording @{register}"`,
  lowercase text that R1 misses.
- **R4, user-facing output.** The arguments of `println!`, `eprintln!`,
  `print!` and `eprint!` are checked as R1 and R3. They are **not** exempt,
  because CLI errors and `--help` are user-facing. Developer tooling that
  prints is allow-listed by file, for example
  `kiln-platform/src/screenshots/goldens.rs`.

**Contexts that are not reported:**

- **Logging and assertions:** the arguments of `log::*` and `tracing::*`
  (developer logs), `panic!`, `assert*!`, `unreachable!`, `todo!`, `.expect(`,
  `concat!`, `env!` and `include_*!`.
- **`Debug` impls:** anything inside an `impl fmt::Debug` (`debug_struct`,
  `write!`).
- **Matchers on external text:**
  - `match` patterns, and `==` / `!=` operands;
  - `.contains(`, `.starts_with(`, `.ends_with(`, `.strip_prefix(`, `.find(`.

  This is how fe-git's stderr strings stay.
- **Subprocess arguments:** `Command::new`, `.arg(s)` and `.env(`.
- **Id attributes:** `#[serde(rename)]` and `#[command(id)]` arguments.
  A `#[error(…)]` **is** reported, because its text is shown to users.
- **The explicit marker:** `verbatim!("…")`.

**Lowercase single words outside sinks and format strings are not reported.**
fe-workbench has 733 such literals. A random sample of 25 had 24 ids:

- `"escape"`, `"left"`, `"normal"`, `"cargo"`;
- TOML keys and LSP token kinds.

Reporting them all would bury the allow-list.

**The residual gap.** One of the 25 was a plural-noun pair handed to a counting
helper: `("implementation", "implementations")` in `nav.rs`, and in the same
style `count(counts.warnings, "warning")` in `problems_panel.rs`.

- The migration cards replace those helpers with plural messages.
- After that, the codex reviewer's check against English plural helpers
  (section 10) is what catches a new one.
- Kiln widgets are covered by layer 1 regardless.

**The allow-list file.** `l10n-allow.toml` sits at the crate root and lists:

- whole files, such as `src/language.rs` (language display names);
- single `file:literal` pairs.

Each entry has a reason. During a crate's migration the file also has a
`pending` list of files not yet migrated. Each fe-workbench card removes its
files from it, which is how the lint is "switched on per crate", and per area
within fe-workbench.

**False positives.** I applied R1–R4 by hand to the 80 sampled candidates
under [Background](#background-what-is-there-today):

- They report all 74 user-facing strings.
- The two `Debug` impls are skipped.
- That leaves 4 for the allow-list: "Application Support" (a path component),
  "Unicode-3.0" (a licence id), and "Java" and "Dockerfile" (language display
  names, values in a `match` arm). Each falls in a file that is naturally
  allow-listed whole: paths, the licence table, the language table.

So about 5% of candidates.

**The lint's own tests in I1.** Each case below is a fixture:

| Reported | Not reported |
|---|---|
| a capitalised const button array | `log::warn!("can't add …")` |
| a lowercase label in `.child("reload")` | `.contains("Authentication failed")` |
| the same through a `const` | `Command::new("git").arg("Status")` |
| `Toast::info(LABEL)` with `LABEL = "Saved"` | text inside an `impl Debug` |
| `format!("({chars} selected)")` | `#[serde(rename = "Foo")]` |
| `eprintln!("tenmoku: unknown flag {flag}")` | `verbatim!("Cargo.toml")` |
| an `#[error("E492: …")]` | |

**Data files** are checked by their loaders' own tests, not by the syn pass:

- the bundled menu test rejects `title` in bundled files, which must use
  `title_key`;
- the settings goldens cover descriptions and the template;
- the preset test checks that every bundled `fegroup` has a `vim-group-*`
  key.

### Layer 3: keys and catalogue match

**Ownership.** Every resource belongs to the crate that lists it in
`resources!`. A crate may use its own keys. It may use another crate's key
only if that crate puts the key in a `## public` group comment in its FTL
file. Fluent group comments are part of the syntax, so this needs no new
format. kiln's generic messages, such as a Cancel button, are public for apps.
fe-vim instead exports functions for its messages (section 4).

**Per crate**, in the crate's own `keys_used_here_exist` test:

- **Every key used exists.** Each key literal the crate passes to `tr!`,
  `text!` or `tr_or` must exist in the en-US resources linked into its test
  binary: its own, plus its dependencies', through `inventory`.
- **Cross-crate keys are public.** A key that is another crate's must be in
  that crate's `## public` group.
- **Every `.ftl` parses**, and every file on disk is listed in `resources!`.

**At the two catalogue roots** run the checks that need the whole picture. Both
read the tree from disk, not through what a binary links, so no crate is missed
for lack of a dependency edge.

**The app root**, `crates/fastedit/tests/l10n_catalogue.rs`, added by I2:

- **Resources:** every `.ftl` under `crates/*/l10n/` and
  `kiln/crates/*/l10n/`.
- **References:** every non-test source file under `crates/*/src` and
  `kiln/crates/*/src`, and the bundled data:
  - `title_key` in `crates/fe-keymap/menus/*.toml` and
    `kiln/crates/kiln/src/setup/menus.toml`;
  - the preset's group names.

  So a message used only inside kiln counts as used.
- **Exemptions:** none, not even kiln's `## public` keys, because the app sees
  every user of them.
- **When it runs:** on every change to any crate in either tree.
  `scripts/ci-test-affected.sh` tests a changed package's reverse dependencies,
  and `crates/fastedit` depends on every app crate and, through them, on every
  kiln crate. That includes `kiln-plugin`, through fe-workbench
  (`crates/fe-workbench/Cargo.toml:47`).

**The kiln root**, `kiln/crates/kiln/tests/l10n_catalogue.rs`, added by I3:

- **Resources:** every `.ftl` under `kiln/crates/*/l10n/`.
- **References:** every non-test source file under `kiln/crates/*/src`, and
  `kiln/crates/kiln/src/setup/menus.toml`.
- **Exemptions:** kiln's `## public` keys, because apps outside kiln may be
  their only users.
- **Why it exists:** kiln has to keep its catalogue whole if it leaves this
  repository, where the app root can't see it.
- **When it runs:** `kiln` doesn't depend on `kiln-plugin`. So a change to
  kiln-plugin alone runs only the app root, which already covers kiln-plugin's
  resources and references. That is enough inside this repo.

**The checks at both roots:**

- **No key is unused.** A key counts as used when it appears in one of these:
  - a key literal in a referencing source file;
  - bundled data;
  - a reference from another message, `{ other-message }` or `{ -term }`,
    found in any variant or attribute (below);
  - a declared dynamic prefix with its own test: `command-*` against the
    registry, `settings-*` and `settings-template-*` against the goldens,
    `vim-group-*` against the preset.
- **Every reference resolves, in every branch.** This is a static walk of
  each resource's `fluent-syntax` AST, not formatting. It visits:
  - every message value and attribute;
  - every term;
  - every variant of every select expression, including nested ones.

  Each `MessageReference`, `TermReference` and attribute accessor must
  resolve **inside the bundle that formats it**. That bundle is the
  reference's own locale, all crates combined (section 3). en-US doesn't
  count for another locale:
  - a reference in a `de` resource must name a message or term that a `de`
    resource defines;
  - an attribute accessor, `{ msg.attr }` or a selector on `-term.attr`, must
    name an attribute that the **`de`** definition of that entry has. An
    en-US definition with the attribute doesn't count, because Fluent takes
    the whole entry from the bundle it formats in.

  This matches the runtime. The per-message fallback (section 3) moves a whole
  missing message to en-US's bundle, where en-US's own references resolve. It
  never mixes a `de` message with en-US terms. So a translator who adds a
  message also adds, in the same locale, every term and message it references.
  Terms are few: the brand and product names.

  The error names:
  - the locale;
  - the file and the message;
  - the variant path, such as `[few]` or `[file]`;
  - the reference;
  - "defined only in en-US" when en-US has it, which is the usual cause.

  Formatting alone would miss a bad reference in a plural or gender branch
  that its dummy arguments don't select. **I1's fixtures** pin the rule:

  | Fixture | Resources | Walk | Formatting |
  |---|---|---|---|
  | Undefined term in en-US | en-US `files-left` has an undefined `{ -brand }` inside `[few]` | rejects | `$count = 1` passes |
  | **Term only in en-US, in an unselected branch** | en-US defines `-brand = Tenmoku`; `pl`'s `files-left = { $count -> [one] … [few] Zostały { $count } pliki { -brand } *[other] … }`, with no `pl` `-brand` | rejects `pl` `files-left` `[few]` `-brand`, "defined only in en-US" | `$count = 1` passes; through the catalogue in `pl`, `$count = 3` selects `[few]` and reports Fluent's unknown-term error, so the walk and the runtime agree |
  | **Attribute only in en-US, in an unselected branch** | en-US `save = Save` with `.tooltip = Save the file`; `pl` `save = Zapisz` with no attribute, and `pl` `save-hint = { $kind -> [file] { save.tooltip } *[other] Zapisz wszystko }` | rejects `pl` `save-hint` `[file]` `save.tooltip`, "defined only in en-US" | the dummy `$kind` selects `*[other]` and passes; `$kind = "file"` in `pl` reports the unknown-attribute error |
  | The same two, fixed | `pl` adds `-brand` and `save`'s `.tooltip` | accepts | formats in every branch |

- **Every message formats.** After the walk, every message of every locale is
  formatted once with dummy arguments, as a smoke test for runtime errors
  such as cyclic references.
- **Duplicates fail.** A key defined twice anywhere in the resources is an
  error.
- **Other locales stay in step with en-US.**
  - A key that isn't in en-US is an error: a stale translation.
  - An argument the en-US message doesn't take is an error.
  - A key missing from a locale is only counted, because it falls back.

**Until a crate is migrated,** it has no `.ftl`, so the roots have nothing of
its to check, and an unmigrated crate is never blocked.

### Where it runs, and why there

It runs as **a test in each crate**, plus the two root tests, in the existing
`test` stage. The gate tests the packages a change affects and their reverse
dependencies (`scripts/ci-test-affected.sh`, regatta.toml's `test` command).
So the check costs one syn pass over one crate, under a second.

A change to an FTL file or a bundled menu file touches its crate, so that
crate's tests run.

- The app root runs on every change in either tree, because `crates/fastedit`
  is a reverse dependency of every crate.
- The kiln root runs on changes to the kiln crates it depends on, which is all
  of them except `kiln-plugin` (above).

A crate opts in when its migration card lands.

The alternative was a Python script beside `check_kiln_boundary.py`, called by
`check-no-gpui.sh` in the `no_gpui` stage, which has no path filter. It would
need no build, but it is rejected:

- A regex over Rust can't see a `Debug` impl, a macro argument or a multi-line
  raw string, which is where the false positives come from.
- It would be one script judging every crate. Kiln's copy would have to live
  in `kiln/scripts` to keep kiln standalone.

**No new gate stage is needed**, so no change to `regatta.toml`. If the user
later wants the check to run on every commit regardless of path, that is a new
stage, and `regatta.toml` is a path that needs a human.

**Decision:** three layers:

1. **Types:** `Picker::new` and `Button::new` take only `Text` once Z1 removes
   the transitional conversions.
2. **A `syn` literal lint** with rules R1–R4. Capitalised and spaced text is
   reported anywhere, any text in a UI sink (following consts and simple
   locals), words in format strings, and CLI output. A per-crate
   `l10n-allow.toml` lists exceptions.
3. **Key checks.** Per crate: used keys exist, and cross-crate keys are
   `## public`. At the app and kiln roots, read from disk: unused keys
   (including data files and FTL references); a static AST walk that every
   reference resolves in every branch, inside its own locale's bundle;
   formatting; duplicates; and stale translations.

All of it runs as tests in the existing `test` stage, so there is no new stage
and no `regatta.toml` change. It is switched on crate by crate, and within
fe-workbench area by area through the `pending` list.

## 9. Migration plan

**Order.**

1. The infrastructure (I1–I3).
2. kiln's own crates, which have no polish dependencies except menus and the
   picker.
3. The headless crates.
4. fe-gpui and fe-workbench, ordered after the T1429 polish cards that rewrite
   their text.

Each card adds the crate's `l10n/en-US/*.ftl`, moves the strings, switches on
that crate's (or area's) lint, and keeps every en-US string byte-equal to
today's. So no golden and no existing test changes, except tests that
construct removed literals.

**Strings T1429's follow-ups are likely to rewrite.** None of those P-cards is
filed yet: they wait for the user's direction. A migration card that shares
files with one of them lands after it, to avoid moving text twice:

| P-card | Strings it rewrites |
|---|---|
| P4, P5 | The macOS app menu, About, Help and Window menus, and View items as Show/Hide (`fastedit.toml`, `kiln-core/src/menu.rs`, `kiln-platform/src/menus.rs`) |
| P7 | Empty-state text in the Problems, Search no-results and Git no-repo views, and a new kiln-ui `EmptyState` |
| P8 | The trust question and the plugin alert text, made shorter (`trust.rs`, `plugin_prompt.rs`), and Start page recents |
| P11, P17 | Start page layout, and settings category order and titles |
| P13 | Dock headers that name each view (`kiln-shell/src/dock.rs`, `view_stack.rs`, `project_panel.rs`, `search_panel.rs`) |
| P18 | The palette's count line |
| P19 | The commit box placeholder and the button tooltip (`git_panel.rs`) |
| P20 | The Git log's tool-window title |
| P3, P16 | `toast.rs` layout |
| P21 | `hover.rs`, `completion_menu.rs` |
| P22 | `peek.rs` |

T1396 owns `crates/fe-gpui/src/vim/which_key.rs`.

In the table, "Pn" means T1429's card when it is readied: wait for it if it is
filed and unlanded, ignore it if the user drops it. Each estimate is
**messages moved**. fe-workbench's come from the per-file count in the background section,
which is about 7% high for non-UI strings, plus doc-comment command titles.

**Every card lands alone and builds.** The API changes are additive until the
last card, Z1:

- **`Text` converts from strings.** It has `From` impls for `&str`, `&String`,
  `String`, `SharedString` and `Cow<str>`, so existing call sites of
  `Picker::new` and `Button::new` keep compiling. I3 changes those two
  signatures, before any card that migrates their callers. Every such card
  depends on I3: K2, G1, W1–W8 (through W1), CV1, F1 and X1.
- **`CommandMeta` doesn't change.** Its localised text comes from the
  `label()` and `detail()` methods (section 4), so none of its struct literals
  change.
- **`CommandItem` gains one field.** That is `title_key: Option<String>`, and
  K1's scope includes its three struct literals.
- **Nothing is `#[deprecated]`.** The `vet` stage's clippy runs with
  `-D warnings`, so a deprecation would fail every later card's build.
- **Z1 finishes the switch.** It moves the remaining `title` readers to
  `label()` and removes `Text`'s `From` impls, which turns on layer 1 of
  section 8.

**Cards that share a crate land in order, so none edits another's
allow-list.**

- V1 and V2 land before G1. So when G1 switches fe-gpui's lint on, `vim.rs`
  and `vim/which_key.rs` are already migrated, and nothing is `pending`.
- V2 lands before F1, which covers `vim_remaps.rs`.
- V1 lands before W1, which shares `git_commands.rs`.

**Shorthand.** In a scope, **L(`<dir>`)** stands for the files every crate's
migration touches:

- `<dir>/Cargo.toml`, which gains the `kiln-l10n` dependency and its `lint`
  feature as a dev-dependency;
- `Cargo.lock`;
- `<dir>/src/l10n.rs`, the `resources!` list;
- `<dir>/src/lib.rs`, for its `mod l10n;`;
- `<dir>/l10n/**`;
- `<dir>/l10n-allow.toml`;
- `<dir>/tests/**`, which holds the l10n tests.

Each card's scope is its L(…) entries plus the files listed.

| Card | Size | Scope globs | Depends on | Platform | Strings |
|---|---|---|---|---|---:|
| **I1** `kiln-l10n`: Fluent wrapper, a lazy `inventory` catalogue with per-message en-US fallback and log-once, `tr!`/`text!`/`Text` (with the transitional `From<&str>`/`From<String>`)/`verbatim!`/`tr_or`, formatter-based isolation and `strip_isolates`, the arg-less cache, negotiation, `resources!` with `## public` groups and dynamic prefixes, one bundle per locale holding only that locale's resources (section 3), the `lint` feature (layers 2 and 3) and its fixtures (section 8), including the per-bundle reference walk's fixtures with a term and an attribute defined only in en-US in an unselected branch, the isolation and selector tests (section 5) | m | `kiln/crates/kiln-l10n/**`, `Cargo.toml` (workspace member), `Cargo.lock` | this doc approved | Linux-OK | 0 |
| **I2** Locale selection: kiln-platform `locale.rs` (`sys-locale`), the `ActiveLocale` global, `set_locale` → bundles + `menus::refresh` + `refresh_windows`; `workbench.locale` (user-only, schema enum, template line, the unshipped-locale diagnostic and its three tests); `crates/fastedit` warms the catalogue on a thread at the top of `main`, applies the setting, adds the app root test `tests/l10n_catalogue.rs` and a `--perf` before/after; a Linux check that FSI/PDI draw nothing | m | L(`kiln/crates/kiln-platform`), L(`crates/fe-config`), L(`crates/fastedit`), `kiln/crates/kiln-platform/src/locale.rs`, `kiln/crates/kiln-platform/src/lib.rs`, `kiln/crates/kiln-platform/src/config.rs`, `crates/fe-config/src/settings.rs`, `crates/fe-config/src/tests/**`, `crates/fe-config/settings.schema.json`, `crates/fe-config/settings-template.toml`, `crates/fastedit/src/main.rs`, `crates/fastedit/src/config.rs` | I1 | Linux-OK (the macOS preference path is `sys-locale`'s, exercised by B1's human check) | 3 |
| **I3** Command titles as keys, and the label constructors: `Picker::new`'s placeholder and `Button::new`'s label take `impl Into<Text>`, with the transitional `From` impls, so every later card can pass `text!`; and, with no struct change, the command-label resolver `kiln-command/src/l10n.rs` (`command_key`, `command_label`, `command_detail`, `derive_label`; section 4) with `label()` on `CommandMeta` (and `detail()`), `TypedCommand` and `CommandSpec`, each falling back silently to its registered `title`/`description`; the macro, `register` and `register_command` unchanged; the palette (`command_palette.rs:93, 98`) sorts and matches on `label()` when it opens; the menu items at `contributions.rs:1737` and `:1879` read `label()`, and kiln re-resolves its menus (`resolve_again`) on an `ActiveLocale` change; the panel toggles as `kiln-command-toggle` through `derive_label` (`:1072, 1507`); the fake-locale test `kiln/crates/kiln/tests/l10n_command_labels.rs` (palette and both menu paths, and a keyless command's silent fallback); kiln's own registry command titles; the registry test (every command of a migrated crate has a key, every `command-*` key has a command); kiln's root test `tests/l10n_catalogue.rs` | m | L(`kiln/crates/kiln-command`), L(`kiln/crates/kiln-shell`), L(`kiln/crates/kiln-ui`), L(`kiln/crates/kiln`), `kiln/crates/kiln-shell/src/picker.rs`, `kiln/crates/kiln-ui/src/controls.rs`, `kiln/crates/kiln-command/src/**`, `kiln/crates/kiln-shell/src/command_palette.rs`, `kiln/crates/kiln/src/contributions.rs`, `kiln/crates/kiln-platform/src/command_actions.rs` (`TypedCommand::label()`; kiln-platform already depends on kiln-command) | I1, I2 (`ActiveLocale` and `set_locale`, which the menu re-resolve and the test use); not alongside P18 (same `command_palette.rs`) | Linux-OK | 25 |
| **K1** kiln-core and kiln menus: `CommandItem` and `SystemItem` get `title_key` and `label()` (Services becomes `kiln-core-menu-services`), menu `id`; `{app}` as `$app`; Linux key-name labels; extension, paths, settings and secrets messages; kiln's `menus.toml`; `menus::refresh` (including `os_submenu` at `menus.rs:145`) and the drawn bar read `label()`; a fake-locale test of the bundled Services item and a keyed command item; lint on for kiln-core | m | L(`kiln/crates/kiln-core`), `kiln/crates/kiln-core/src/**` (including the struct literals at `menu.rs:600` and `:635`), `kiln/crates/kiln/src/contributions.rs` (its literals at 1739 and 1877), `kiln/crates/kiln/src/setup/menus.toml`, `kiln/crates/kiln/src/setup/defaults.rs`, `kiln/crates/kiln-platform/src/menus.rs`, `kiln/crates/kiln-shell/src/menu_bar.rs` | I2 (kiln-platform's dependency), I3 (kiln-shell's and kiln's), P4, P5 | Linux-OK (data-level tests; the Mac bar's look was P4's human check) | 51 |
| **K2** kiln-shell, kiln-platform, kiln-ui, kiln-plugin and kiln: picker actions, trust, close and pane prompts, fonts, platform commands, dialogs, plugin messages; lint on for those five crates | m | L(`kiln/crates/kiln-shell`), L(`kiln/crates/kiln-platform`), L(`kiln/crates/kiln-ui`), L(`kiln/crates/kiln-plugin`), L(`kiln/crates/kiln`), `kiln/crates/kiln-shell/src/**`, `kiln/crates/kiln-platform/src/**`, `kiln/crates/kiln-ui/src/**`, `kiln/crates/kiln-plugin/src/**`, `kiln/crates/kiln/src/**` | K1, P2, P7, P13, P18 | Linux-OK | 61 |
| **E1** fe-editor: command titles and descriptions (`commands.rs` 50, fold levels, multi-cursor, column select, row edits, sort, indent) | m | L(`crates/fe-editor`), `crates/fe-editor/src/**` | I1, I3 | Linux-OK | 107 |
| **V1** fe-vim messages: `ExError`'s 30 variants with `E…:` codes kept, other messages and summaries as plurals with en-US equal to nvim's English, exported as `fe_vim::messages::*`; the E-messages formatted in `fe-workbench/src/git_commands.rs:260–264` and `fe-gpui/src/vim.rs:3061` call those (both crates already depend on fe-vim, and neither has its lint on yet, by the ordering above) | m | L(`crates/fe-vim`), `crates/fe-vim/src/**` (not `which_key.rs`), `crates/fe-gpui/src/vim.rs`, `crates/fe-workbench/src/git_commands.rs` | I1 | Linux-OK | 143 |
| **V2** Which-key: `BUILTIN_KEYS` as keys, `WhichKeyLabel::Builtin` resolved by fe-vim; the bundled LazyVim preset's group names by slug; user groups unchanged | s | `crates/fe-vim/src/which_key.rs`, `crates/fe-vim/src/remap.rs`, `crates/fe-vim/src/l10n.rs`, `crates/fe-vim/l10n/**`, `crates/fe-vim/l10n-allow.toml`, `crates/fe-vim/tests/**`, `crates/fastedit/src/vim_remaps.rs`, `crates/fe-gpui/src/vim/which_key.rs` | V1, T1396 | Linux-OK | 92 |
| **C1** fe-config: `setting_key`; generated, golden-checked `settings.ftl` (from the schema) and `settings-template.ftl` (blocks keyed by TOML context, commented TOML kept verbatim); `user_settings_template()` renders per locale (en-US byte for byte); all five template-writing paths call it (`persist.rs`, `open.rs`, `settings_edit.rs`, `write.rs`); kiln-shell's `persist::setting_writer` takes a template provider and calls it at write time, never at startup, and `persist.rs:65` passes `fe_config::user_settings_template` (section 4); the fake-locale tests, including creation through Open Settings and `write_setting`, the live-locale toggle test `crates/fe-workbench/tests/l10n_settings_writer.rs` (writer initialised, locale changed, file created by a toggle) and kiln-shell's provider-timing test; the settings UI looks up descriptions and group titles by key; config-problem messages | m | L(`crates/fe-config`), `crates/fe-config/src/**`, `crates/fe-config/settings-template.toml`, `crates/fe-workbench/src/persist.rs`, `crates/fe-workbench/src/persist_tests.rs`, `crates/fe-workbench/src/open.rs`, `crates/fe-workbench/src/open*_tests.rs`, `crates/fe-workbench/tests/**`, `kiln/crates/kiln-shell/src/persist.rs`, `kiln/crates/kiln-shell/tests/persist.rs` | I2 (fe-config's dependency and `settings.rs`, and `set_locale` for the toggle test); not alongside K2 (same kiln-shell `persist.rs`) | Linux-OK | 270 descriptions (249 generated) + about 125 template blocks (generated: 104 settings, 12 tables, 6 commented example tables, the intro) |
| **G1** fe-gpui: status bar cursor and selection messages as whole messages with plurals, diff view, image and file viewer, proposals, command titles, which-key's `command_title` (`which_key.rs:564–583`) reading `label()` for every registry, the typed one at `:580` included, and re-exported from fe-gpui for Z1's test; lint on for all of fe-gpui, with nothing `pending` | m | L(`crates/fe-gpui`), `crates/fe-gpui/src/**` | I1, I3, V1, V2, P21, T1396 | Linux-OK | 100 |
| **KM1** fe-keymap bundled menus: `fastedit.toml`'s titles as `title_key`; the per-platform copies `build.rs` writes; a test that every bundled title resolves to the same English as before | m | L(`crates/fe-keymap`), `crates/fe-keymap/menus/fastedit.toml`, `crates/fe-keymap/build.rs`, `crates/fe-keymap/src/**` | K1, P4, P5 | Linux-OK | 131 |
| **W1** fe-workbench commands: the doc titles in `workbench_commands.rs`, plus `find_commands`, `spell_commands`, `task_commands` and `git_commands` (`GitCommand::title()` reads the same key, which removes the duplicate); `keymaps_picker.rs` reads `label()` (`:85` typed, `:93` registry); the shortcuts editor's `commands` fills fe-keymap's `CommandInfo` from `label()` (`shortcuts_editor.rs:1276` typed, `:1288` registry) and rebuilds its model on an `ActiveLocale` change; fe-workbench's lint on, with every other file `pending` | m | L(`crates/fe-workbench`), `crates/fe-workbench/src/{workbench,find,spell,task,git}_commands.rs`, `crates/fe-workbench/src/keymaps_picker.rs`, `crates/fe-workbench/src/shortcuts_editor.rs` (its `commands` and the observer only; its strings stay `pending` for W4, which lands after W1) | I3 (and through it I2's `ActiveLocale`), V1 (`git_commands.rs`) | Linux-OK | 314 |
| **W2** fe-workbench Git: `git_*.rs` except `git_commands.rs`, plus `commit_diff_view.rs` | m | `crates/fe-workbench/src/git_*.rs`, `crates/fe-workbench/src/commit_diff_view.rs`, `crates/fe-workbench/src/*git*_tests.rs`, `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n/en-US/git.ftl`, `crates/fe-workbench/l10n-allow.toml`, `crates/fe-workbench/tests/**` | W1, P7, P10, P19, P20 | Linux-OK | 169 |
| **W3** fe-workbench navigation, search and project: `project_*`, `search_*`, `find_bar`, `nav`, `outline_*`, `symbol_picker`, `go_to_line`, `quick_open`, `harpoon`, `local_history_view`, `workbench_files`, `ignore_assist` | m | those `crates/fe-workbench/src/*.rs` and their `*_tests.rs`, `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n/en-US/navigation.ftl`, `crates/fe-workbench/l10n-allow.toml`, `crates/fe-workbench/tests/**` | W1, P6, P7, P13 | Linux-OK | 170 |
| **W4** fe-workbench setup, start, settings and shortcuts: `setup_wizard*` (including the a11y labels), `start_page*`, `settings_editor*`, `shortcuts_editor`, `encoding_picker`, `toolbar_picker`, `config_*` | m | those `crates/fe-workbench/src/*.rs` and their `*_tests.rs`, `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n/en-US/setup.ftl`, `crates/fe-workbench/l10n-allow.toml`, `crates/fe-workbench/tests/**` | W1, C1, P8, P11, P17, P25 | Linux-OK (en-US text unchanged, so the setup wizard goldens don't move) | 190 |
| **W5** fe-workbench extensions, plugins, updates and trust: `extension_*`, `extensions_picker`, `plugin_*`, `grammar_prompt`, `update_check`, `lsp_consent_prompt`, `trust` | m | those `crates/fe-workbench/src/*.rs` and their `*_tests.rs`, `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n/en-US/extensions.ftl`, `crates/fe-workbench/l10n-allow.toml`, `crates/fe-workbench/tests/**` | W1, P8 | Linux-OK | 114 |
| **W6** fe-workbench LSP, tasks and problems: `lsp*`, `refactor_preview_view`, `problems_panel` (its `count(…, "warning")` helper becomes a plural message), `task_*`, `quickfix_*`, `hierarchy_panel`, `references_panel`, `peek`, `snippet_picker` | s | those `crates/fe-workbench/src/*.rs` and their `*_tests.rs`, `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n/en-US/lsp.ftl`, `crates/fe-workbench/l10n-allow.toml`, `crates/fe-workbench/tests/**` | W1, P7, P15, P22 | Linux-OK | 82 |
| **W7** fe-workbench chat and AI: `chat_*`, `ai_*`, `inline_edit` | s | those `crates/fe-workbench/src/*.rs` and their `*_tests.rs`, `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n/en-US/chat.ftl`, `crates/fe-workbench/l10n-allow.toml`, `crates/fe-workbench/tests/**` | W1 | Linux-OK | 59 |
| **W8** fe-workbench chrome and the rest: `pane`, `workbench*`, `dock`, `status_bar`, `toast`, `terminal_*`, `vim_*`, pickers, save, open and path commands; empties the `pending` list | m | the remaining `crates/fe-workbench/src/*.rs` and their `*_tests.rs`, `crates/fe-workbench/src/l10n.rs`, `crates/fe-workbench/l10n/en-US/workbench.ftl`, `crates/fe-workbench/l10n-allow.toml`, `crates/fe-workbench/tests/**` | W1–W7, V1, P3, P11, P13, P14, P16 | Linux-OK | 108 |
| **CV1** fe-chat-view | s | L(`crates/fe-chat-view`), `crates/fe-chat-view/src/**` | I1, I3 | Linux-OK | 28 |
| **F1** crates/fastedit: CLI usage and errors (isolation off, system locale), Sparkle and speller messages, theme and grammar install messages; lint on for all of crates/fastedit | s | L(`crates/fastedit`), `crates/fastedit/src/**` | I2, I3, V2 (`vim_remaps.rs`) | Linux-OK (the macOS-only sources are linted from source on any OS) | 40 |
| **X1** The remaining crates: lint on for fe-lsp, fe-markdown-view, fe-chat, fe-chat-anthropic, fe-net, fe-nav, fe-syntax, fe-git, fe-project, fe-fileview, fe-buffer and fe-text. User-facing text moves; language and file names and git stderr matchers go on the allow-list | s | L(`crates/fe-{lsp,markdown-view,chat,chat-anthropic,net,nav,syntax,git,project,fileview,buffer,text}`), `crates/fe-{lsp,markdown-view,chat,chat-anthropic,net,nav,syntax,git,project,fileview,buffer,text}/src/**` | I1, I3 | Linux-OK | 20 |
| **Z1** Finish the switch: check that no reader of `title` on `CommandMeta`, `TypedCommand`, `CommandSpec` or fe-keymap's `CommandInfo` is left outside `label()` and the resolver (section 4): the palette at `command_palette.rs:93, 98` and the menus at `contributions.rs:1737` and `:1879` (moved by I3), which-key at `which_key.rs:568–580` (G1), the keymaps picker at `keymaps_picker.rs:85, 93` and the shortcuts editor at `shortcuts_editor.rs:1276, 1288` (W1), moving any it finds; add the cross-surface fake-locale test `crates/fe-workbench/tests/l10n_command_labels.rs`, one macro-declared command through all five surfaces (section 4); remove `Text`'s transitional `From` impls, including `From<SharedString>`, turning every remaining argument of `Picker::new` and `Button::new` into `text!` or `Text::verbatim`: each `tr!` passed to them becomes `text!` with the same key and arguments, and string literals in tests and examples become `Text::verbatim` | m | `kiln/crates/kiln-l10n/src/**`, `crates/*/src/**`, `crates/*/tests/**`, `crates/*/examples/**`, `kiln/crates/*/src/**`, `kiln/crates/*/tests/**`, `kiln/crates/*/examples/**` | every card above except B1 | Linux-OK | 0 |
| **B1** Bundle localisations (only once the user picks a second locale): `bundle-macos.sh` writes `CFBundleLocalizations` and `<name>.lproj/InfoPlist.strings` per shipped locale; a human checks AppKit's inserted menu items, the open panel and Sparkle's alert on a system set to that language | s | `scripts/bundle-macos.sh`, `crates/fastedit/macos/Info.plist`, `crates/fastedit/macos/l10n/**` | I2, the user's launch-locale choice (Q1) | Mac-only | 2 a locale |

**Total:** 24 cards (7 s, 17 m). They move about 2,280 messages, 249 of them
generated settings descriptions, plus about 125 generated template blocks.

- Every crate the spec names is covered:
  - fe-workbench: W1–W8;
  - fe-gpui: G1, plus V1/V2 for its vim files;
  - fe-editor: E1;
  - fe-vim: V1, V2;
  - fe-config: C1 (I2 adds `workbench.locale`);
  - fe-chat-view: CV1;
  - fastedit: I2, F1;
  - `kiln/crates/*`: I1 kiln-l10n, I3 kiln-command and kiln-macros, K1 kiln-core, K2 kiln-shell, kiln-platform, kiln-ui, kiln-plugin and kiln.
- Z1 closes the additive phase, and with it every kiln crate's sink types.
- fe-keymap's menus (KM1) and the smaller crates (X1) are added, because they
  carry text the spec's list doesn't name.
- **What can start at once.** I1 can start as soon as the doc is approved,
  I2 once I1 lands, and I3 once I2 does. E1, V1, CV1 and X1 need only I1 and I3. C1
  needs I2. V2 and G1 need T1396, and F1 needs I2, I3 and V2. The rest wait
  on the P-cards named above.
- **No card passes `text!` to `Picker::new` or `Button::new` before I3**
  changes their signatures.

**Decision:** 24 cards, all s or m, each building on its own through the
additive phase that Z1 closes. The infrastructure (I1–I3) lands first.
The lint is switched on per crate by each card, and per area in fe-workbench
through the `pending` list. Crates and files that T1429's P-cards rewrite
(menus, trust and plugin prompts, empty states, dock headers, palette, Git
panel and log, toast, hover, peek) migrate after those cards. B1 waits for a
launch locale.

## 10. Cost to every future card

**What a card that adds UI text must do:**

1. Add each message to its crate's `l10n/en-US/<area>.ftl`, with a
   crate-prefixed key:
   - whole sentences;
   - named arguments;
   - a plural selector for any count.
2. Call `tr!` / `text!` in code instead of writing the literal. Use
   `verbatim!` or an `l10n-allow.toml` entry, with a reason, for text that must
   stay as it is.
3. For a new command, add `command-<id>`. For a new setting, run
   `UPDATE_GOLDEN=1` to regenerate the schema, `settings.ftl` and
   `settings-template.ftl`, as today's schema golden already requires. For a
   new bundled menu item, add a `title_key`.
4. A key used from another crate must be in that crate's `## public` group.
5. Translations need nothing. Other locales fall back to en-US until a
   translator adds the key.
6. No new test is needed. The crate's l10n tests fail on a hard-coded literal
   or an unknown key. The kiln and fastedit root tests fail on an unused key. A behaviour
   test that asserts the text reads it in en-US, exactly as now.

**Overhead.**

- About a minute per message: a key and one line of `.ftl`.
- A typical UI card adds 2–10 messages, so about 5–15 minutes and a 5–20 line
  `.ftl` diff.
- The gate adds the crate's l10n tests, under a second each.
- A card that adds no UI text pays nothing.

**The card template** (the lead's card-drafter brief in the Regatta lead guide,
which isn't in this repo) gains one line for cards whose scope includes UI:

> UI text goes in `<crate>/l10n/en-US/*.ftl` (keys prefixed by crate, whole
> messages, plural selectors for counts); add that file to the scope.

**The codex reviewer's checklist** gains four checks:

- no literal moved to `verbatim!` or `l10n-allow.toml` without a reason;
- no message assembled from fragments, and no English plural helper such as
  `count(n, "warning")`, which layer 2 can't see (section 8);
- counts use selectors;
- en-US text for vim matches nvim's English.

The lead makes both changes after approval.

**Decision:** each card adds its en-US keys and uses `tr!`. The existing
per-crate tests enforce it, with no new test per card. The cost is about a
minute per message, and nothing for a card without UI text. The card template
and the reviewer's checklist each gain the lines above, which the lead adds
after approval.

## 11. Out of scope and open questions

**Out of scope:**

- **Translating anything.** This design and its 24 cards keep en-US byte-equal
  to today's strings.
- **A translation service or community workflow**: Pontoon, Weblate, Crowdin,
  `.ftl` review rules for translators, or credits.
- **Localised release notes**, the appcast or the website.
- **Localising extension contributions** (WIT `locale`, `%key%` in
  `extension.toml`, VS Code `package.nls.*.json`). That belongs to the
  extensions plan's later stage (section 4).
- **The JSON schema in other languages.** The settings template is keyed (C1)
  and renders in the user's locale once a translation ships.
- **Right-to-left mirroring of the UI layout** (dock sides, chevrons). It needs
  an RTL launch locale first, and the editor's own RTL gaps are G1–G3 of the
  non-Latin audit.

**Gaps the review found, now closed (T1506).** T1432's review left three
gaps, each of which would have kept some text English, or failed, once a second
locale shipped. T1506 amended this design to close them, and none changes
en-US:

- **G-1, the typed-command path.** `TypedCommand`, `CommandSpec` and
  fe-keymap's `CommandInfo` carried English titles that bypassed
  `CommandMeta::label()`. Section 4 now resolves all four representations
  through one kiln-command resolver, and moves every consumer to it. I3, G1
  and W1 move the consumers, and Z1 checks that none is left and adds the
  cross-surface fake-locale test.
- **G-2, the live-locale settings-template writer.**
  `kiln_shell::persist::setting_writer` captured its template at startup
  (`persist.rs:65`). Section 4 makes it call a host-owned provider at write
  time, and C1 tests a toggle that creates the file after a locale change.
- **G-3, the per-locale-bundle reference check.** The static walk accepted a
  reference that existed only in en-US, which Fluent can't resolve in another
  locale's bundle. Sections 3 and 8 now keep each bundle to its own locale and
  require every reference to resolve inside it. I1's fixtures put a term and
  an attribute that only en-US defines into an unselected branch.

**Open questions for the user:**

1. **Q1 — Launch locales.** Which languages ship first, if any, and when? B1
   and `icu_decimal` wait for this. Until then the catalogue ships `en-US`
   only, and the migration is pure refactoring.
2. **Q2 — Who translates?** A service, the community, or a paid translator.
   This decides whether `.ftl` files also need translator comments
   (`# Context: …`) from day one. The proposal: comment only ambiguous keys,
   such as "Open" as a verb or an adjective.
3. **Q3 — Should `workbench.locale` also set AppKit's language for the next
   launch?** That would mean writing `AppleLanguages` into Tenmoku's defaults
   domain, so AppKit and Sparkle follow the override after a restart. The
   proposal is no: point to System Settings' per-app language, which macOS
   already offers.
4. **Q4 — Locale-aware dates** in the Git log and Local History (`icu_datetime`,
   Unicode-3.0, more data). Should they come with the first non-English
   locale, or later?
5. **Q5 — A separate command `category` field** ("Git", "Harpoon"), for
   grouping the palette and menus, instead of "Git: …" inside each title. It
   isn't needed for translation, so it would be a palette design card.
