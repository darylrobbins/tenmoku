---
task_uid: "01m7cw15fdgnhmq5g30bkbs8zh"
task: "T648"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-acp preflight::tests passes: configured_path_wins_over_path, missing_configured_path_does_not_fall_back, found_on_login_path, relative_path_entries_are_skipped, found_in_fallback_locations (~/.claude/local, ~/.local/bin, mise shims, Homebrew)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-acp preflight::tests passes: claude_version_is_parsed (a real script, cfg(unix)), version_timeout_is_unknown, adapter_version_from_package_json (through a symlinked bin), adapter_below_floor_is_too_old, ready_needs_both"
    required: true
    manual: false
  - id: "C3"
    text: "The adapter floor is AG6's constant, not a copy (preflight.rs has no 0.106.0 literal); the adapter is never run, claude --version runs with the home directory as cwd"
    required: true
    manual: false
  - id: "C4"
    text: "preflight.rs reads no Claude Code credential file and decides nothing about login (module doc says so); design calls recorded in docs/decisions/\u003clane\u003e-*.md"
    required: true
    manual: false
---

# Claude Agent preflight in fe-acp: find claude and the ACP adapter on the login PATH, ~/.claude/local, mise shims and Homebrew, with versions: found, missing or too old (Claude Code onboarding 2) (Mac T1831)

# Claude Agent preflight in fe-acp: find `claude` and the ACP adapter, with their versions, before any session

User-approved 2026-11-13 (Claude Code onboarding in the AI chat, card 2 of 5). Builds on
`docs/decisions/50t8wt9we2-ai-agents.md` (T1779): § 4 "Locating the programs", "What fe-acp never
does" and "States the panel shows", and AG6 (T1787), whose `claude.rs` holds the adapter version
floor (0.106.0). Read those sections first.

## Why

Today Claude Agent only learns that a program is missing or too old when a session starts (AG6's
spawn and `initialize`). The chat's empty state (card 1) and the guided setup (card 3) need the
answer before any send: found and which version, missing, or too old. This card is that check,
pure and testable, in fe-acp beside AG6's launcher so both share one version floor.

## What to build

New `crates/fe-acp/src/preflight.rs`, `pub mod preflight` in `crates/fe-acp/src/lib.rs`.
fe-acp stays headless and knows nothing of the workbench or fe-lsp: the caller passes everything in.

- **Input** (`PreflightEnv` or similar): the configured `agents.claude_code.claude` and `.adapter`
  paths (empty = not set), the login shell's `PATH` as the workbench already has it
  (`fe_lsp::LoginPath::path()`, passed as `Option<OsString>`), the home directory, and the few
  environment values the fallbacks need (`MISE_DATA_DIR`, `HOMEBREW_PREFIX`).
- **A probe seam** (a trait): is this path a file this process may run, canonicalize, read a small
  file, and run `<program> --version` with a timeout. A real implementation lives in the same file;
  tests use a fake over a temp dir.
- **Where it looks**, for each of `claude` and `claude-agent-acp`, first hit wins, and the report says
  which source found it:
  1. the configured path (`~` expanded); if set but not runnable, report *missing (configured
     path)*, and do not fall back;
  2. each absolute entry of the login `PATH` (skip empty, `.` and other relative entries, as
     `LoginPath::resolve` does: the cwd may be an untrusted workspace);
  3. `~/.claude/local/claude` (claude only) and `~/.local/bin` (the native installer);
  4. mise shims: `$MISE_DATA_DIR/shims`, else `~/.local/share/mise/shims`;
  5. Homebrew: `$HOMEBREW_PREFIX/bin`, else `/opt/homebrew/bin`, `/usr/local/bin`,
     `/home/linuxbrew/.linuxbrew/bin`.
  The worker may add a location (say so in the decision file), but never `npx` and never anything
  under the workspace root.
- **Versions.**
  - `claude`: run `claude --version` with the home directory as cwd (never the workspace), the
    login `PATH`, a 5 s timeout and kill on timeout; parse the leading `MAJOR.MINOR.PATCH`.
    The record sets no floor for `claude`, so it is never "too old"; an unparsable or timed-out
    answer is *found, version unknown*.
  - the adapter: **do not run it** (it is an ACP server and would wait on stdin). Canonicalize
    the found path and walk up to the nearest `package.json` whose `name` is
    `@agentclientprotocol/claude-agent-acp`; read its `version`. Below AG6's floor constant is
    *too old*; not found is *found, version unknown* (AG6's `initialize` check still applies).
- **Output**: one report per program, `Found { path, source, version: Option<Version> }`,
  `Missing { configured: bool }` or `TooOld { path, version, floor }`, plus a `ready()` that is
  true only when both are found and neither is too old.
- **Never** read Claude Code's credentials or decide "logged in" (§ 4 "What fe-acp never does"):
  login state only comes from AG6's `authRequired` / `_auth/status_update`. Say so in the module doc.
- The probe runs nothing at startup: callers run it off the UI thread when the chat needs the answer.

## Follow-up for the lead (not in this card)

AG11 (T1792) resolves the programs with `LoginPath` alone. It should launch what this preflight
found, so the chooser and the launcher never disagree (a `claude` found only in `~/.claude/local`).
That is a respec of T1792 (add this card as a dependency), not part of this card.

## Tests

New `crates/fe-acp/src/preflight_tests.rs`, declared from `preflight.rs` as
`#[cfg(test)] #[path = "preflight_tests.rs"] mod tests;`. Fake probe over a temp dir; one real
`--version` run against a script written in the temp dir, Unix only (`#[cfg(unix)]`).

## Notes

- Record design calls (locations added, version parsing, the probe seam) in the card's decision file,
  `docs/decisions/<lane>-*.md`.
- `Cargo.toml` only if a dev-dependency is needed (a temp dir crate already in the workspace); no new
  external crate. `Cargo.lock` follows.
- Linux-OK: nothing here needs macOS.
