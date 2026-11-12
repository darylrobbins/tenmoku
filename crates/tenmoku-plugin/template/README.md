# Word Count: a Tenmoku plugin to copy

This plugin shows the focused buffer's word count in the status bar, and logs
it on the command **Word Count: Count Words**. Copy this folder to start a plugin of
your own. `docs/plugins/authoring.md`, the author guide, has the steps for
renaming it.

## API 1.1, a preview

The plugin builds against plugin API **1.1**: the WIT packages
`kiln:extension@1.1.0` and `fastedit:editor@1.1.0`, which the `tenmoku-plugin`
SDK binds. Its `extension.toml` says so:

```toml
[engine]
tenmoku = "1.1"
```

API 1.1 is a **preview**. Code plugins run only while the setting
`extensions.plugins_preview` is on. The API may change incompatibly in any
release until it is declared stable, so rebuild your plugin against each
release.

A host keeps one world for each API minor it supports. A plugin built for 1.0
still loads, against the 1.0 world, and is never asked for anything 1.1 added.

## Building

```zsh
rustup target add wasm32-wasip2   # once
./build.zsh                       # arguments go to cargo, such as --offline
```

`build.zsh` builds the crate for `wasm32-wasip2` and copies the component to
`word_count.wasm`, the file `extension.toml` names. To try it, turn on
`extensions.plugins_preview`, run **Extensions: Install Folder for
Development…** and pick this folder. The plugin reloads whenever
`word_count.wasm` is rebuilt.

In this repository, `crates/tenmoku-plugin/tests/template.rs` loads the
prebuilt `word_count.wasm`, since CI doesn't build guests. Run `build.zsh` after
any change to the WIT or the SDK.
