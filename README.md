# Tenmoku

Tenmoku is a native code editor for macOS, written in Rust on
[GPUI](https://www.gpui.rs/) and rendered with Metal. It is a small, curated
editor with the parts of an IDE you reach for every day (a project tree,
language-server features, git, a terminal) and no embedded web runtime. It is
pre-1.0: expect rough edges, and settings and the plugin API may change.

## Install

1. Download `Tenmoku-<version>-macos-arm64.zip` from the
   [Releases page](https://github.com/darylrobbins/tenmoku/releases).
2. Unzip it and move `Tenmoku.app` to `/Applications`.
3. A release is ad-hoc signed and not notarized unless its notes say
   otherwise, so macOS Gatekeeper blocks the first launch. Right-click the app
   and choose Open, or try to open it once and then choose Open Anyway in System
   Settings > Privacy & Security, or clear the quarantine flag:

   ```zsh
   xattr -dr com.apple.quarantine /Applications/Tenmoku.app
   ```

The executable inside the bundle is `Tenmoku.app/Contents/MacOS/tenmoku`.

**Check for Updates…** is the manual way to look for a newer release; it asks
GitHub. A bundle that carries Sparkle and the update feed keys uses Sparkle
instead once Sparkle has loaded and started, and then `update.automatic_checks`
turns its daily background check on or off. If it doesn't start, or the build
has no Sparkle, only the manual check exists. See
[docs/release/macos-signing.md](docs/release/macos-signing.md) for how
releases are signed.

## Build from source

You need macOS on Apple Silicon, the Xcode command line tools and
[rustup](https://rustup.rs/). [rust-toolchain.toml](rust-toolchain.toml) pins
the Rust version, and rustup installs it on the first build.

```zsh
git clone https://github.com/darylrobbins/tenmoku.git
cd tenmoku
cargo build --release -p fastedit      # produces target/release/tenmoku
scripts/bundle-macos.sh                # produces target/bundle/Tenmoku.app
```

The Cargo package is still named `fastedit` and the crates `fe-*`; the binary is
`tenmoku`. `scripts/bundle-macos.sh` does not run cargo; see its `--help`. To
run without bundling, use `cargo run --release -p fastedit -- path/to/project`.

## Platform support

- **macOS on Apple Silicon**: the supported platform, and the only one with a
  release download.
- **Linux and Windows**: built and tested in CI (`.github/workflows/ci.yml`)
  but unsupported, with no release. For Windows build notes, see
  [CONTRIBUTING.md](CONTRIBUTING.md).

## Features

- **Syntax**: tree-sitter highlighting, indentation, folding and injections.
  Built in: Rust, Python, JavaScript, TypeScript and TSX, Go, C, Java, Lua,
  HTML, CSS, JSON, JSON5, TOML, YAML, XML, INI, CSV, Markdown, Bash,
  Dockerfile, Make, Just, diff and the git file formats.
- **WebAssembly grammars**: the grammar sources for 56 more languages (C++,
  C#, Kotlin, Swift, SQL, PHP, Ruby and others), one directory each under
  [extensions/grammars/](extensions/grammars/), are not built in. The app
  offers to install one only after the grammar release's hashes are recorded in
  `extensions/grammars/pack.toml`, and checks a download against them. The
  [language pack](docs/release/language-pack.md) is the same files in one
  download for machines that can't reach GitHub.
- **Language servers**: rust-analyzer, pyright, typescript-language-server (or
  typescript-go), gopls and the CSS server, found on your `PATH`, with
  diagnostics, hover, completion, go to definition, references, rename, code
  actions, inlay hints, code lenses, semantic highlighting and format on save.
- **Navigation**: peek definition and references, symbol picker, workspace
  symbols, project tree, find and replace, project-wide search.
- **Editing**: multiple cursors, soft wrap, sticky scroll, minimap.
- **Vim mode**: remaps, ex commands, macros and marks, see [docs/vim.md](docs/vim.md).
- **Git**: gutter change bar, line blame, diffs, stash, branch create and switch.
- **Terminal** and **Markdown preview**.
- **Claude chat** panel (needs your own Anthropic API key).
- **Themes**: Yohen and Hakuji (the defaults), Tenmoku Dark, Light and Ember,
  Dark Modern, Dark 2026, Light 2026, the four Catppuccin flavours and Monokai;
  dark and light can follow the system.
- **Extensions** for themes and grammars, and a preview of WebAssembly plugins,
  off by default: see [docs/plugins/authoring.md](docs/plugins/authoring.md).
- **Setup wizard** on first launch, and a `tenmoku` command line:
  `tenmoku [options] [path[:line[:column]] ...]`, with `--new-window` and
  `--wait` (block until the files' tabs close, useful as `$EDITOR`).

Every setting, with its default, is in
[crates/fe-config/settings-template.toml](crates/fe-config/settings-template.toml).
Settings reload when you save the file.

## Documentation

- [CONTRIBUTING.md](CONTRIBUTING.md): how to contribute
- [TESTING.md](TESTING.md) and [PERF.md](PERF.md): how the editor is tested and measured
- [docs/vim.md](docs/vim.md): vim mode
- [docs/plugins/authoring.md](docs/plugins/authoring.md): writing a plugin
- [docs/release/language-pack.md](docs/release/language-pack.md): the offline language pack
- [docs/release/macos-signing.md](docs/release/macos-signing.md): signing and releases
- [kiln/README.md](kiln/README.md): the app framework in this repo

## License

Tenmoku is licensed under the Apache License 2.0, see [LICENSE](LICENSE).
Third-party components and their licences are listed in [NOTICE](NOTICE) and
[THIRD_PARTY.md](THIRD_PARTY.md).
