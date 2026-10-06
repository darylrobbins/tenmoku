---
task_uid: "01m48tbnqjt8tqdtn2m5m90mpr"
task: "T100"
rev: 1
type: "docs"
complexity: "l"
criteria:
  - id: "C1"
    text: "docs/decisions/lsp-client.md exists with the ten numbered sections named in the spec (Summary and recommendation, Client crate and runtime, Server lifecycle and process management, Document sync and position encoding, Hand-off from search-based navigation, Testing, Feature scope for M4, Proposed fe-lsp cards, Questions for the user, Unanswered)"
    required: true
    manual: false
  - id: "C2"
    text: "Client crate and runtime recommends one client crate and one lsp-types version, naming the versions read, and gives for it and for each alternative considered (at least tower-lsp or tower-lsp-server, lsp-server, lapce-proxy's approach, helix-lsp, and a hand-written client) the licence from crate metadata or LICENSE file; it states which async runtime fe-lsp uses, whether that adds tokio or another runtime to a core crate, how results reach the UI thread without GPUI, and lists any dependency licence outside deny.toml's allowlist or says there is none"
    required: true
    manual: false
  - id: "C3"
    text: "Server lifecycle and process management sketches a named host-pluggable process/transport trait with its default local-process implementation, states how it relates to fe-syntax's existing Spawner trait, and gives a policy for instances per language and root, initialize/shutdown, readiness, crash restart with backoff, cancellation and timeouts"
    required: true
    manual: false
  - id: "C4"
    text: "Document sync and position encoding recommends incremental or full sync from fe-buffer's BufferEvent edits, states the positionEncoding offer order and the encoding each of rust-analyzer, pyright and typescript-language-server picks (with the version and the evidence), maps each encoding onto existing or missing fe-text conversions, and lists concrete UTF-16 emoji test cases with expected positions in each encoding"
    required: true
    manual: false
  - id: "C5"
    text: "Hand-off from search-based navigation either cites the filed fe-nav task handles and aligns with their seam, or describes a NavigationProvider seam abstractly (precedence, readiness, stale-request cancellation, replace vs merge); Testing names the fake in-process server approach, how each real server is installed and pinned on macOS dev machines and the macos-15 CI runner with its licence, and which tests run in the gate versus a slower group, with the gate needing no Node or network"
    required: true
    manual: false
  - id: "C6"
    text: "Proposed fe-lsp cards is an ordered list of titles with one line each and their dependencies; Questions for the user lists each open decision with options and a recommendation, covering at least process ownership when embedded, installing or bundling servers, tokio in a core crate, and when real-server tests run in CI"
    required: true
    manual: false
  - id: "C7"
    text: "The candidate changes only docs/decisions/lsp-client.md (plus its own spec file); no Cargo.toml or Cargo.lock changes and no crates/fe-lsp directory"
    required: true
    manual: false
---

# Decide the fe-lsp client design: crate, server lifecycle, document sync and test servers

# Decide the fe-lsp client design: crate, server lifecycle, document sync and test servers

M4 "Navigate", second half. Treat this as a decision task, like T70 (the hjkl evaluation). The
output is one decision document that the user rules on before any `fe-lsp` implementation card is
written, and **no code merges**. The `fe-nav` (search-based navigation) cards are drafted
separately and can run in parallel with this one.

## Problem

The plan fixes only a sketch of `fe-lsp`:

- Architecture table: "`fe-lsp` | `async-lsp` client, server lifecycle, document sync,
  `positionEncoding` negotiation, `Spawner` trait | GPUI: no". It is a core crate, so
  `scripts/check-no-gpui.sh` must stay green for it.
- Reuse map: "LSP | `async-lsp` (executor-agnostic) and one `lsp-types` version | MIT/Apache |
  depend", and "IME, undo coalescing, ghost-text layout, LSP trait shapes | gpui-kit | Apache |
  reference". It also notes gpui-kit's editor has "wrong UTF-16 handling for LSP".
- Extension seams: "`Spawner`" and "`NavigationProvider` (search-based → LSP → SCIP later)".
- M4: "`fe-lsp` (diagnostics, definition, references, hover, completion)", done when "Go-to-def
  works instantly by search, then precisely via rust-analyzer, pyright and tsserver; UTF-16 emoji
  tests pass".
- Verification: "LSP integration tests"; manual check of "go-to-def before and after the LSP is
  ready".

Those lines are second-hand and leave the important choices open. fastedit will be embedded in an
AI dev tool (the host), which may want to own the server processes. We need evidence and a
recommendation before cards fix shapes.

Facts in the repo the doc must account for:

- `fe-syntax` already has a `Spawner` trait (`crates/fe-syntax/src/driver.rs`, from T1/T24): it
  spawns background *jobs* and `fe-gpui/src/view.rs` implements it on GPUI's executor. The plan's
  `fe-lsp` `Spawner` is about server *processes*. Say whether they are one trait, two traits, or
  whether the fe-lsp seam needs a different name.
- `fe-text` already has UTF-16 conversions (`PointUtf16`, `offset_to_point_utf16`,
  `point_utf16_to_offset` in `crates/fe-text/src/lib.rs`). `fe-buffer` has
  `Transaction` origins including `Lsp` (`crates/fe-buffer/src/buffer.rs`) and
  `BufferEvent` with edit ranges.
- `Cargo.lock` already contains `tokio`, `smol`, `async-process` and `async-io` through existing
  dependencies. Say which come from GPUI and which a new runtime would add.
- The gate is `fmt, vet, build, test, no_gpui, syntax_features, deny` (`regatta.toml`); CI runs
  on `macos-15` (`.github/workflows/ci.yml`); slow property runs live in the on-demand `soak.yml`.

## How to work

- Research from crate tarballs or clones **outside this repository** (`curl
  https://static.crates.io/crates/<name>/<name>-<ver>.crate` into a temp dir, or `git clone`).
  A throwaway spike (for example a tiny binary that starts rust-analyzer, pyright and
  typescript-language-server over `async-lsp` and prints the negotiated `positionEncoding`) is
  encouraged, but it lives in a scratch dir outside the repo and is described, not merged.
- Do **not** add any dependency to a `Cargo.toml` here, and don't touch `Cargo.lock`. Don't create
  `crates/fe-lsp`. The only file this task changes is `docs/decisions/lsp-client.md`.
- Licensing (`deny.toml`, `CLEAN_ROOM.md`): permissive only; MPL only as an unmodified dependency;
  GPL never. Clean room:
  - **Never open Zed's `lsp` crate** or any other Zed GPL crate (`project`, `language`, `editor`
    …), not even to compare.
  - Helix's `helix-lsp` is MPL-2.0: you may assess it as a candidate dependency, but nothing from
    it is copied or paraphrased into our design.
  - gpui-kit (Apache-2.0) and Lapce's `lapce-proxy` (Apache-2.0) are fine to read and cite.
- Cite evidence: file paths with line numbers at the version or commit you read, `cargo tree` /
  licence-metadata output, or spike output.
- Time box: one lane session. Leave something unanswered rather than overrun, and say what in the
  last section.

## Output: `docs/decisions/lsp-client.md`

It has these numbered sections:

1. **Summary and recommendation.** One paragraph per decision in sections 2–7, each with a single
   recommendation, then the list of questions for the user (section 9) repeated in short.
2. **Client crate and runtime.**
   - Confirm or replace `async-lsp`: version read, licence in crate metadata and `LICENSE` file,
     release cadence over the last 12 months, open versus closed issues, MSRV against our
     pinned toolchain (`rust-toolchain.toml`), and how many crates it adds to the lock with the
     features we'd enable.
   - Runtime fit: does it need tokio, and for what (process stdio, timers)? Can it run on GPUI's
     executor, on `smol`/`async-process`, or does `fe-lsp` need its own I/O thread? Which runtime
     crates are already in our lock and from where? Recommend one arrangement, and how results
     reach the UI thread without `fe-lsp` depending on GPUI.
   - `lsp-types`: which version (0.95 vs 0.97 or later; the `Uri` change), and whether it supports
     `positionEncoding` and the LSP 3.17 features M4 needs.
   - Alternatives considered, each with its licence and one line on why not: at least
     `tower-lsp`/`tower-lsp-server` (server-side), `lsp-server` (rust-analyzer's, sync), Lapce's
     `lapce-proxy` approach, `helix-lsp` (MPL), and writing a thin JSON-RPC client ourselves.
   - Any runtime dependency licence outside `deny.toml`'s allowlist, or "none".
3. **Server lifecycle and process management.**
   - The process seam for an embedding host: a trait (name it) that lets the host supply a
     server's stdin/stdout or a whole transport, so the host can sandbox, containerise or proxy
     servers; the default implementation that spawns a local process. A Rust signature sketch is
     fine (prose plus a short code block, not a crate).
   - Relationship to `fe-syntax`'s existing `Spawner` (see Problem).
   - Instances: one server per (language, workspace root)? Multi-root workspaces and
     `workspaceFolders`; how a server is chosen for a file; how server commands and settings are
     configured (TOML via `fe-config`, or host-provided).
   - Initialize / initialized / shutdown / exit sequence; readiness ("LSP is ready" for the
     fe-nav hand-off: after `initialize`, or after the server's first progress `end`?).
   - Crash and restart policy (backoff, give-up limit, re-sending open documents), cancellation
     (`$/cancelRequest`), request timeouts, and what the user sees while a server is down.
   - Server-to-client requests we must answer (`workspace/configuration`,
     `window/workDoneProgress/create`, `client/registerCapability`, `workspace/applyEdit`) and
     which origin (`Lsp`) applied edits carry.
4. **Document sync and position encoding.**
   - Incremental vs full sync with `fe-buffer`: how `BufferEvent` edit ranges become
     `didChange` content changes, ordering and versioning, batching per frame, and what happens
     with large files (the plan's 50 MB threshold).
   - `positionEncoding` negotiation: offer `utf-8` first and `utf-16` as fallback (UTF-32 too?);
     what each of rust-analyzer, pyright and typescript-language-server actually picks (from the
     spike or their source/docs, with version). Map each encoding onto `fe-text`: which existing
     conversions cover it and what is missing (for example a UTF-8 column `Point` or
     `PointUtf8`), and their cost on long lines.
   - The "UTF-16 emoji tests pass" acceptance: define it concretely — a list of cases (astral-plane
     emoji, ZWJ sequences, combining marks, CRLF, a position past end of line, a surrogate split)
     and, for each, the expected LSP position in each encoding and how a test checks it.
5. **Hand-off from search-based navigation.** How `fe-nav` results give way to LSP results for
   definition, references and symbols. Run `rgt task search nav` and `rgt task search
   NavigationProvider`; if the fe-nav cards are filed, align with the seam they define and cite
   the task handles. If not, describe the seam abstractly: a `NavigationProvider` with a
   precedence order, readiness signal, cancellation of a stale request, and whether search
   results are shown first and then replaced, merged, or only used while the LSP is not ready.
6. **Testing.**
   - A fake in-process LSP server for most tests (scripted responses over an in-memory duplex,
     no subprocess): what it must support, and which crate it is built on.
   - Real servers: how rust-analyzer, pyright and typescript-language-server (plus `typescript`)
     are installed and **pinned** on macOS dev machines and on the `macos-15` CI runner
     (rustup component, Homebrew, `npm`/`mise`, checked-in lockfile, cache key), each with its
     licence. Whether we ever bundle servers is a user question, not a decision here.
   - Which tests run in the fast gate (`cargo nextest run --workspace`) versus a slower group
     (nextest test group, `#[ignore]`, a feature, or a separate CI job like `soak.yml`), and how a
     real-server test skips cleanly when the server is missing. Target: the gate never needs Node
     or a network.
7. **Feature scope for M4.** For diagnostics, definition, references, hover and completion: the
   LSP methods and capabilities used, what lands in `fe-lsp` versus `fe-editor`/`fe-gpui`, and
   what is explicitly out of M4 (code actions, rename, formatting, semantic tokens, inlay hints,
   signature help). Note where diagnostics meet the plan's copied `DiagnosticSet`.
8. **Proposed fe-lsp cards.** An ordered list of implementation cards, each a title plus one
   line, with dependencies between them and on fe-nav cards, for the lead to draft after the user
   decides. Expect roughly 6–10.
9. **Questions for the user.** Each a decision only the user can make, with the options, the
   recommendation, and what changes downstream under each answer. Include at least: who owns
   server processes when embedded (host vs fastedit); whether fastedit ever installs or bundles
   servers; tokio in a core crate or not; and whether real-server tests run in CI on every PR or
   on demand.
10. **Unanswered.** What the time box didn't cover.

## Decided (lead defaults; the user may revisit)

- Docs only. Any spike stays outside the repo and is described, not merged.
- `fe-lsp` stays GPUI-free, per the plan's architecture table.
- One `lsp-types` version across the workspace.
