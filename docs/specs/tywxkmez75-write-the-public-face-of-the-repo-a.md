---
task_uid: "01m6ab4b4077ke7atywxkmez75"
task: "T1271"
rev: 2
type: "docs"
complexity: "s"
criteria:
  - id: "C1"
    text: "README.md exists and has sections for what Tenmoku is, building from source, running, platform support, documentation and License; the License section says Apache-2.0 and links LICENSE, NOTICE and THIRD_PARTY.md"
    required: true
    manual: false
  - id: "C2"
    text: "Every relative markdown link in README.md and CONTRIBUTING.md resolves to an existing file: rg -o \"\\]\\((?!https?:|#|mailto:)[^)#]+\" -r \"\" --pcre2 README.md CONTRIBUTING.md piped through a test -e loop prints no missing path"
    required: true
    manual: false
  - id: "C3"
    text: "The build and run commands in README.md are ones that work: cargo build --release -p fastedit succeeds and target/release/tenmoku exists; the README names that path, and rg -n \"fastedit\" README.md CONTRIBUTING.md only matches the package/crate name explanation, repo paths or crate names (no user-facing product name)"
    required: true
    manual: false
  - id: "C4"
    text: "CONTRIBUTING.md links CLEAN_ROOM.md before its setup section, names cargo nextest run --workspace, cargo fmt --all --check, cargo clippy --workspace --all-targets -- -D warnings and cargo deny check, and states Apache-2.0 for contributions; it contains no config or state directory path"
    required: true
    manual: false
  - id: "C5"
    text: ".github/ISSUE_TEMPLATE/bug_report.yml and feature_request.yml parse as YAML (ruby -ryaml -e \"YAML.load_file(ARGV[0])\" file for each, or yq) and have name, description, and a body with at least a required field; config.yml has blank_issues_enabled: false"
    required: true
    manual: false
  - id: "C6"
    text: ".github/PULL_REQUEST_TEMPLATE.md has a checklist (lines starting \"- [ ]\") naming tests, fmt and clippy, and CLEAN_ROOM.md"
    required: true
    manual: false
  - id: "C7"
    text: "git diff --stat against the base lists only the six scope paths"
    required: true
    manual: false
---

# Write the public face of the repo: a README for strangers, CONTRIBUTING.md, bug and feature issue templates and a pull request template

## Goal

The repository can be made public: a stranger lands on a README that says what Tenmoku is and how to build and run
it, and finds how to contribute, file a bug and send a PR. Docs and templates only; no code.

## Facts found

- There is no `README.md` at the root (T1257 had nothing to rename). `kiln/README.md` exists and stays as is.
  There is no `CONTRIBUTING.md`, no `.github/ISSUE_TEMPLATE/`, no PR template. `.github/` holds only `workflows/`.
- Build facts to state accurately (verify each against the repo, do not paraphrase from memory): toolchain pinned in
  `rust-toolchain.toml` (rustup installs it); `cargo build --release -p fastedit` produces `target/release/tenmoku`;
  `cargo run -p fastedit`; tests with `cargo nextest run --workspace` (TESTING.md; some tests need each test in its
  own process); macOS is the primary platform (Metal via GPUI), Linux builds in CI (`linux` job in ci.yml) and runs
  headless under Xvfb (`.claude/skills/run-fastedit-linux/`); gate checks in `scripts/` and CI: fmt, clippy
  `-D warnings`, `cargo deny check`, `scripts/check-no-gpui.sh`, `scripts/check-syntax-features.sh`.
- `CLEAN_ROOM.md` is the rule contributors most need to see: never read or port Zed's GPL/AGPL crates or queries.
  CONTRIBUTING must link it near the top.
- The licence is Apache-2.0 once the licence card lands (this card depends on it). Package and crate names stay
  `fastedit`/`fe-*`, so say that once to avoid confusing readers.
- Config and state paths are changing in T1258; do not write them in these docs (link `docs/` or the in-app settings
  instead).
- The repo's origin is `github.com/darylrobbins/fastedit`; do not write the final public URL or name anywhere:
  use relative links.

## Changes

1. `README.md`: one-paragraph what/why (a fast native editor on GPUI with AI proposals reviewable as diffs and a Vim
   engine; confirm each claim from `docs/` and NOTICE-listed features, drop any you cannot point at), status
   (pre-1.0), install-from-source, run, platform support, a "Documentation" list linking TESTING.md, PERF.md,
   CONTRIBUTING.md, docs/, `kiln/README.md` (the app framework in this repo), and a License section (Apache-2.0,
   link LICENSE and NOTICE, third-party notices in THIRD_PARTY.md). No screenshots that do not exist: use only
   files already in the repo or none.
2. `CONTRIBUTING.md`: before you start (open an issue for anything non-trivial; CLEAN_ROOM.md), setup, running
   tests and which checks CI runs, code style (what `cargo fmt` and clippy enforce; the repo's own rules, which you
   find in existing docs, no invented ones), commit/PR expectations, licence of contributions (Apache-2.0 inbound =
   outbound; no CLA text unless the user adds one), security issues: say to email the maintainer (use the address in
   `git log -1 --format=%ae` only if the user's public address is already in Cargo.toml authors; otherwise write a
   TODO marker the lead resolves, listed in your hand-in).
3. `.github/ISSUE_TEMPLATE/bug_report.yml` (version, OS and chip, steps, expected, actual, log excerpt, recent
   regression?), `.github/ISSUE_TEMPLATE/feature_request.yml` (problem, proposal, alternatives),
   `.github/ISSUE_TEMPLATE/config.yml` (blank issues off; contact link to CONTRIBUTING.md).
4. `.github/PULL_REQUEST_TEMPLATE.md`: what and why, how tested, checklist (tests added, `cargo fmt`/clippy clean,
   CLEAN_ROOM.md respected, docs updated).

## Not here

Code of Conduct (ask the lead), SECURITY.md, a docs site, badges that need the final public URL.

## User steer (2026-11-05): the README ships with the public v0.1.0 release

The repo is now public at https://github.com/darylrobbins/tenmoku (renamed from fastedit; use that URL
everywhere, never darylrobbins/fastedit). v0.1.0 is tagged right after this card lands, and its Release
workflow attaches `Tenmoku-0.1.0-macos-arm64.zip` (ad-hoc signed, not notarized, no appcast). So the README
also has an "Install" section: download that zip from the Releases page, unzip, move Tenmoku.app to
/Applications, then clear Gatekeeper's quarantine with `xattr -dr com.apple.quarantine /Applications/Tenmoku.app`
(or System Settings > Privacy & Security > Open Anyway). Do not promise in-app updates for 0.1 (no Sparkle
key or appcast yet) and do not invent a CLI install step: `tenmoku` is the binary inside the bundle. The
licence section says Apache-2.0 (T1267). A draft written from the code, to check and trim rather than copy, is at
/private/tmp/claude-501/-Users-daryl-code-fastedit/858e5bc6-3a6f-483a-9a5f-352b4521ad02/scratchpad/README.md;
claim only features you can point to in the code.
