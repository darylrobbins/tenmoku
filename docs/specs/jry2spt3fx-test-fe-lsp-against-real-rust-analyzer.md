---
task_uid: "01m49phgjcmh5vs7jry2spt3fx"
task: "T148"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tools/lsp/package.json and its generated package-lock.json pin exactly pyright 1.1.414, typescript-language-server 6.0.1, typescript 6.0.3 and typescript-go as npm:typescript@7.0.2; tools/lsp/mise.toml pins an exact Node 24 LTS patch; .gitignore ignores tools/lsp/node_modules/"
    required: true
    manual: false
  - id: "C2"
    text: "scripts/lsp-real.sh runs mise install and npm ci --prefix tools/lsp, rustup component add rust-analyzer, then cargo nextest run -p fe-lsp --run-ignored only -E 'test(/real_server_/)', and exits non-zero when any step fails; rust-toolchain.toml is unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "Every test in crates/fe-lsp/tests/real_server.rs is named real_server_* and marked #[ignore = \"needs language servers: scripts/lsp-real.sh\"], so cargo nextest run --workspace runs none of them; each finds its binary from FE_LSP_\u003cSERVER\u003e, then tools/lsp/node_modules, then rustup which, and when missing prints skipped: \u003cbin\u003e not found (run scripts/lsp-real.sh) and passes, or fails when FE_LSP_REQUIRE_SERVERS=1"
    required: true
    manual: false
  - id: "C4"
    text: "The real-server tests cover: §4.4 cases 1, 3 and 7 resolving to the §4.2 columns for rust-analyzer under utf-8 and under utf-16, pyright and typescript-language-server under utf-16 and typescript-go under utf-8, with use sites not at a line's end; definition and references through LspNavigation for Rust, Python and both TypeScript servers; launch to Ready within 60 s; clean shutdown with no kill, typescript-go's exit code 1 counted as normal; rust-analyzer diagnostics changing after saving a type error introduced and then fixed; whether each server's publishDiagnostics carries a version"
    required: true
    manual: false
  - id: "C5"
    text: ".github/workflows/lsp.yml runs only on workflow_dispatch (no schedule), on macos-15 with a 45-minute timeout, installs Node through jdx/mise-action from tools/lsp/mise.toml, caches npm on hashFiles('tools/lsp/package-lock.json') plus the Node version and ~/.rustup/toolchains on rust-toolchain.toml, sets FE_LSP_REQUIRE_SERVERS=1 and runs scripts/lsp-real.sh; ci.yml and soak.yml are unchanged (the user approved it on demand only, 2026-10-10)"
    required: true
    manual: false
  - id: "C6"
    text: "scripts/lsp-real.sh passes on this Mac with every server present and FE_LSP_REQUIRE_SERVERS=1 (summary in the criteria file); its LspNavigation definition checks through rust-analyzer, pyright and typescript-language-server stand in for M4's in-app check, which moves to a separate human card; TESTING.md's Real language servers section notes the script, env vars, workflow and the in-app F12 steps"
    required: true
    manual: false
  - id: "C7"
    text: "docs/decisions/lsp-client.md §10 records, from these runs, which servers version publishDiagnostics, the negotiated encoding of the rust-analyzer 1.98.1 rustup component, and launch-to-Ready time on this repository; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Test fe-lsp against real rust-analyzer, pyright and TypeScript servers, on demand

# Test fe-lsp against real rust-analyzer, pyright and TypeScript servers, on demand

Card 10 of `docs/decisions/lsp-client.md` §8. The design is:

- §6.2 (versions and how each server is installed)
- §6.3 (the gate versus the slow group, `lsp.yml`, skipping cleanly)
- §4.4 "Real servers, slow group"
- §4.2's measured columns
- §4.1's real-server save test
- §10's open questions this card can close

It builds on card 6's `LspNavigation` and card 7's diagnostics.

User decisions this card depends on (§9, "Decided (user, 2026-10-06)"):

- **Q2 (a):** `tools/lsp` is for tests only. fastedit never installs servers for users.
- **Q4 (a):** real-server tests run in CI **on `workflow_dispatch` only**, not on PRs and not on a schedule (the user
  dropped the nightly run on 2026-10-10; a cron can be added later). The
  gate stays free of Node, network and server binaries.
- **Q5 (c):** both TypeScript servers are tested: typescript-language-server 6.0.1 with `typescript`
  6.0.3 (the default), and typescript-go 7.0.2 (`tsc --lsp --stdio`).

**The workflow file needs the user's approval:** Regatta always asks for `.github/workflows/**`.

## Change

- **`tools/lsp/package.json` + `package-lock.json`** pin exact versions:
  - `pyright` 1.1.414
  - `typescript-language-server` 6.0.1
  - `typescript` 6.0.3
  - typescript-go as an npm alias, `"typescript-go": "npm:typescript@7.0.2"`, resolved by its package
    path, since both ship a `tsc` bin

  The lockfile is generated, not hand-written. `tools/lsp/mise.toml` pins Node 24 LTS to an exact patch.
  `.gitignore` gains `tools/lsp/node_modules/`.
- **`scripts/lsp-real.sh`** runs, in order:
  1. `mise install` and `npm ci --prefix tools/lsp`
  2. `rustup component add rust-analyzer` (not added to `rust-toolchain.toml`)
  3. `cargo nextest run -p fe-lsp --run-ignored only -E 'test(/real_server_/)'`
- **`crates/fe-lsp/tests/real_server.rs`** holds the slow group, each test
  `#[ignore = "needs language servers: scripts/lsp-real.sh"]` and named `real_server_*`. Fixtures go
  under `crates/fe-lsp/tests/fixtures/real/`.
  - **Finding each binary:**
    - `FE_LSP_<SERVER>`, then `tools/lsp/node_modules/…`, then `rustup which rust-analyzer`
    - missing prints `skipped: <bin> not found (run scripts/lsp-real.sh)` and passes, unless
      `FE_LSP_REQUIRE_SERVERS=1`
  - **The tests (§6.3):**
    - definition and references through `LspNavigation`, for Rust, Python and both TypeScript servers
    - §4.4 cases 1, 3 and 7 at §4.2's columns: rust-analyzer under utf-8 and under utf-16 (offer
      `["utf-16"]`), pyright and typescript-language-server (utf-16), and typescript-go (utf-8).
      Use sites are not at a line's end.
    - readiness from launch to `Ready` within 60 s
    - clean shutdown: typescript-go's exit code 1 is a normal stop
    - rust-analyzer's diagnostics change after saving a type error introduced and then fixed
    - for each server, whether `publishDiagnostics` carries a `version` (§10; moved here from card 7)
- **`.github/workflows/lsp.yml`:**
  - `workflow_dispatch` only, no `schedule`
  - `macos-15`, a 45-minute timeout
  - `rustup toolchain install`, `Swatinem/rust-cache`, an `actions/cache` step for
    `~/.rustup/toolchains` keyed on `rust-toolchain.toml`
  - `jdx/mise-action` reading `tools/lsp/mise.toml`, and an npm cache keyed on
    `hashFiles('tools/lsp/package-lock.json')` plus the Node version
  - `FE_LSP_REQUIRE_SERVERS=1`, then `scripts/lsp-real.sh`
- **`TESTING.md`** gains a "Real language servers" section: the script, the env vars, the workflow,
  and the steps for M4's in-app go-to-definition check, which a separate human card runs.
- **`docs/decisions/lsp-client.md` §10** records what the runs showed:
  - which servers version their diagnostics
  - the rust-analyzer component's negotiated encoding
  - readiness time on this repository

## Out of scope

- Shipping servers (Q2).
- Running real servers on PRs (Q4).
- Changes to `ci.yml` or `soak.yml`.
- Any server-specific diagnostics remedy: if a server never versions them, file a follow-up.
