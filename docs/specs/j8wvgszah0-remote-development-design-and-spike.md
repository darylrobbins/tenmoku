---
task_uid: "01m7th2h316pgzqcj8wvgszah0"
task: "T1986"
rev: 2
type: "docs"
complexity: "l"
criteria:
  - id: "C1"
    text: "docs/decisions/\u003ckey\u003e-remote-development.md exists and decides transport, protocol framing and encoding (with measured reasons), server shape and lifetime, deploy/version policy (the server downloaded from the GitHub release matching the client version, never bundled in the app), host trait set, ProjectLocation, reconnect, port forwarding, trust, and what stays local (AI agents and chat local-only and not offered on remote projects in v1; dev containers out of v1 beyond the transport seam)"
    required: true
    manual: false
  - id: "C2"
    text: "tools/remote-spike runs two multiplexed channels over a child stdio with cancel, credits and an injected-latency transport, and the decision file records its measured round trip, throughput, 1 MB open and 50k-file tree-walk numbers"
    required: true
    manual: false
  - id: "C3"
    text: "docs/designs/remote-development.md describes connect, first-connect install, reconnect, ports and trust end to end"
    required: true
    manual: false
  - id: "C4"
    text: "No file under crates/ or kiln/ is modified"
    required: true
    manual: false
---

# Remote development: design and spike. Settle the ssh stdio protocol, headless tenmoku-server shape, host traits, reconnect and trust model in a decision file, with a measured throwaway spike

# Remote development: design and spike

Plan: see the survey and architecture in the lead's remote-dev plan (summarised here). Tenmoku should edit a project on another machine (SSH host first, then WSL; dev containers are NOT in v1: only the transport seam stays) with the UI local and files, search, watching, language servers, debug adapters, terminals, tasks and git running remotely.

## Deliver
1. Decision file docs/decisions/<key>-remote-development.md settling:
   - Transport: system `ssh -T host tenmoku-server --stdio` (ssh config, agent, ProxyJump, askpass prompts reuse the fe-git askpass pattern); the transport is a trait so WSL (`wsl.exe -d`) is a later impl and containers (`docker exec -i`) can be one after v1 (user: no dev containers in v1, keep only the seam).
   - Protocol: one multiplexed framed stream (length prefix, channel id, request id, cancel, per-channel flow-control credits, heartbeat, handshake with strict protocol AND build-version match). Pick the encoding (e.g. postcard or bincode over serde, vs protobuf) with a measured reason; bound frame sizes; fuzzable decoder.
   - Server shape: headless `tenmoku-server` binary, no GPUI, builds on Linux and macOS, calling fe-project, fe-lsp, fe-dap, fe-terminal, fe-git. Lifetime, grace period for reconnect, one server per host+user or per project, deploy path `~/.cache/tenmoku-server/<version>/`, arch detection, upgrade policy. Distribution (user decision): the server is NOT bundled in the app; it is downloaded from the GitHub release whose tag matches the client's exact version (`v<version>`, previews included), one asset per os/arch with a checksum or signature the client verifies. Decide whether the host fetches it itself (curl/wget) or the client downloads, verifies and uploads it over the ssh connection, the fallback for a host with no internet, and what a dev build with no matching release does (e.g. an explicit local server path).
   - The host trait set (FsHost, WatchHost, SearchHost, ProcessHost, PtyHost, GitHost) with local impls = today's behaviour, and which existing crates get which trait (fe-project has no FS seam today; fe-workbench run_process/task_run spawn directly; fe-terminal opens its pty locally; fe-lsp ServerLauncher and fe-dap AdapterLauncher already exist).
   - `ProjectLocation` type and where it is threaded (recents, reopen, IPC handoff, undo/backup keys, window identity).
   - Reconnect model, queued saves, stamp conflict flow, port forwarding, remote workspace trust and where it is stored, what stays local (UI, buffers, undo, vim, syntax, plugins). AI agents and chat (user decision): they stay local and are not offered on remote projects in v1; record how a remote window hides or disables them.
   - Latency rule: typing never awaits the network; list every operation that is async and its UI state.
   - A phase list with cards and which can be built and tested on Linux.
2. A throwaway spike under tools/remote-spike/ proving: framed multiplexing of two channels over a child process's stdio with cancel and credits, a loopback injected-latency transport, and measured numbers (round trip, throughput, open of a 1 MB file, a 50k-file tree walk streamed) written into the decision file. The spike is not shipped code and must not be a dependency of any crate.
3. docs/designs/remote-development.md: the user workflow end to end (Open Remote, ssh:// CLI, recents, first connect, reconnect, ports, trust), with the pill and dialog states described for later design cards.

## Out of scope
Any change to crates/*. Introducing the traits is phase 1+.
