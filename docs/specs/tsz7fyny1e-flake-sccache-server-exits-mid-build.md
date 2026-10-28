---
task_uid: "01m5yanqw8xbz5r6tsz7fyny1e"
task: "T1204"
rev: 3
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "scripts/ci-env.sh exports SCCACHE_IDLE_TIMEOUT=\"${SCCACHE_IDLE_TIMEOUT:-0}\" and SCCACHE_IGNORE_SERVER_IO_ERROR=\"${SCCACHE_IGNORE_SERVER_IO_ERROR:-1}\" inside the existing sccache block (where SCCACHE_DIR and SCCACHE_CACHE_SIZE are defaulted), so a caller's value wins and nothing changes when sccache is absent or RUSTC_WRAPPER is set"
    required: true
    manual: false
  - id: "C2"
    text: "Nothing outside scripts/ci-env.sh changes, except this lane's own docs/specs file"
    required: true
    manual: false
  - id: "C3"
    text: "Proof (no regression test, this is an environment setting): a shell check shows both variables reach the rustc wrapper's environment with their defaults (0 and 1), and that caller-set values (SCCACHE_IDLE_TIMEOUT=60, SCCACHE_IGNORE_SERVER_IO_ERROR=0) are kept; and the gate passes on the Mac. Confirming the fix on argon's next post-merge build is the lead's follow-up after landing, not a criterion here"
    required: true
    manual: false
---

# Flake: a broken sccache server fails the build (Connection reset by peer, os error 104, on argon); set SCCACHE_IGNORE_SERVER_IO_ERROR=1 and SCCACHE_IDLE_TIMEOUT=0 in scripts/ci-env.sh

T234's post-merge build on argon failed: sccache reported "Connection reset by peer (os error 104)" while compiling fe-vim. That was infra, not code, and it has frozen argon's merges since 10-25. By default an sccache client fails the build when it can't talk to its server. sccache's README: "To have sccache instead gracefully failover to the local compiler without stopping, set the environment variable SCCACHE_IGNORE_SERVER_IO_ERROR=1" (src/commands.rs: on an IO error it compiles locally instead of returning the error; UnexpectedEof already falls back). Set it in the sccache block of scripts/ci-env.sh with ${VAR:-1}. Keep SCCACHE_IDLE_TIMEOUT=0 from rev 1 (the server never idles out mid-gate), as ${VAR:-0}. Out of scope: a server that hangs rather than resets (seen on neon 10-27, clients in wait_woken) is not an IO error and needs a separate fix. No regression test.
