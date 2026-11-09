# Grammar registry index (T325)

Card 1 of the grammar registry (Mac T1481). Grammar source stays in
`extensions/grammars/<language>/`; built artifacts and a signed index will go
to the public repository `tenmoku-editor/registry`. This card builds them
locally: nothing is signed or published, no workflow changes, and the app
doesn't read the index yet. `pack.toml` and `grammar_install.rs` are
unchanged, so the app keeps working from `pack.toml` until card 4.

## What's built

`scripts/build-grammar-wasm.sh`, then `scripts/build-grammar-registry.sh`,
writes `target/registry/`:

- `languages/<language>/`: the language's artifact, exactly the files its
  index entry lists;
- `index.json`: the index, below;
- `packs/Tenmoku-language-pack-<pack>.tar.gz` for each pack in the index, and
  `Tenmoku-language-pack.tar.gz` with every language, each with its
  `.sha256`.

The generator is the new crate `crates/fe-registry` (lib, and the bin
`grammar-registry`), on serde, serde_json, toml, sha2 and thiserror only, all
already in Cargo.lock. Its types are in `fe_registry::grammars`, so plugin
entries can sit beside them later. The card 2 signer and the card 3 app parse
with the same types.

The registry script runs `grammar-registry generate`, packs each pack with
`scripts/package-language-pack.sh --pack <pack>` (the pack's languages come from
the generated index, through `grammar-registry packs --index <file> <pack>`),
packs every language with no `--pack`, as before, then runs `grammar-registry
finalize`. That fills in each pack's `tarball` and rewrites `index.json`. A
separate finalize step was the simplest deterministic choice: the index
without tarballs is written first, and the tarballs don't contain the index,
so there's no cycle.

## Index schema 1

The shape is the spec's:

```json
{
  "schema": 1,
  "serial": 1762430400,
  "url_template": "https://github.com/tenmoku-editor/registry/releases/download/{tag}/{file}",
  "packs": {
    "jvm": { "name": "JVM", "description": "Kotlin, Scala, Groovy and friends",
             "languages": ["kotlin", "scala"],
             "tarball": { "name": "Tenmoku-language-pack-jvm.tar.gz", "size": 123, "sha256": "…" } }
  },
  "languages": {
    "kotlin": {
      "name": "Kotlin", "version": "0.4.1",
      "tag": "kotlin-<first 16 hex of digest>", "digest": "<64 hex>",
      "extensions": ["kt", "kts"], "file_names": [], "shebangs": [],
      "packs": ["jvm", "mobile"],
      "license": "MIT AND Apache-2.0",
      "engine": { "tenmoku": "1.0", "abi": 14, "captures": 1 },
      "files": [ { "name": "LICENSE", "size": 1, "sha256": "…" }, … ]
    }
  }
}
```

- `files` lists every file of the artifact, sorted by name in byte order, so
  `LICENSE`, `LICENSE-*` and `NOTICE` come before `extension.toml`: the wasm,
  `extension.toml`, the two queries the manifest names, `LICENSE` (which must
  exist), any `LICENSE-*` and any `NOTICE`. Never `grammar-source.toml`,
  `test/` or any other file. The names are plain: no `/`, `\` or leading `.`.
- `digest` is SHA-256 over `<name>\t<size>\t<sha256>\n` for each file, in
  order. `tag` is `<language>-<digest[..16]>`.
- `name` is the manifest's `name` without a trailing " syntax". `version`,
  `license`, `extensions`, `file_names`, `shebangs` and `packs` come from
  `extension.toml`, and `engine.tenmoku` from its `[engine] tenmoku`.
  `engine.captures` is `fe_registry::grammars::CAPTURES` (1). Its doc comment
  says to bump it only when an existing capture name's meaning changes in
  `crates/fe-syntax/src/highlight.rs`.
- `serial` is the caller's (`--serial`; the script uses `REGISTRY_SERIAL`, else
  HEAD's committer time).
- The types are `Deserialize` with `deny_unknown_fields`. `Index::parse`
  checks `schema` before anything else, so a schema 2 index is refused as
  such, not as an unknown key. It also checks what the types can't: plain
  file names, SHA-256 hex, each digest and tag matching its files, and packs
  and languages listing each other. The app writes these names to disk, so the
  parser refuses a `../` name rather than leaving that check to card 3.

Choices the spec left open, or small deviations from it:

- **Keys and arrays.** Map keys (`packs`, `languages`) are sorted, and so are
  `files` and each pack's `languages`. A struct's own keys come in the spec's
  order, not alphabetically, which is still the same every run. A language's
  `packs` keep the manifest's order, because the first is the main pack.
  `extensions`, `file_names` and `shebangs` also keep the manifest's order:
  they're deterministic as they are.
- **Empty packs aren't in the index.** `registry.toml` defines 14 packs, and
  the eight languages fill eight of them. A pack no language lists is left
  out of `packs`, since its tarball would be empty. Once the sweep card gives
  the other languages packs, the rest appear.
- **`tarball` is optional in the type.** `generate` writes the index without
  tarballs, and `finalize` adds them. The serializer skips a missing
  `tarball`, and the parser accepts one, so the intermediate index parses too.
  Every published index has them, so card 3 can treat a missing tarball as
  "no offline pack".
- **The all-languages tarball isn't in the index.** The schema has per-pack
  tarballs only. `Tenmoku-language-pack.tar.gz` is still built, for an
  offline user who wants everything.

## Why the tags are content-addressed

A tag is the language's name and the first 16 hex digits of the digest over
its files. A change to any file of the artifact (the wasm, a query, the
manifest or a licence file) gives a new tag, so it's a new release.
`grammar_index.rs`'s `digest_and_tag_change_with_any_file` checks each file
in turn. A published release is never replaced: the app checks every file
against the index's hashes, and those hashes never change under a URL it has
already seen. An unchanged language keeps its tag, so a registry publish
uploads nothing for it, and nobody has to bump a version for the release to
move. The `version` field is shown to users, nothing more.

## The ABI

`scripts/build-grammar-wasm.sh` already has each grammar's `parser.c`. It
records that file's `#define LANGUAGE_VERSION <n>` as
`target/grammar-wasm/<wasm>.abi`, one line holding the number, before it
moves the module into place. If `parser.c` has no such line, the build fails.
The generator refuses a language whose `.abi` is missing or isn't one line
holding a number (`missing_or_bad_abi_is_refused`).

`LANGUAGE_VERSION` is what the generated `TSLanguage` struct's `abi_version`
field is set to (`.abi_version = LANGUAGE_VERSION` in `parser.c`), and
`Language::abi_version()` reads that field. To check that they agree, I added
a temporary test to fe-syntax, ran it on the built `target/grammar-wasm/` and
then deleted it. It loaded each module through `tree_sitter::WasmStore::load_language`,
as fe-syntax's runtime does, and compared `abi_version()` with the `.abi`
file. All 19 matched (2026-11-08, Linux x86_64, wasi-sdk 34):

| Language | `.abi` | `abi_version()` |
|---|---|---|
| cpp | 14 | 14 |
| csharp | 15 | 15 |
| kotlin | 14 | 14 |
| php | 15 | 15 |
| ruby | 14 | 14 |
| scala | 15 | 15 |
| sql | 14 | 14 |
| swift | 15 | 15 |

The other 11 matched too: clojure, elixir and r are 14; dart, erlang,
fsharp, haskell, ocaml, perl, powershell and zig are 15.

## Packs

`extensions/grammars/registry.toml` defines the packs by name and
description, along with `url_template` and `index_url`. Card 3 compiles it
in, which is why the URLs live there. Card 2 adds `public_key`. Membership
lives only in each language's `[[contributes.grammars]] packs`, which is
optional and empty by default. `ExtensionGrammar` in
`crates/fastedit/src/config.rs` has the field, so `deny_unknown_fields` and
`every_first_party_manifest_parses` keep holding. The loader ignores it. The
first pack listed is the language's main pack. A pack `registry.toml` doesn't
define is refused. A language in no pack is a warning, because the ~46
language cards landing beside this one may not add the key.

The packs: shells, web, infra, functional, jvm, dotnet, systems, mobile, data,
docs, shaders, hardware, config, scripting. The eight languages:

| Language | packs |
|---|---|
| kotlin | jvm, mobile |
| scala | jvm, functional |
| csharp | dotnet |
| swift | mobile |
| cpp | systems |
| sql | data |
| php | web |
| ruby | web, scripting |

## The tarballs

Each tarball unpacks to `tenmoku.grammar_<language>/`, holding the
language's artifact files. That now includes its licence files, so the
all-languages pack gains `LICENSE` and any `LICENSE-*` and `NOTICE` for each
language. `language_pack.rs` now expects them. The installer and the
loader read only the files they know, so the extra files change nothing for
an install from an unpacked pack (`install_from_pack_dir` reads the wasm
alone).

The archive is the same for the same inputs with GNU tar and with BSD tar,
which macOS has: the first review found that BSD tar's branch kept the
stage's real times. Neither tar's own options are relied on now. The script
sets the times (2000-01-01 00:00 UTC) and modes (755 and 644) in the stage,
with `TZ=UTC` and `COPYFILE_DISABLE=1`, lists the names sorted with
`--no-recursion`, and writes ustar, which keeps no access or change times,
extended attributes or ACLs. Owner and group are 0, with no names.
`language_pack_per_pack.rs`'s `same_inputs_give_identical_tarballs` packs twice,
a second apart, under umasks 022 and 077 and two time zones, and compares the
bytes. In this lane that runs GNU tar; on the Mac it runs BSD tar. An empty
`--pack ""` is refused rather than taken as no `--pack`.

`package-language-pack.sh` copies a language's files from
`extensions/grammars/` (`GRAMMARS_DIR`) and `GRAMMAR_WASM_DIR`, by the same
rule as the generator, rather than from `target/registry/languages/`. That
way the no-`--pack` run keeps working as `grammars.yml` runs it today, with
no index. `--pack` takes the pack's languages from the index
(`GRAMMAR_INDEX`, default `target/registry/index.json`), read through
`GRAMMAR_REGISTRY`, which defaults to a `cargo run` of the bin. An unknown
pack fails naming it. `language_pack_per_pack.rs` runs the script on a
fixture grammars directory with an index the bin generates.

## Reproducible modules

The first two-run check found every `.wasm` different between runs. That
made every tag, the index and every tarball differ too. wasm-ld writes the
output file's name into the module, and `build-grammar-wasm.sh` linked to
`<wasm>.$$.tmp`, a name holding the shell's PID. It now links under the
module's own name in a temporary directory, `target/grammar-wasm/.<wasm>.XXXXXX/`,
then moves the file into place. With that fix, the same sources give the same
bytes. A content-addressed tag needs this: without it, every CI build would
publish every language again. It also means a module built by
`grammars.yml` before this change won't match a rebuild's hash. `pack.toml`
records only the released files, so nothing that's recorded changes.

## Evidence (C3)

C3 is the Linux determinism check: this lane runs on Linux (x86_64, GNU
tar 1.35). The two-run check with macOS's BSD tar is deferred to a follow-up
on the Mac and isn't part of this card (the respec of 2026-11-09). Until
then, `same_inputs_give_identical_tarballs` covers the BSD tar path whenever
the tests run on the Mac.

The run below is on the integrated candidate: commit aa187ffd (committer
time 1794249569, which is the serial), the lane rebased onto main at the
workspace's 0.1.2, on 2026-11-09. Each run starts from a clean
`target/grammar-wasm/`:

```sh
for i in 1 2; do
  scripts/build-grammar-wasm.sh
  scripts/build-grammar-registry.sh
  (cd target/registry && find . -type f | LC_ALL=C sort | xargs sha256sum) >run$i.sha
done
cmp run1.sha run2.sha && echo IDENTICAL
```

In both runs, `build-grammar-wasm.sh` built all 55 modules for the 56
languages (ERB and EJS share one), then its parity test failed on 4 of 56
languages: astro, ejs, svelte and vue. Their goldens predate main's T1649
change, which highlights the embedded JavaScript's object keys as
`string.special.key` where the goldens say `property`. This card changes
none of those files, nor fe-syntax. The test runs after every module is
built, so the registry build went ahead on the full set.

Both runs wrote 312 files:

- 56 artifact directories (the eight with packs have 5 files each, and
  Kotlin has 6 with `LICENSE-APACHE`);
- `index.json` (schema 1, 56 languages, 8 packs);
- 9 tarballs with their `.sha256` files.

`cmp` printed `IDENTICAL`, and the two listings hash the same:

```text
0981a74cf5d23998c78085380e622225186c510b8a9baf7bc3062fc13c72ea8e  run1.sha
0981a74cf5d23998c78085380e622225186c510b8a9baf7bc3062fc13c72ea8e  run2.sha
```

Run 1's listing, the same as run 2's:

```text
4dc5a7f705c9eb9f931b9d1900c2f282d1addf53a574579c03240e855e8620a2  ./index.json
9ec95a90150fa19e2cee78faf49138fc480f6f7a2354006a0ab150c44239447e  ./languages/astro/LICENSE
c1aefa040a884a393152576b8360d700b73e40ea64c8044d155afc9bdb991031  ./languages/astro/extension.toml
ffe276824ff4e1e1c7f471c8d246585d6120f1d48f6df709fa98a76380fceae8  ./languages/astro/highlights.scm
e454c59fb808c594ce0026a86c47c3e5d61c384ec8952fcfb27a45eb3656895f  ./languages/astro/injections.scm
5a2fb8768d5e1d1dfe39c7a96a68a53000b64558cb16d2a06e93c01868376e47  ./languages/astro/tree-sitter-astro.wasm
512d5f9072b6227e356c765b40a4194fda964f75095b0816f24ce75aa8df03fa  ./languages/awk/LICENSE
db970526f273b6634dde245530fc273369d74b406dc169c7a85a247b34ce1607  ./languages/awk/extension.toml
c0de09335da85d9c939b6168c0765f4161133218afd3e0b4ebee6225b5897d4c  ./languages/awk/highlights.scm
34999105fcfaa7b31d41b547f0f99ca86c96a0bd9aa1679c7c42aa9777b6550a  ./languages/awk/injections.scm
2702683f99b6c3cd77a0d0240c7b2974c4171c5c60d26327568e83001d0d798c  ./languages/awk/tree-sitter-awk.wasm
96e0c1803ffb831c7287b1fb7f88567d05e58d37da553e6bb9a9c4a765434ecb  ./languages/blade/LICENSE
c0bdd90d5f69d046b7fdec009f4b9762b63106ceccfae7d2ff07cdf6951c38cf  ./languages/blade/extension.toml
489dafc5343f9def2652035c9b507683d98c5668c63407f33e79e2f9f173f145  ./languages/blade/highlights.scm
2fd6f76c4c18c04a43cc905d87330ffe458106b79fa71510534db5ca5fb34b8f  ./languages/blade/injections.scm
b63488cd57db7c33e4e25cebb48a3e9d7f07512cb6e234054d28b222b8b010fd  ./languages/blade/tree-sitter-blade.wasm
a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499  ./languages/clojure/LICENSE
6a1040101b598875bb8605d63d5e7a40c3aceb8aff76e6f6cec15bbfd1ecf502  ./languages/clojure/extension.toml
3901bb140af6dd026771a6d14c993d0d83671a460100b68a51b8fc8ebc6e4a33  ./languages/clojure/highlights.scm
b06b36a5404b734f55ae280012bf8881f28390298f948b06e905eafe0734a919  ./languages/clojure/injections.scm
86bc78e2f401f373ca4d82cc75975d2baf246b4945a4eb9eee4514195bc4d2ee  ./languages/clojure/tree-sitter-clojure.wasm
97334c0f9c0962b33cbe03223f584b8ed64c2d14419ce80cc2e945eb8d367d28  ./languages/cmake/LICENSE
f2ff9ec7d94a94c3dc7af9d387dbe87732392e26d8a5e6a84a2f73939bfc5f40  ./languages/cmake/extension.toml
f6b574615c9eac3b925cb4354c29101ea5930857591c5629a437387ee102aeed  ./languages/cmake/highlights.scm
ac596f9afbb98946caf788d166d45772e1de9a6b3e1de0a1eb90fdb91c205bc4  ./languages/cmake/injections.scm
76be696089b6cfaee1bd50fde5b9883e1fdde69e96a31a77642326b7e954aae5  ./languages/cmake/tree-sitter-cmake.wasm
2e0110e07abef7c2548b26ec9d6969775617ca539a0dc8dbeeb14d6452c711d1  ./languages/cpp/LICENSE
300ee4288f0d901ed0800c2bed837aafd3c267746339dd05f069b4ae204a8448  ./languages/cpp/extension.toml
4301cfaa699e55984b6f9106c2aaa6477066b634fe8beb7387bf1c73a26ecdc3  ./languages/cpp/highlights.scm
b738d12eb8f04d14d7454d2cf874e070620f6f074c4af9559cf8fd507c0ec814  ./languages/cpp/injections.scm
20129669e8bb9a2f69ec1bbbbd2d04395dff936f8704cbf72532627423e7dbb1  ./languages/cpp/tree-sitter-cpp.wasm
778fb7d63b8c1844da315648c02f325c4713a6f4a5d19644fd413421422776d3  ./languages/csharp/LICENSE
303055bb6ffc14755ff2caf87e15d1a97caf93d8de5c088459ee212c15752be6  ./languages/csharp/extension.toml
ab8a9930aeeee70fa2dbfde82e4763170b7e826bc642338ad0683772c20c060f  ./languages/csharp/highlights.scm
f106338d536801b0838cbcd173813f01fc086cca3736938024dd1e81381adda6  ./languages/csharp/injections.scm
4367df4bff295a670c1d64b18db4217648d6c9789408db9275e909edcaa08d64  ./languages/csharp/tree-sitter-c-sharp.wasm
ecf206b1a27e8e4ea07f1e1529cb111b4be5ce8420bd27b8269981d9c0c389f8  ./languages/d/LICENSE
c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4  ./languages/d/LICENSE-APACHE
b4ca05d0b0104835123c7faa89fce708bf5dcf516f216eaaeb3cf1138e656609  ./languages/d/extension.toml
a4c63d0e4cf55e284ad8c2489505018190dd558dd0c47db7534f89f99f333d74  ./languages/d/highlights.scm
2978c30a90acc34497e21dfda6963f22af79d57d949d2f872f9f4a725c62a31f  ./languages/d/injections.scm
d0983a2e574bfd014bf12d15cb7fb7a3376e3cdf1f8f38a3da844aa70b54adfb  ./languages/d/tree-sitter-d.wasm
d270cb3a4985d75033bd77d875ccebff1d66e32788a3f727891e28d76132dd46  ./languages/dart/LICENSE
a70a04ebb286d72b3606dda7939606c4183da58a33b95790f80256cef5b639f1  ./languages/dart/extension.toml
a5f4b5c9986fa22b4fcc9b70a5aef7e24ab4363926e12fa8bc0fb1b3b4f3640a  ./languages/dart/highlights.scm
963efa4924a90013ae878a0ad19f177cb07129040eed4a37ecf7fddb40117a6f  ./languages/dart/injections.scm
824e5c81a8c0208d5a8deaeafabf47b53652372d9761608364d4e67a3cba67fa  ./languages/dart/tree-sitter-dart.wasm
c5cfb43042b6b72045f4ba997834d0a7786d2793d91680868b5815b39f14fc78  ./languages/ejs/LICENSE
40be1df6705ab9384b44c62f2240d0e2dc4ef5c92d27e5fb6dc84ff13fc13048  ./languages/ejs/extension.toml
b8f95f416ea17d755acf4a695b153ba997332cc8ed18c4ba8f1c0da89b3bd3b5  ./languages/ejs/highlights.scm
fb5fb124e719d107447c70773c249064af7bcc5322cc4f679b5b4dfab4a20a77  ./languages/ejs/injections.scm
e7a8caec5355d1e9394b39efeb5acb6ec5b674590369084b874f0cd96d539212  ./languages/ejs/tree-sitter-embedded-template.wasm
a6cba85bc92e0cff7a450b1d873c0eaa2e9fc96bf472df0247a26bec77bf3ff9  ./languages/elixir/LICENSE
119588423fbaa46a6955a044fee5a23338c7de190d8c4651a94c6d0b98ca2cda  ./languages/elixir/NOTICE
e949d1ab37404e1fcfc35ffcd67968c36f0f1917ce892dcd12c91ca52026b87d  ./languages/elixir/extension.toml
72d83a4dbeb10824086bd7216a5020351286bee53f2970e3f7e1c536f691d1f9  ./languages/elixir/highlights.scm
aca1ea5381d136bfe46586288c8f6b95618636d105e65b8adc6cc9d2dc313b26  ./languages/elixir/injections.scm
c7c8f489f6b6c1fc984bf4312a41e0ae3e74f963df8b5eeba3efb07814254336  ./languages/elixir/tree-sitter-elixir.wasm
26f7bf09693dd895ea64777316bad0133b131a8478a660b4785d4b9d1322f9a1  ./languages/elm/LICENSE
0eed3a4e59df71a29b7f4566e10629cf1aa5c354707dd1426dc89f9f5f624897  ./languages/elm/extension.toml
04073e9e4a06bfee5dfce2b501344ec23b0c3db29bc42be57d4ec4d4cc415c38  ./languages/elm/highlights.scm
b008498037f2d0aa2e9ef77d7181f1f9a0be9a73781858edc9b1a8a1b7b14a2a  ./languages/elm/injections.scm
6a865e1fb9f0ad36701d05584bbe06333f7fb86cbbd1252e26936e6ba277d1bb  ./languages/elm/tree-sitter-elm.wasm
c5cfb43042b6b72045f4ba997834d0a7786d2793d91680868b5815b39f14fc78  ./languages/erb/LICENSE
4cd43dbe17f596994313128067897ae044d4a5eb839a1a57dd5d8a7f61b16f97  ./languages/erb/extension.toml
b8f95f416ea17d755acf4a695b153ba997332cc8ed18c4ba8f1c0da89b3bd3b5  ./languages/erb/highlights.scm
de8cefde8eb6e9d1f55ae6d029189fe2244f5e6a74748a3876681fcc0aa23011  ./languages/erb/injections.scm
e7a8caec5355d1e9394b39efeb5acb6ec5b674590369084b874f0cd96d539212  ./languages/erb/tree-sitter-embedded-template.wasm
c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4  ./languages/erlang/LICENSE
35f77e2833f79d5527d3d9f247d90c41fa0fe1a10a7c6dcda001a20b6b2f3652  ./languages/erlang/extension.toml
c2fc0904d0c19a3073cc7840a49e69c37cbd98a55c33fbba471aaef8e1bf7e86  ./languages/erlang/highlights.scm
b06b36a5404b734f55ae280012bf8881f28390298f948b06e905eafe0734a919  ./languages/erlang/injections.scm
d8ed8e25f6c894230ea56828de8af85a814e3e371dc207ca9454d0e57132d2ef  ./languages/erlang/tree-sitter-erlang.wasm
6b0382b16279f26ff69014300541967a356a666eb0b91b422f6862f6b7dad17e  ./languages/fish/LICENSE
28b7516a63328f076857681561f9b6efbcdb729bf079c994d97977e9ab41ec18  ./languages/fish/extension.toml
053707f2a2158772b2c2392107244c2815bec9e3e8cafc09aef0c90b0e8841bb  ./languages/fish/highlights.scm
d50d0bc716833dddb48fa22e77d38c96ad514baf4aea88826390c371315418ea  ./languages/fish/injections.scm
fd67720f9b365ceee387e0f7a7dc5f81cef7cb79a6869a2a3e411b8eb94bbabf  ./languages/fish/tree-sitter-fish.wasm
c0fc02a86ae2b179c4729694c523bbf42da674e5136ed8e12e3cb2f26556ea9f  ./languages/fortran/LICENSE
490d7240c3ec2b14aa3afaef3c1218d1f513baaca02c832c0e86884014931f54  ./languages/fortran/extension.toml
db892f2cfc9c97d6664f63112c1c5e7f3a982775ad0568c7084a988bd73580a1  ./languages/fortran/highlights.scm
c838eb88558c4032c727e0249436324432836592f1bb78ec9353c0487e2539ce  ./languages/fortran/injections.scm
1f72c6c413ac48eee64ef3f115b0458c4c324b4989924cbddef70415fccaf63d  ./languages/fortran/tree-sitter-fortran.wasm
f9352b63e87252b282b252bf27530027c8ca69829e0023f8b8b682a736f271fd  ./languages/fsharp/LICENSE
be0d410e135942241b326f7eb663c301099bf999b326cc107e860bd08a172553  ./languages/fsharp/extension.toml
327a26f010c8c274092dc8cb3047a4812142e1b9b398ccb82c698f3560a8f02f  ./languages/fsharp/highlights.scm
dfb1c27a0bf14911e605fa8a9133369d8a42421a51d2d13c0951bc372da4801c  ./languages/fsharp/injections.scm
2655d5b2b43eb6ee04533e5ebc179e6032e73075cde6b86bea0a3f803545fbf1  ./languages/fsharp/tree-sitter-fsharp.wasm
cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30  ./languages/gleam/LICENSE
b053ff5356a6b2c6970542388bc4fd63e670d281867cd7fd232a3547d9cd90bb  ./languages/gleam/extension.toml
0fe6a57af70a9f3b2c4fb210647b116595648121d2dc8f3f995adb77050232fd  ./languages/gleam/highlights.scm
e5ec9bdf27e08ac6e7acf5d8b49747ce9e6ad9ff975b4fd69b1e1efeb5cf7856  ./languages/gleam/injections.scm
dd4138dce003c9f008bfcf0bae2bcfc68337ebcbf0b8e971939ea1add6561c1f  ./languages/gleam/tree-sitter-gleam.wasm
806b8fcd19c1cdf23e178eb25942acc844a38669e36b2c896c7baf5f8bcecbb4  ./languages/gomod/LICENSE
b4703ad5fa110f2a0fd3642c82b5bbd59238905941e295f3fc72bc93aa85283d  ./languages/gomod/extension.toml
1107b38d2754cf8c902f42c579e2cf8b3d1d850c9cf17190f599378e62bdde08  ./languages/gomod/highlights.scm
8d76186494c71453c368d2ed491c4b02b2ae24ca2a69cf0efa77bde9f6b8c414  ./languages/gomod/injections.scm
527f6d7641438143fa5edbce43d52288fc14fca7e06c97a38c73ab1b01eb510d  ./languages/gomod/tree-sitter-gomod-orchard.wasm
099c44248f8cf353123211318680e93465587c005a4b3730ce8cb5334de043d6  ./languages/gosum/LICENSE
735527bd0ab4b7903f7cad761c0f5e8c3c1edf40e6b97e187b8cfda799e284be  ./languages/gosum/extension.toml
7184274210f23c3a396f5e41d39ec0ff609e891ccc56b109f531b67a7c482771  ./languages/gosum/highlights.scm
a88a2a945063e5a2a6038ef7f947c61c74dff1ac764c1970a3accde177fec088  ./languages/gosum/injections.scm
489e34b3079d9fdb3bf6072f3c448b8a410ebee0ed020309ed6806050cc09ba1  ./languages/gosum/tree-sitter-gosum.wasm
910d2cfec76df90457dd9ff08abb273c8e52348b2e04ffd80e10cf89b912a553  ./languages/gowork/LICENSE
b8db50221fcb61c1420bd8e2afe6684b085b011c88e86521ca67cf4c96606656  ./languages/gowork/extension.toml
27db1c11e6d214431795389825ddb9884bdac1a7b87aa68c99490ceb44cada1f  ./languages/gowork/highlights.scm
c183540e626209d19b84ab261762a83e6ef6313bcd1b798a5f21956d8515d6af  ./languages/gowork/injections.scm
b5b8bb9e672be5b07ba24b001bb748879084421c15769c039633fa62530e5a9a  ./languages/gowork/tree-sitter-gowork.wasm
7bd2788c7154b102cf03eeb335faba32d897fb82f252b28663c16c76738aaba5  ./languages/graphql/LICENSE
dda7680b81c150682f322a124b21d604d54931edff6da16fb74a31ec76f6c844  ./languages/graphql/extension.toml
408e6734b129d247514d07ae1da431ab65ca25505d20bf5bfd46d1b7559d6e1c  ./languages/graphql/highlights.scm
f5da78d06f12587ee4bf59a2b9a688090662b44b1dd201779add699897280c45  ./languages/graphql/injections.scm
41daf134d1416e12292bafe347559581c543ed91b6462c5cb989266e4076783f  ./languages/graphql/tree-sitter-graphql.wasm
5f10fff4ea3c4892af1e2f1af0b9ba7fa79843598808a1dbbbbc8231ca81d1ad  ./languages/groovy/LICENSE
d75a2884758df3c738e7d3911d6a759a6e51ea41f5e2f4dfdedf3d5db55be43d  ./languages/groovy/extension.toml
628cf9cb2255f9656c32c2d44466bc38adc85e851d62247b862b96166fef6d8d  ./languages/groovy/highlights.scm
3c92b04cc357c71cba29f540fe2f96bc58cb2994e7afcc681581cc580fdd06a7  ./languages/groovy/injections.scm
c210cb7f29b6514c714c05c60c97b460df63f8e8581d47bf631a845b998104bf  ./languages/groovy/tree-sitter-groovy.wasm
2e0110e07abef7c2548b26ec9d6969775617ca539a0dc8dbeeb14d6452c711d1  ./languages/haskell/LICENSE
5a9f79af945bd2135806304c32284356197775d2724699e058c5a2cd0dd9e24a  ./languages/haskell/extension.toml
8e7ca3c2f69dc9d1056ddf9e2901bcc9b5e3e29da190626015c33950cb0a00ab  ./languages/haskell/highlights.scm
4ae628d072f6d73fb8df10bd01094437bbe566bc03797161426978ea861ae9de  ./languages/haskell/injections.scm
8a370a41aec73c9bf299784fd6cc2a6efa3c9d91aa9959f7bad3ac9d568b8d5f  ./languages/haskell/tree-sitter-haskell.wasm
c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4  ./languages/hcl/LICENSE
074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff  ./languages/hcl/LICENSE-APACHE
4736bc330788e3aecc49422c141b83517399bd52ac07214c84f6cf51128a160c  ./languages/hcl/extension.toml
6e6235e2bd4f7daf31a902a0869368644be6472ce810f1f7a0a46dd9b2487434  ./languages/hcl/highlights.scm
8a07b208848c5dc8d45331f9f010a08b37c3d5a5c8162d1bf222266e5158195c  ./languages/hcl/injections.scm
8a1d8338ce7472547d1bbbc3a1e5206423988efdcea4ce9e20288c7dc78637a9  ./languages/hcl/tree-sitter-hcl.wasm
dec735ccc031754dd05e6556dd9e959915eb367cf01cf009c77a1a38786932b1  ./languages/hlsl/LICENSE
c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4  ./languages/hlsl/LICENSE-APACHE
9c910f97c0dc03b1cf0d402fd53e6322bd058252eea94186f4f67772c226e291  ./languages/hlsl/extension.toml
adf0e314a1da54a14338a32694a84b74fd415746c4f3dd01eb0d431166cb2f08  ./languages/hlsl/highlights.scm
fb6206189b65c2e4f85dfa196889c00fc577f63a7d3af9665ca0f1f12e66bbdd  ./languages/hlsl/injections.scm
0a35284ef0a8e7d167259cc87593125bd3886b89b9fdbb7f1d2c24add921a851  ./languages/hlsl/tree-sitter-hlsl.wasm
74f800107f200f4e28da33e9c3f86230d37d8f4c0a4916f7111e800c395ee3a2  ./languages/julia/LICENSE
6c0904a117cf7222a1159fe719af4d7f2ff289f421e2e1e79a8931365dae0849  ./languages/julia/extension.toml
18b8dd94acce1c2faeda2ff659216c1a6e97d3e93d3b7c7779e8b35f8e6af2de  ./languages/julia/highlights.scm
3e41a78c747359ac3d9f65a08767c91fabe3a804973625abd0acfa73de9f0ce1  ./languages/julia/injections.scm
6b8723dadca806051f32c2397769bfbe5063bbd1e35fa7d898c82927adc47ad4  ./languages/julia/tree-sitter-julia.wasm
8159dc4e5ec52ee8ad940e2faa687d8b045b672d34102adb644571477f63e347  ./languages/kdl/LICENSE
89f6e510fc2fd09d65a6a4880f4f6064da917d34c4373863145b8df99e5627cb  ./languages/kdl/extension.toml
6c3e2d6fd8073059899aa2022723d57004c0a4267c3bd2f9ce2ef3ca29088953  ./languages/kdl/highlights.scm
92cdbd9a4ea6bc4713b23a316bb726ff53e5e7776b85f4082067d96f74baecbb  ./languages/kdl/injections.scm
77270a1bd4afe7eb988cfeab7e916e36c6520e25ad7f77ffff45ef4f3d47d3ec  ./languages/kdl/tree-sitter-kdl.wasm
948495f61768f7de26bcc61113d8cd95f50bbc15adb678c28c941c6c8fcd5903  ./languages/kotlin/LICENSE
074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff  ./languages/kotlin/LICENSE-APACHE
8a39f4395faa3a28ff2008d8aaf7bf6c07d14637df96fd351441149abfa586f8  ./languages/kotlin/extension.toml
53f71edf120a3ad1d82da7d408a02c91acd14dba200e4d02f6fb0f674d002f46  ./languages/kotlin/highlights.scm
5cd8ed9255bbac7b1ebe792545f45d615ce413968c77e7e0f051cd69fc20ae21  ./languages/kotlin/injections.scm
ff41d07736ef1b22def69557add54167c07158bac2b0203af97c6671def0a589  ./languages/kotlin/tree-sitter-kotlin.wasm
7174d0c27fa92745f89c7e64bdf3cdc94eacac77915cad97790d993cdc24a010  ./languages/latex/LICENSE
c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4  ./languages/latex/LICENSE-APACHE
f95e79ee79be0d553f9b2ed671e63907e1c4141714e09fd7258f43f2fd442bdb  ./languages/latex/extension.toml
524cc114457ae8ea2903503347fc81dfb34c2d35fd11d7edf62e7810faa4209f  ./languages/latex/highlights.scm
f91b85e987b8842993f34d85eda2212058763878b6aa51e00e2037efe40950b8  ./languages/latex/injections.scm
3fab859773370c8870a67ffe5f4ff5320b90c1c9c421bf23a33058220770c6d7  ./languages/latex/tree-sitter-latex.wasm
5edebab3318dc8aabfaf032f62aec7c4a597745948102f5c4a4ab0e1dd218c72  ./languages/nginx/LICENSE
2a04fbf7e1cd312446e38941e6efad4bf6174fa4b0557c2d5202f181c18ac579  ./languages/nginx/extension.toml
e95cf27132065ebc60d6f8382866c25e26d247e32aae6775acf8fa3e7a59f079  ./languages/nginx/highlights.scm
07f836b8d09b1a55287b8b6a3c8fcc1d1ee9dde4a8e6473a1c931ab23501f0f6  ./languages/nginx/injections.scm
9368eb2119328be9757a0a1c2ffdcf72b0545948d3e6f071f1c00b1088239c6e  ./languages/nginx/tree-sitter-nginx.wasm
c1b72e50266464ec5118d26200e17bf1f472d142e62ab637e1ffc23986657a78  ./languages/nix/LICENSE
30e2e50e652be22ad8a3a700cebb570d86ee1ac2ffe570c0065938b025fc9246  ./languages/nix/extension.toml
9570e63a7cd09d10456241f7bc01c48b247aa5ff200c6bc0a12684128fb45a46  ./languages/nix/highlights.scm
b8899cc47c04632abea5858b6b432e623817dc5b98db65556fa8ba1d1b4df0cb  ./languages/nix/injections.scm
e8e23db3384a341ddab82307aaf9d2001df3c2d81843d4af29d6f08e857def7f  ./languages/nix/tree-sitter-nix.wasm
099c44248f8cf353123211318680e93465587c005a4b3730ce8cb5334de043d6  ./languages/objc/LICENSE
2e0110e07abef7c2548b26ec9d6969775617ca539a0dc8dbeeb14d6452c711d1  ./languages/objc/LICENSE-TREE-SITTER-C
465c45946a887a9e7608fb8f22b1fdc7ced101f64ce15e4eec72d54663bd37b7  ./languages/objc/extension.toml
1411c1ba85a44ec1c63e939e283d1f40748f74c15a02d36d40150fc5c8b2192f  ./languages/objc/highlights.scm
ce25a2f8f01bc48a1e89c21d1e032ab6a4795c66b9fb01f1d59e75ec55d796c1  ./languages/objc/injections.scm
b0d08c296e9d316ee1baef70c8383e2e8ced955d0ac8883356b4eb51c62f9b78  ./languages/objc/tree-sitter-objc.wasm
f977a20805dd6e275f14bc090a5f8e638401dad4df366841c1060f12c75b74ae  ./languages/ocaml/LICENSE
40c331f2dd7b0330b475c04953a3368c946e1a8abbdd9090e63f9d3c57ae95ac  ./languages/ocaml/extension.toml
9a2696aaf3c7434e6c913f1c40c210a6dce9afb2ce986891d767d50b51a33b65  ./languages/ocaml/highlights.scm
b06b36a5404b734f55ae280012bf8881f28390298f948b06e905eafe0734a919  ./languages/ocaml/injections.scm
36bb2fab66b16ded6003a95f3ed52ea63c5d67e5cf2ee6e1e529897161b751d0  ./languages/ocaml/tree-sitter-ocaml.wasm
68a9a526ae357ed5a2f8ca5dabb14131b283554b69639bb04816088d4b1f2fa0  ./languages/perl/LICENSE
f58576682df1bf62e00277ab51b1cb4408c4265f89de8ca80b0335642e3d4856  ./languages/perl/extension.toml
6400ebad4f0b86a918268f1179baaf046826ad5f8db2089331ee79915468138c  ./languages/perl/highlights.scm
31b8c2df3aa953b3b3339811c6bd1f50aac1a2e9d62dce2781cf958d835ec6da  ./languages/perl/injections.scm
e886a6704e9e3c171e145ccdcd3e2df11663ef32ddcca68d0181294d378bb141  ./languages/perl/tree-sitter-perl.wasm
664764fabf40c0b318e06324a5a12379092a2df25b8cbf2415f71bc0bb352346  ./languages/php/LICENSE
0a6c8170881f8c51553475fee0790c718366b464be4f5b3d5f6c92753d3f0e1d  ./languages/php/extension.toml
a2a7367659cff3b4be09961c8117d69a9b8703bfc266f8092e089b1760f197e9  ./languages/php/highlights.scm
7d07b1ed3ce1b8ffe3b91731e71afa627ecb1c8edd899605d98f930b4171f76c  ./languages/php/injections.scm
800ff4989aab8aa74584be7d837ef46b53779c44a0df16f4ec218997cfd85a85  ./languages/php/tree-sitter-php.wasm
664764fabf40c0b318e06324a5a12379092a2df25b8cbf2415f71bc0bb352346  ./languages/php_only/LICENSE
2837bf389b430769399982031ea4a3ca4a207e314fdc704dd8f40baa9253ec9a  ./languages/php_only/extension.toml
dee02aeaa5d44bbeffe067227a72d4371f52387107dc29bfeb13765a755b1e09  ./languages/php_only/highlights.scm
b90a3d4df0216a730e795da814bd6a90a6910752c0a0df450e3f08121c9d825f  ./languages/php_only/injections.scm
1ad79486818c875a4db139b4baf465a637d5344f1e99571cbf5a73145cbc46f0  ./languages/php_only/tree-sitter-php-only.wasm
1fca8454c0d77a73f9922a52d39e1f7c339f5da5d03f2766eff66b859fa19477  ./languages/powershell/LICENSE
194bcdfb7296fbf4ad41df1b80a47543d0c2fcd8eb3dd26c435c84ee182bcdb6  ./languages/powershell/extension.toml
37b202e28cbf7d69b21ef1f4bdb4cf79a87607164941c1f46620e0dc0d73e29b  ./languages/powershell/highlights.scm
c838eb88558c4032c727e0249436324432836592f1bb78ec9353c0487e2539ce  ./languages/powershell/injections.scm
a4708bddb25afd97c63eb2e24a1a0ae7c39eb9b7290de438c18273315c201d60  ./languages/powershell/tree-sitter-powershell.wasm
3297073719030b7261afd78b7267931ccc03a27f7a65ca6fc644316180f71bf6  ./languages/properties/LICENSE
38af545ab006f3cd1164dadce3938a3d276405e9f17fcecd95a0b79e29d3bf3e  ./languages/properties/extension.toml
3aa0902a52a4da23f0ce240b2968d8c1bf3ace6c90056e1f859ab389a9ca5e26  ./languages/properties/highlights.scm
25dffcbfd48950f9eda2892a7aace6b944c8653be0605c3be0ed6deb88212b82  ./languages/properties/injections.scm
a44a4658d8c16936aeab043afb34b0a4c11de53cc67b697cd3dcbce11c04de36  ./languages/properties/tree-sitter-properties.wasm
1cc17f880ff4fa68079a72b98b89d42e8ce16b1bdbb1ec7b6f33465241e41003  ./languages/proto/LICENSE
fac6561fd3bf56448c069ca7031d4ccadd717a2b8b2a48a762d0be3e34b59679  ./languages/proto/extension.toml
3883fc70019f57e5967c9770c91d2812f046a742d3b44627387384c4341fb767  ./languages/proto/highlights.scm
922d08a3eef7a4e248706db8b1139443bc134d4b34652a6cb562829e477efa8c  ./languages/proto/injections.scm
2f5f8447e0bfc12e54987b7fd227c6e6d0b0d1677366002790e231d76f441e4d  ./languages/proto/tree-sitter-proto.wasm
de2e49529f03d573bc3fa229dc83acfe22c63a5ad1766b289563edfd45b72dea  ./languages/r/LICENSE
e05db9bc720748e822069a2e99304438ea92c52a259376d36efb0a5734864c3f  ./languages/r/extension.toml
41c4007628edd78c6f1bae70b4cd9faa55cac83d3eb0c0bc9230a52a613c2237  ./languages/r/highlights.scm
eea3cb100b570bc5f5843b6d1e3c22f265fd884f435e6772770e05980e384ebf  ./languages/r/injections.scm
d69b2257dece5ddda025212e265d9c817be1bac172e8b1338dd0ce7b87559fea  ./languages/r/tree-sitter-r.wasm
cc35bd4a973bf6067558e88b56d9a369e02c36e240b2b537aef8f751b7808fd7  ./languages/racket/LICENSE
f8d1fb6224e65a963834aaae71619988521eeba31ffc2d7d934a1010fb856d3c  ./languages/racket/extension.toml
4b0a84b0a3c9f0cc0d7a3fd99d1a98549c9fd59725dc60cbe6668836966a8dd7  ./languages/racket/highlights.scm
0a96d2252b3ad8ca2544501dd88120ea6c6ffb8bf82762407ea89fc647c4d645  ./languages/racket/injections.scm
5d81922a8b4e76641ff05bdac31dffac566518e73dc34a58dd167afc87e0cc1f  ./languages/racket/tree-sitter-racket.wasm
c6137844f3f63aa4522dfc1cde8dff4f3314ed594ea954a73e3073dcc9d33b11  ./languages/ron/LICENSE
62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a  ./languages/ron/LICENSE-APACHE
c6137844f3f63aa4522dfc1cde8dff4f3314ed594ea954a73e3073dcc9d33b11  ./languages/ron/LICENSE-MIT
500c9c5cd452b3169eb9b184cc7962cca9a06b979ad4e411156539d4eaba8472  ./languages/ron/extension.toml
59f0f6c8bea0858aefad24885aa4272dfa8fd719dee59f32b9918ccb8c04cf94  ./languages/ron/highlights.scm
532c751f9c8defd5efc797cd0956b13a2f3e049024127d88bf690eb19d25763d  ./languages/ron/injections.scm
adbf681a100d47a185dda4866140546da37e1659600a6b699bf8df98fed25c6f  ./languages/ron/tree-sitter-ron.wasm
ee006f02a3d856df282e409be2a86e24a65bb573a98b9c28343771141351bb6b  ./languages/ruby/LICENSE
07f33545bf02b7a2fa09aec9d1475af09789f923cbcd0f755118509479977a54  ./languages/ruby/extension.toml
e04c8af7f629fecb4db155e28b2c1d676fbd8a9c6fbd2ac9a66cf141748e9029  ./languages/ruby/highlights.scm
bac7d3389dae76f31810375fbbd11ce12bce9638cd76a1116a18991307199ab9  ./languages/ruby/injections.scm
b3546d9df1ff6aa875cfe213fd83f8b8b2ba5413f29607c86d4b641cbe175553  ./languages/ruby/tree-sitter-ruby.wasm
1f95ed26e1f4074074c9c7083e61c0a9e4c3b9f435745044995f3beb4ed28575  ./languages/scala/LICENSE
a974a805b2f6363c97c53f762c4cc920c528d45c718c149bab1e0ee7c503427c  ./languages/scala/extension.toml
c6e2d65c4f84b5484af3e1edc3451850fe8d878ce11f79550e426f8ac6e2df4a  ./languages/scala/highlights.scm
4e14e0797e6543a53206872e989481860e46c200eaaeb561a9c4064cabb71d2c  ./languages/scala/injections.scm
eef94ce5ffc24456528319ca940f97001a4634385702cd313859f3f27f5e262d  ./languages/scala/tree-sitter-scala.wasm
0eea8dc45e89deeb03c7799bbbc7b4688f365fb274562f4540ecfebdea82e727  ./languages/scss/LICENSE
c5cfb43042b6b72045f4ba997834d0a7786d2793d91680868b5815b39f14fc78  ./languages/scss/LICENSE-CSS
144e4535e433d92bf52f7c1ec7ca1178df0c9df34a3148e94692dadb06a30f69  ./languages/scss/extension.toml
cbcdbb766e45987488f090cc56f1833057675095b37f6c6aed0d3325f8e2e645  ./languages/scss/highlights.scm
8698072e1540fcfe9a3d2790d8450dd61788e0c4f8c4cbd632fde23a6084c355  ./languages/scss/injections.scm
5be363ca82cf2913b882d21a91730bed9c252d491995dfbdf4c9184df67d6239  ./languages/scss/tree-sitter-scss.wasm
8844f0cc9b76b9c8a9d0251904eb7536b6bb9976e0ec577e8f27ab96d42523ef  ./languages/solidity/LICENSE
54f2a9b56ec322494fcbea55cbcbece40bb2333e839ec9d333eecfe27dcc5536  ./languages/solidity/extension.toml
f75c5b3d540931ad4204da9987253419c944002feafc9b6c0fcac155f6293e83  ./languages/solidity/highlights.scm
e2a50d888cbd90258e93197c9fbf668eebe1cdccd29248b6041d716bbc7c00e4  ./languages/solidity/injections.scm
c796a2dc9801af147dec44e9058147c6ef140aed0ac332ba9869e27de3c276ab  ./languages/solidity/tree-sitter-solidity.wasm
3b3e4e4252d5d7d1c18e1257005f23242bf0580ad619204fd093c99b8c56748d  ./languages/sql/LICENSE
bc19525ab2b17b181a7e4060a883fe8f9e0f5c04ca9b4a7bacdaf9b7d1eaa3c6  ./languages/sql/extension.toml
fe319dcc0add9e45380316f820f3cc8af33795a99a4550fb4bd91a3b99159cbb  ./languages/sql/highlights.scm
348a22ec95d5c3a876db856108870c8ef14e80511e7b80638504d21526b9f1cb  ./languages/sql/injections.scm
ef40885877b7a38a55295f31f953816c74aec31b9349271562707cce974071d6  ./languages/sql/tree-sitter-sql.wasm
099c44248f8cf353123211318680e93465587c005a4b3730ce8cb5334de043d6  ./languages/starlark/LICENSE
26f17fc8f34fc3583c9f6a8e64c8f54ab9be4306165b11dce7b4457f81f6476a  ./languages/starlark/extension.toml
b786d7d33ffd1b2987660bde171f291e0a685e95c6d22089480cbdd2ed7c8d79  ./languages/starlark/highlights.scm
a1f2c0f9c9f0246afda2fdbe095f39498aaf241e0f1d78967bdb410772a6cdc6  ./languages/starlark/injections.scm
da650e6c2f6e5be0c471c0178aa32c65b83edf792f3d3f89c201f5e984bc32e9  ./languages/starlark/tree-sitter-starlark.wasm
0eea8dc45e89deeb03c7799bbbc7b4688f365fb274562f4540ecfebdea82e727  ./languages/svelte/LICENSE
c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4  ./languages/svelte/LICENSE-APACHE
9d86557b8be5c9a72e7fb7ad75dc3a4dd6f6343034a071fd2d6001fd7356254b  ./languages/svelte/extension.toml
1f28360c8aa11cec98bfa6680c7d1aca408c8f0ce331f1baee7975f7ff7654fe  ./languages/svelte/highlights.scm
2cdc027fa6241d544b041b78099459a1b308c5ba1c549d319630e955dfa3103f  ./languages/svelte/injections.scm
9d1cdc676b811f7f4131a804bffd3f5b6c7bbf3086e0c887a01131f3319eec1d  ./languages/svelte/tree-sitter-svelte.wasm
3533cec129bb4bba015c0d61d86dd7c3b7e82110e4d2ff7837a01eff5bad5ccc  ./languages/swift/LICENSE
f7abd9b3f16f1e187cbd83ae35b41c855eaddce34cd9ead51381da312850736c  ./languages/swift/extension.toml
b0bbeb918904b252e8bd361f4afb4756647232391110a67deecbf7227464aff8  ./languages/swift/highlights.scm
30081a3ec89c3c1f6a9020e6f104b8c9dd4fc69d59ea11aeab81328bf7f749a2  ./languages/swift/injections.scm
bb66de33963b9c0d9d8c5f4181a3dcfd0be527a0cec026d4233005b175a52bc2  ./languages/swift/tree-sitter-swift.wasm
c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4  ./languages/terraform/LICENSE
074e6e32c86a4c0ef8b3ed25b721ca23aca83df277cd88106ef7177c354615ff  ./languages/terraform/LICENSE-APACHE
61df8977be477c3834071978715b613b130f9923f7bc9bf66e6e40ad4603fdfc  ./languages/terraform/extension.toml
4b9e331fad2eb3a0cde17e898951f0857df662f23c5715821ce57bf94ac7418a  ./languages/terraform/highlights.scm
ec784b07a6a18e259cb9569e19fbbb556d90f96d25baf63d245978677560277b  ./languages/terraform/injections.scm
525cdde2e77e54b09c6bfd36a04c284a327df314ba9b464d1340a1198ff35a43  ./languages/terraform/tree-sitter-terraform.wasm
57058cefd423b17720664c5d09fee47cd641c8f2705d3716d57299a284952591  ./languages/twig/LICENSE
0a39db996ff4e8ae74a3d39cea898d850a02b466c778d3fb926c7fb2c56d8d4f  ./languages/twig/extension.toml
385b031dac865a03804f4446c6587f788f5d1307108939ca0a7694fc7b347136  ./languages/twig/highlights.scm
0768de62a5dace728b519614b8014da171baeca64c5c006bda32c67e0c38ef70  ./languages/twig/injections.scm
74a3888284a6b0ee30d587767c3749e992ac22f293feb7bdf66a964f78583773  ./languages/twig/tree-sitter-twig.wasm
14eaaaa5c0cdb0eb86e85c55643c4cfa04593dbfbaef6a20f35337667b7278d7  ./languages/vim/LICENSE
deec47c91b65939fefc942ba231c68d8010745920432b7deba305f089e718f93  ./languages/vim/extension.toml
705bcc3a08eff73f89bbf15e9f2fa7fdf6e249821990a56f2c1aebd48d4aa3b8  ./languages/vim/highlights.scm
45c7d8eb9201e857e7aa3a3a3c63252fc4504c84f2918e9033618a7cb553e69e  ./languages/vim/injections.scm
1fd44b568fc2d9441fe4627e6f845db35bf77b45074fc12a7848c237fbfb479b  ./languages/vim/tree-sitter-vim.wasm
91ebb180e4e2e34628c45e405584d0faac6863660b69e8139da96290e58c14dd  ./languages/vue/LICENSE
c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4  ./languages/vue/LICENSE-APACHE
8a981a7ebe4533c01b3ebdf08d17d1937e892000c0618a9fc2a51b11a3da6816  ./languages/vue/extension.toml
55f92f4cc9a8ad1f9701986b58c26abe571ba7b30c79141cad624fe9c18eae2f  ./languages/vue/highlights.scm
8fbb6dcc0ca7c14cb238002335d013a430140d1bd4e478b03642af32a77b3adb  ./languages/vue/injections.scm
cdfdc60b440806346372966859a9748154c18c85d64e9dfba4868a79d5d8725a  ./languages/vue/tree-sitter-vue.wasm
0eea8dc45e89deeb03c7799bbbc7b4688f365fb274562f4540ecfebdea82e727  ./languages/zig/LICENSE
050ce6749244f463c3fc602d0cf57507caee85538ef8f5d3052b0640c37b9f6e  ./languages/zig/extension.toml
30388b3fb4930247d4b56d63a73bc590263cfee7faf6cbf9d55e425767632381  ./languages/zig/highlights.scm
d55106b00ab4b4b89076aa5fd766e1742bbb317aad8194b8510e2e14340a0e5b  ./languages/zig/injections.scm
c152827c347ff03742986d4369e864424accc353be62f8ddcb95f1a3711399a9  ./languages/zig/tree-sitter-zig.wasm
14a1da6f9b7d0b36f72620d9e59fd8e0b8599948879a6ccae13cc8eba426c136  ./packs/Tenmoku-language-pack-data.tar.gz
e7524bd777119f99d987b76026f2465c80e332e16450b6a3982e96a3c46bc2aa  ./packs/Tenmoku-language-pack-data.tar.gz.sha256
43f11012fdf92e8cc07b723e5246eb875520dcd7a4164a00cccda095d8aa45f3  ./packs/Tenmoku-language-pack-dotnet.tar.gz
0fbacc7a8f5106420b5a270b397f271f7e124585382848e598db3e53806742e9  ./packs/Tenmoku-language-pack-dotnet.tar.gz.sha256
012b9bc7f82858c134e19f79f9b8adeb5ccce4ecce2da9b986d8b18224ad3c29  ./packs/Tenmoku-language-pack-functional.tar.gz
9d3f1c186ca301af262ff33da9f7bc56e45ded1f8e4a1a0b18d563a1280596d6  ./packs/Tenmoku-language-pack-functional.tar.gz.sha256
6f0ac7ad20f89ac6bed69333625682605e06711eacba481ff60295cb76b94de3  ./packs/Tenmoku-language-pack-jvm.tar.gz
a138a96116358a5b7c4b7ef3e94031c4962c44f603975406dd69ed781d14b2eb  ./packs/Tenmoku-language-pack-jvm.tar.gz.sha256
4f9580eaab3827987f7d8cf1de998ffe8ed6ae1b262b6de72e66e69dd7f2fdea  ./packs/Tenmoku-language-pack-mobile.tar.gz
410bfc51db3a25efb455ae8188acc04fd953d1cb32e2d28209f8a01521b72c0c  ./packs/Tenmoku-language-pack-mobile.tar.gz.sha256
46c7dd7b268327b520a15865fae961c961620a68c4e2ce63efe41acb4e747169  ./packs/Tenmoku-language-pack-scripting.tar.gz
a3e790a13a9de657d694e6bc42f13ab433e78e027d86b79ad76209457fa43fa2  ./packs/Tenmoku-language-pack-scripting.tar.gz.sha256
428b8ab50f2ef60638b0873b099b03b60654d859ee8688c22787c4b4872b0e94  ./packs/Tenmoku-language-pack-systems.tar.gz
c3ea977e5a11b7dfda6f4f16c29334c7112aaff623d3b5d378f4478941a4b3f0  ./packs/Tenmoku-language-pack-systems.tar.gz.sha256
242fc584d67674a4070fa7aac2672c37cc5ab1dc919e3a55c4c776e01ddeb651  ./packs/Tenmoku-language-pack-web.tar.gz
c8b3746948284d2943727e46d43ba329e09496a865259adb8fb11a63f3cccf16  ./packs/Tenmoku-language-pack-web.tar.gz.sha256
659e9dc5b345644e8a6848197ecae8bdbfa128d31ef986f1bd2c82c9b9a78ddd  ./packs/Tenmoku-language-pack.tar.gz
f144acd715cc5d6e7fa2fcfd53a82ecf13335a7719f94c406c24a7f763c6d975  ./packs/Tenmoku-language-pack.tar.gz.sha256
```

The eight languages' entries in that index. Each one's tag is the same as in
the earlier runs before the rebases, since their files didn't change:

| Language | tag | packs | engine |
|---|---|---|---|
| kotlin | kotlin-7777d185c16b0777 | jvm, mobile | 1.0, 14, 1 |
| scala | scala-edfdcf8c0848825a | jvm, functional | 1.0, 15, 1 |
| csharp | csharp-e9148480c1cabd7a | dotnet | 1.0, 15, 1 |
| swift | swift-8d8a29a580a573c4 | mobile | 1.0, 15, 1 |
| cpp | cpp-4242143bd2449232 | systems | 1.0, 14, 1 |
| sql | sql-dcba292b48d5ce4c | data | 1.0, 14, 1 |
| php | php-1d10df8cbe5d4376 | web | 1.0, 15, 1 |
| ruby | ruby-c182b2ec54ced7a1 | web, scripting | 1.0, 14, 1 |

A Mac build may give other wasm bytes than this Linux one, since it uses a
different wasi-sdk archive. Card 2 builds in CI, and that build is the one
published.
