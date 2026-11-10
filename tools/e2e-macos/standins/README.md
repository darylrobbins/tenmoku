# Stand-ins for xcodebuild

Harmless scripts that `scripts/e2e-macos.sh` runs in place of `xcodebuild` when
`E2E_XCODEBUILD` names one, to exercise its watchdog, its cleanup of launched processes and
its pass checks without a UI session. None of them launches the app. See
`docs/decisions/n5krsav71f-xcuitest-suite.md` for each one's expected result, and run them
with `--no-build`, for example:

    MODE=hang E2E_XCODEBUILD=tools/e2e-macos/standins/late-orphan \
        scripts/e2e-macos.sh --no-build --timeout 8
    pgrep -fl 'e2etest|late-|helper-|standins/'   # finds nothing afterwards

The `Mock*-e2etest` copies they leave under the build products are removed by the next
stand-in run that makes them, and are inert.

- `family`: two runners outside the group, one TERM-resistant with a TERM-resistant child,
  one that leaves a TERM-resistant orphan (T1661). `MODE=hang` or `MODE=exit`.
- `late-orphan`: F1 (T1682): an orphan starts a TERM-resistant child after its parent has
  exited. `MODE=hang` or `MODE=exit`.
- `hang-ignore-term`, `exit-zero`, `short-run` (needs `MOCK_XCRESULT`, a real result
  bundle): the T1661 record's other stand-ins.
- `resister`, `quitter`, `setsid`, `late-parent`, `late-helper`, `late-child`: their parts.
  `helper-resist` and `helper-orphan` are links to `resister`, under names `pgrep` can find.
