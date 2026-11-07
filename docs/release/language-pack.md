# Tenmoku language pack

One download with every first-party WebAssembly grammar (one for each directory
under `extensions/grammars/`), for machines that can't reach GitHub from the app.
The in-app **Install** action offers the same files online, each checked against
the hash the app carries; use this pack only when that isn't possible.

Each grammar release (`v*` tag) has, among its assets:

- `Tenmoku-language-pack.tar.gz`
- `Tenmoku-language-pack.tar.gz.sha256`
- each language's `tree-sitter-*.wasm` file

## Verify

In the directory you downloaded both files to:

```sh
# Linux
sha256sum -c Tenmoku-language-pack.tar.gz.sha256
# macOS
shasum -a 256 -c Tenmoku-language-pack.tar.gz.sha256
```

It prints `Tenmoku-language-pack.tar.gz: OK`.

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
tar -xzf Tenmoku-language-pack.tar.gz -C "$dir"
```

That makes `tenmoku.grammar_<language>/` for each language, as the in-app
install does. A running app picks them up without a restart. Unpacking over an
earlier copy replaces its files. To remove a language, delete its directory or
disable it in the extensions picker.

## Building it

`scripts/build-grammar-wasm.sh` builds the wasm into `target/grammar-wasm/`, then
`scripts/package-language-pack.sh` writes `dist/Tenmoku-language-pack.tar.gz` and
its `.sha256`. `.github/workflows/grammars.yml` runs both and uploads the results.
Adding a language to the pack is adding its directory: see
`extensions/grammars/README.md`.
