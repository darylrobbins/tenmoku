---
task_uid: "01m70d6gbd4pgmpqvhgvgvhhfb"
task: "T1655"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "The linux job frees runner disk before its first cargo step (or otherwise gains a stated, measured margin), with df -h / printed before and after the freeing"
    required: true
    manual: false
  - id: "C2"
    text: "df -h / is printed immediately before and after the 'Kiln builds and tests on its own' step, and that step still builds kiln in its own target dir outside the workspace"
    required: true
    manual: false
  - id: "C3"
    text: "Swatinem/rust-cache still saves the workspace target, or the finish notes state what dropping it costs; no other job changes"
    required: true
    manual: false
  - id: "C4"
    text: "actionlint .github/workflows/ci.yml passes if actionlint is installed (else the YAML parses with a yq or python yaml load), and scripts/check-framework-standalone.sh still passes locally if changed"
    required: true
    manual: false
---

# CI (linux): the runner runs out of disk in kiln's stand-alone check after the workspace test build; free preinstalled SDKs at job start and log df around the check

GitHub CI on main has been red on every run (40+). Two causes:

1. windows / Test: kiln-l10n lint_catalogue and lint_keys tests fail on '\' path separators. NOT this card: Mac T1622 (neon T309) fixes it and is gated in stack 107, landing next.
2. linux / "Kiln builds and tests on its own" (scripts/check-framework-standalone.sh): the ubuntu-24.04 runner runs out of disk. Run 42632078776 (82a61b155), 14:35:02Z: "error: linking with `cc` failed" then "##[error]No space left on device". The Test step has already filled target/ with the whole workspace's debug test build; the standalone check then builds kiln again in its own target (${TMPDIR}/kiln-standalone-target, deliberately separate: a separate workspace shares nothing but downloads). Earlier runs (42616736230, 42602055974, 42593595288) end with that step and every later linux step unreported, the same signature. The step ran fine when it was added (6e65173d8); the workspace has grown since.

Fix it in .github/workflows/ci.yml's linux job (and the standalone script only if needed):
- Free runner disk at the start of the linux job by removing preinstalled SDKs the job never uses (e.g. /usr/share/dotnet, /usr/local/lib/android, /opt/ghc, /usr/local/.ghcup, /opt/hostedtoolcache/CodeQL, docker images), printing `df -h /` before and after; or another approach with a measured margin, stated in the commit.
- Print `df -h /` right before and after the standalone check, so the next shortfall shows in the log.
- Keep the standalone check's own target dir (its isolation is the point) and keep Swatinem/rust-cache working (don't delete the workspace target that the cache saves at job end without saying what it costs).
- Consider also CARGO_PROFILE_DEV_DEBUG=line-tables-only (or 0) for the linux job if it fits TESTING.md's CI notes; measure the target size change locally on Linux if you are on a Linux host.

Workflow edits may need a human risk approval at review; say in the finish notes what was measured and that the proof is the first main run after landing.

## Implementation notes

Linux job now removes dotnet, Android, ghc/ghcup, CodeQL, powershell, swift and docker images before its first cargo step (df -h / before and after), and prints df -h / around the standalone check (also on failure). Kiln keeps its own target dir; rust-cache is untouched; no other job changed. Not measured locally (macOS host): the freed size is shown in the first main run log, which is the proof. CARGO_PROFILE_DEV_DEBUG was not changed. actionlint passes.
