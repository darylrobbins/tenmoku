# Contributing to Tenmoku

Thanks for helping. Please read this first.

## Before you start

- **Read [CLEAN_ROOM.md](CLEAN_ROOM.md).** Never read, copy or port Zed's GPL/AGPL
  crates or queries, or Vim and Neovim source. Patches that do are rejected.
- Open an issue before starting anything non-trivial, so we can agree on the
  approach. Small fixes can go straight to a pull request.
- Security issues: do not open a public issue. Email the maintainer at
  TODO(lead): security contact address.

## Setup

Install [rustup](https://rustup.rs/); `rust-toolchain.toml` pins the Rust version
and rustup installs it on the first build. macOS on Apple Silicon is the primary
platform; see the platform notes in [README.md](README.md). You also need
[cargo-nextest](https://nexte.st/) and [cargo-deny](https://github.com/EmbarkStudios/cargo-deny).

```zsh
cargo build --release -p fastedit   # the package keeps its old name; produces target/release/tenmoku
cargo run -p fastedit               # run a debug build
```

### Windows

You also need:

- The MSVC toolchain: Visual Studio 2022 or its Build Tools, with the "Desktop
  development with C++" workload.
- CMake on `PATH`, for the WebAssembly runtime's C API. Visual Studio's own copy
  works: `C:\Program Files\Microsoft Visual Studio\2022\<edition>\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`.

The language servers aren't bundled. For Rust, add the `rust-analyzer` component
to the pinned toolchain with `rustup component add rust-analyzer`.

The Windows build lacks two things the other platforms have:

- A second launch opens its own window. It doesn't hand its files to the running
  instance.
- Copying a symlink in the project tree is refused.

## Tests and checks

CI runs these, and a pull request should pass all of them:

```zsh
cargo nextest run --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
scripts/check-no-gpui.sh
scripts/check-syntax-features.sh
```

Use nextest rather than `cargo test`: some tests need each test in its own
process. [TESTING.md](TESTING.md) describes the test layers and where to add
tests; [PERF.md](PERF.md) covers benchmarks.

## Code style

`cargo fmt` and clippy with warnings denied are the style rules; there is no
other formatter configuration. Add a test with every behaviour change or fix,
in the layer [TESTING.md](TESTING.md) describes.

## Commits and pull requests

- Keep a pull request to one change, with a message that says what and why.
- Fill in the pull request template, including how you tested.
- Update docs when behaviour changes.

## Licence of contributions

Tenmoku is Apache-2.0 ([LICENSE](LICENSE)). Unless you say otherwise, a
contribution you submit is licensed under Apache-2.0 too (inbound = outbound).
New third-party code or assets must be compatible and listed in
[THIRD_PARTY.md](THIRD_PARTY.md).
