---
task_uid: "01m78thvazpjh4szsda2yzk4nt"
task: "T336"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`Limits` is split into interactive (today's values, unchanged) and background; `[contributes.plugin.background]` parses with units (wall time, memory, data quota field reserved for B7) and rejects unknown keys."
    required: true
    manual: false
  - id: "C2"
    text: "Host ceilings from physical RAM: per job min(2 GiB, RAM/4), all jobs min(4 GiB, RAM/2) (Q2 follows the design's proposal); a declared value above the ceiling is clamped with a diagnostic, not an error."
    required: true
    manual: false
  - id: "C3"
    text: "New tests in kiln/crates/kiln-plugin/tests/tiers.rs: `tiers_interactive_limits_are_unchanged`, `tiers_background_parses_units`, `tiers_background_over_ceiling_is_clamped`, `tiers_ceiling_follows_physical_ram` (RAM injected), `tiers_raising_background_limits_is_an_escalation`."
    required: true
    manual: false
  - id: "C4"
    text: "Wall time is at most 60 minutes and no fuel metering is added."
    required: true
    manual: false
---

# Tier model: interactive vs background limits and [contributes.plugin.background] (B1, 3.2) (Mac T1698)

# Tier model: interactive vs background limits and [contributes.plugin.background] (B1, 3.2)

Draft id B1 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 3.2 and 3.1. Caps on host queues: any counter in this card is a host-side constant with a ceiling.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- `Limits` stays the interactive struct (fields unchanged, so callers outside this card's scope keep compiling); the background tier is `BackgroundLimits` beside it, with `Ceilings` for the host's.
- Background limits are accepted per plugin in `GrantRecord::background`, checked by `evaluate_background` (a raise of `memory` or `wall` is an `Escalation` with an empty `added`). A decline covers the capabilities and the limits (`declined_background`, `GrantStore::decline_with_background`).
- Physical RAM comes from `kiln_core::platform::physical_ram_bytes` (`/proc/meminfo` on Linux, `sysctl hw.memsize` on macOS, `GlobalMemoryStatusEx` on Windows); `Ceilings::host` assumes 4 GiB only when that returns `None`.
