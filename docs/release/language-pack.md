# Tenmoku language pack

Offline tarballs of the grammars the Tenmoku registry publishes, for machines
that can't reach GitHub from the app. The in-app **Install** action offers the
same files online, each checked against the signed index; use a tarball only
when that isn't possible. The registry, its index and how it's published are in
[grammar-registry.md](grammar-registry.md).

The registry's `index` release
(<https://github.com/tenmoku-editor/registry/releases/tag/index>) has:

- `Tenmoku-language-pack-<pack>.tar.gz` for each pack, with the languages in it
  that pack lists, and its `.sha256`
- `Tenmoku-language-pack.tar.gz`, every language, and its `.sha256`

Each is at
`https://github.com/tenmoku-editor/registry/releases/download/index/<file>`.
The packs are those `extensions/grammars/registry.toml` defines: `shells`,
`web`, `infra`, `functional`, `jvm`, `dotnet`, `systems`, `mobile`, `data`,
`docs`, `shaders`, `hardware`, `config` and `scripting`.

## Verify

In the directory you downloaded the tarball and its `.sha256` to (here the
`jvm` pack):

```sh
# Linux
sha256sum -c Tenmoku-language-pack-jvm.tar.gz.sha256
# macOS
shasum -a 256 -c Tenmoku-language-pack-jvm.tar.gz.sha256
```

It prints `Tenmoku-language-pack-jvm.tar.gz: OK`. The signed `index.json`
records each pack tarball's SHA-256 too (its `tarball` entry), so to check the
hash against a signed source, verify the index first
([Checking a published index](grammar-registry.md#checking-a-published-index)):

```sh
base=https://github.com/tenmoku-editor/registry/releases/download/index
curl -fsSLO "$base/index.json"
curl -fsSLO "$base/index.json.sig"
cargo run -q -p fe-registry --bin grammar-registry -- verify --index index.json
cargo run -q -p fe-registry --bin grammar-registry -- tarballs --index index.json
```

`verify` prints `index.json verifies against …'s public_key`, and `tarballs`
lists each pack tarball's name, size and SHA-256, which must match the
`.sha256`. The
all-languages tarball isn't in the index; check it against its `.sha256`.

## Install

Unpack it into the user extensions directory, `<state dir>/extensions`
(`fe_config::paths::app_paths().extensions_dir()`):

| OS | Extensions directory |
| --- | --- |
| macOS | `~/.local/state/tenmoku/extensions` |
| Linux | `~/.local/state/tenmoku/extensions` |

Both follow `$XDG_STATE_HOME` (`$XDG_STATE_HOME/tenmoku/extensions`) when it is
an absolute path, and `$TENMOKU_STATE_DIR/extensions` overrides everything.

```sh
case "$XDG_STATE_HOME" in /*) state="$XDG_STATE_HOME" ;; *) state="$HOME/.local/state" ;; esac
dir="${TENMOKU_STATE_DIR:-$state/tenmoku}/extensions"
mkdir -p "$dir"
tar -xzf Tenmoku-language-pack-jvm.tar.gz -C "$dir"   # or Tenmoku-language-pack.tar.gz
```

That makes `tenmoku.grammar_<language>/` for each language, as the in-app
install does. A running app picks them up without a restart. Unpacking over an
earlier copy replaces its files. To remove a language, delete its directory or
disable it in the extensions picker.

## Building it

`scripts/build-grammar-wasm.sh` builds the wasm into `target/grammar-wasm/`, then
`scripts/package-language-pack.sh` writes the tarballs (all languages, or
`--pack <name>`) and their `.sha256` files into `dist/`.
`.github/workflows/grammars.yml` does this on every merge to main and
publishes the result to the registry. Adding a language to a pack is adding its
directory: see `extensions/grammars/README.md`.
