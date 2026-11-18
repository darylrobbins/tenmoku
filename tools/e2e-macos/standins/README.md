# Stand-ins for xcodebuild

Harmless scripts that `scripts/e2e-macos.sh` runs in place of `xcodebuild` when
`E2E_XCODEBUILD` names one, to exercise its watchdog, its cleanup of launched processes and
its pass checks without a UI session. None of them launches the app. See
`docs/decisions/n5krsav71f-xcuitest-suite.md` for each one's expected result, and run them
with `--no-build`, for example:

    MODE=hang E2E_XCODEBUILD=tools/e2e-macos/standins/late-orphan \
        scripts/e2e-macos.sh --no-build --timeout 8
    pgrep -fl 'e2etest|standins/|helper-resist|helper-orphan'   # finds nothing afterwards

The `Mock*-e2etest` copies they leave under the build products are removed by the next
stand-in run that makes them, and are inert.

- `family`: two runners outside the group, one TERM-resistant with a TERM-resistant child,
  one that leaves a TERM-resistant orphan (T1661). `MODE=hang` or `MODE=exit`.
- `late-orphan`: F1 (T1682): an orphan starts a TERM-resistant child after its parent has
  exited. `MODE=hang` or `MODE=exit`.
- `term-spawner`: review F1 (T1682): a recorded process that, on TERM, starts a
  TERM-resistant child and exits at once. `MODE=hang` or `MODE=exit`.
- `spawn-loop`: review F1 (T1682): a TERM-resistant process that keeps starting
  TERM-resistant children, each in a session of its own, right up to the KILL.
  `MODE=hang` or `MODE=exit`.
- `build-only`: review F3 (T1682): builds the test bundle for real with the script's
  `CODE_SIGN_ENTITLEMENTS`, runs no test, prints the runner's signed read-write exception
  and exits 1. Run it with a `CARGO_TARGET_DIR` holding `&` (and a `release/tenmoku`).
- `fake-launch`: review F5 (T1682): writes launch records naming the bundle under test
  (and, with `MODE=other`, one naming another executable) and exits 0. Run it with a
  `CARGO_TARGET_DIR` holding regex characters, such as `<tmp>/build[1]`: the audit must
  count 0 (or, with `MODE=other`, 1) other processes.
- `scan-cases`: review F4 (T1682): not a stand-in xcodebuild. It runs the script's own
  scan on synthetic `ps` snapshots: a group kept after its leader exits, retired once
  empty, never adopted when its id is reused, pid identity by start time, and the app
  path matched literally. Run it directly; it exits 1 if a case fails.
- `hang-ignore-term`, `exit-zero`, `short-run` (needs `MOCK_XCRESULT`, a real result
  bundle): the T1661 record's other stand-ins.
- `resister`, `quitter`, `setsid`, `late-parent`, `late-helper`, `late-child`,
  `spawn-on-term`, `spawner`: their parts. `helper-resist` and `helper-orphan` are links
  to `resister`, and `loop-child` to `late-child`, under names `pgrep` can find.
