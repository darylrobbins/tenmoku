---
task_uid: "01m70cfjxnbn1d66ztjdhgmvx4"
task: "T339"
rev: 1
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "Every feature, grammar, setting and path README.md names exists in the tree: each language listed is in the bundled set or under extensions/grammars/, each setting is in crates/fe-config/settings-template.toml, each linked file exists (the rg -o relative-link loop from T1271 C2 prints no missing path)"
    required: true
    manual: false
  - id: "C2"
    text: "README.md has sections for what Tenmoku is, Install (Releases zip, ad-hoc signed and not notarized, the right-click Open / Open Anyway / xattr step, no hard-coded stale version such as 0.1.1), Build from source (points at rust-toolchain.toml, builds via cargo build --release -p fastedit and scripts/bundle-macos.sh), Platform support (macOS Apple Silicon only release; Linux and Windows CI-built, unsupported), Features, Documentation links (CONTRIBUTING, TESTING, PERF, docs/plugins/authoring.md, docs/release/language-pack.md, docs/vim.md) and License (Apache-2.0, LICENSE, NOTICE, THIRD_PARTY.md)"
    required: true
    manual: false
  - id: "C3"
    text: "Only README.md changes; no image is added or referenced (committed images are test goldens only); rg -n fastedit README.md matches only the package and crate name explanation, repo paths or crate names"
    required: true
    manual: false
---

# Refresh the README for the current app: install and update, features, platforms and links checked against the code (v0.1.1..main) (Mac T1654)

## Problem

README.md was written for 0.1.0 (T1271, T1388). It still names `Tenmoku-0.1.1-macos-arm64.zip`, and the
tree has moved on (about 150 commits since v0.1.1: injection-only grammars, built-in colour swatches,
SQL in Go strings, code keys, Check for Updates, the setup wizard, language icons, i18n groundwork,
the extensions and plugins preview). The Features list and the grammar lists no longer match
`extensions/grammars/` and `crates/fe-syntax`.

## Change

Edit README.md only (no docs/ index file unless one genuinely fits; if so, docs/README.md as a plain
link list). Check each claim against the code before writing it, not against the old README:

- **What it is**: one honest paragraph (native, GPUI + Metal, curated, pre-1.0).
- **Install**: GitHub Releases zip, ad-hoc signed and not notarized, the right-click Open (or Open
  Anyway, or `xattr`) step as the release notes say it; name the asset without hard-coding a version
  that goes stale (`Tenmoku-<version>-macos-arm64.zip`), and say how Check for Updates behaves
  (read crates/fastedit and docs/release/macos-signing.md for what really ships).
- **Build from source**: the `rust-toolchain.toml` pin (1.98.1 today, state it by pointing at the file),
  Xcode command line tools, the cargo and `scripts/bundle-macos.sh` steps as they work now.
- **Platforms**: macOS Apple Silicon is the supported one; Linux and Windows stated as CI-built,
  unsupported, no release, matching what CI actually runs (.github/workflows).
- **Headline features**, each one real today: LSP (list from crates/fe-lsp), tree-sitter grammars,
  WebAssembly grammars with install offers and the language pack (docs/release/language-pack.md;
  counts and names from extensions/grammars/ and the bundled set, not from memory), vim mode with a
  link to docs/vim.md once that exists (otherwise link nothing and leave a single sentence), git,
  terminal, project search and replace, themes, extensions and the plugins preview (link
  docs/plugins/authoring.md), Check for Updates, the setup wizard, the CLI, Claude chat, localisation
  (only what shipped).
- **Links**: CONTRIBUTING, TESTING, PERF, docs/ guides, settings template; every relative link resolves.
- **Licence**: Apache-2.0 with NOTICE and THIRD_PARTY.md.
- **Screenshots**: none are added. The only committed generated images are goldens such as
  crates/fe-chat-view/examples/screenshots/golden/chat_dark.png; do not reference goldens from the
  README (they are test fixtures), so the README stays text-only.

## Out of scope

Release notes, CONTRIBUTING.md, the docs/ guides, any code change.
