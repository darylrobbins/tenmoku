# Tenmoku

Tenmoku is a fast, native code editor for macOS, written in Rust on
[GPUI](https://www.gpui.rs/) and rendered with Metal. It aims to be a small,
curated editor with the parts of an IDE you reach for every day: a project tree,
language-server features, git awareness and a terminal, without a heavy runtime.

This is an early, pre-1.0 release (0.1).

## Features

- Syntax highlighting, indentation and code folding from tree-sitter grammars
  (Rust, Python, JavaScript, TypeScript, Go, C, C++, Java, C#, Lua, HTML, CSS,
  JSON, YAML, Markdown, Bash and more). Kotlin, Scala, Swift, Ruby, PHP, SQL and
  other grammars ship as downloadable WebAssembly language packs.
- Language server support: diagnostics, hover, completion, go to definition,
  find references, rename, code actions, inlay hints, code lenses, semantic
  highlighting and format on save.
- Peek definition and peek references, a symbol picker and workspace symbol search.
- Project tree, find and replace in a file, and project-wide search.
- Multiple cursors, soft wrap, sticky scroll, a minimap and a tab switcher.
- Vim mode (`fe-vim`), with remaps, ex commands, macros and marks.
- Git: a change bar in the gutter, line blame, diffs against HEAD or the index,
  stash, and branch create and switch.
- Integrated terminal.
- Markdown preview.
- Claude chat panel (needs your own Anthropic API key, `ANTHROPIC_API_KEY`).
- Themes: Tenmoku Dark, Light and Ember, VS Code Dark Modern, Dark 2026 and
  Light 2026, and the four Catppuccin flavours. Dark and light follow the system.
- Extensions for themes and grammars, and a preview of WebAssembly plugins
  (off by default, see below).
- A `tenmoku` command-line interface: `tenmoku [options] [path[:line[:column]] ...]`,
  with `--new-window` and `--wait` (block until the files' tabs are closed,
  useful as `$EDITOR`).

## Platform support

macOS on Apple Silicon is the primary platform (Metal through GPUI), and the
only one with a release download. Linux builds in CI and the editor runs
headless under Xvfb for testing (see `.claude/skills/run-fastedit-linux/`), but
there is no supported Linux release yet.

## Install

1. Download `Tenmoku-0.1.0-macos-arm64.zip` from the
   [Releases page](https://github.com/darylrobbins/tenmoku/releases).
2. Unzip it and move `Tenmoku.app` to `/Applications`.
3. The 0.1 build is ad-hoc signed and not notarized, so macOS Gatekeeper blocks
   the first launch. Clear the quarantine flag:

   ```zsh
   xattr -dr com.apple.quarantine /Applications/Tenmoku.app
   ```

   Or try to open the app once, then go to System Settings > Privacy & Security
   and choose Open Anyway.

The executable inside the bundle is `Tenmoku.app/Contents/MacOS/tenmoku`. To use
it from a shell, link it onto your `PATH`, for example:

```zsh
ln -s /Applications/Tenmoku.app/Contents/MacOS/tenmoku /usr/local/bin/tenmoku
```

## Build from source

You need macOS on Apple Silicon, the Xcode command line tools (the Metal
toolchain and `sips`/`iconutil` for the app icon) and [rustup](https://rustup.rs/).
`rust-toolchain.toml` pins the Rust version, and rustup installs it on
the first build.

```zsh
git clone https://github.com/darylrobbins/tenmoku.git
cd tenmoku
cargo build --release -p fastedit      # the package is still named fastedit; produces target/release/tenmoku
scripts/bundle-macos.sh                # produces target/bundle/Tenmoku.app
```

`scripts/bundle-macos.sh` does not run cargo. It takes `--binary`, `--out` and
`--version` (see `--help`) and signs ad hoc unless `SIGN_IDENTITY` names a
Developer ID certificate. To run without bundling, use
`cargo run --release -p fastedit -- path/to/project`.

Tests run with `cargo nextest run --workspace`. See [TESTING.md](TESTING.md) and
[PERF.md](PERF.md) for how the editor is tested and measured, and
[docs/release/macos-signing.md](docs/release/macos-signing.md) for signing,
notarization and cutting a release.

## Configuration

Every setting, with its default, is in the commented template
[crates/fe-config/settings-template.toml](crates/fe-config/settings-template.toml);
the JSON schema is `crates/fe-config/settings.schema.json`. Settings reload when
you save the file.

## Status

Version 0.1: expect rough edges, and settings and the plugin API may change.
Notarized releases and wider platform support are not done yet.

## Documentation

- [CONTRIBUTING.md](CONTRIBUTING.md): how to contribute
- [TESTING.md](TESTING.md) and [PERF.md](PERF.md): how the editor is tested and measured
- [docs/](docs/): design notes, decisions and release guides
- [kiln/README.md](kiln/README.md): the app framework in this repo

The Cargo package and crates keep their original names (`fastedit`, `fe-*`).

## License

Tenmoku is licensed under the Apache License 2.0, see [LICENSE](LICENSE).

Third-party components and their licences are listed in [NOTICE](NOTICE) and
[THIRD_PARTY.md](THIRD_PARTY.md).
