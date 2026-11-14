# Claude Agent preflight (T648)

Builds on `50t8wt9we2-ai-agents.md` § 4. Code: `crates/fe-acp/src/preflight.rs`.

- **Probe seam.** A `Probe` trait (runnable, canonicalize, read a small file, run `--version`
  with a timeout); `SystemProbe` is the real one, tests use a fake over a temp dir.
  `check` takes `&dyn Probe`; callers run it off the UI thread.
- **Locations.** Configured (`~` expanded, must be absolute; if set but not runnable the answer is
  `Missing { configured: true }`, no fallback), absolute login `PATH` entries, `~/.claude/local`
  (claude only), `~/.local/bin`, mise shims, Homebrew. No location was added beyond the spec; no `npx`,
  nothing under a workspace.
- **Versions.** `claude --version` runs with home as cwd, the login `PATH`, a 5 s timeout (kill on
  timeout; field `version_timeout` so tests are fast). The leading `MAJOR.MINOR.PATCH` is parsed
  (own parser: `claude.rs`'s is private and out of scope; it also accepts a trailing text like
  ` (Claude Code)`). No floor for claude. The adapter is never run: its canonical path is walked
  up to the nearest `package.json` named `@agentclientprotocol/claude-agent-acp`. The floor is
  `claude::MIN_ADAPTER_VERSION`, not a copy.
- **Login.** No credential file is read; login state is not decided here.
