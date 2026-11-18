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
- `quick-exit`: review r3 F6 (T1682): a command that leaves a TERM-resistant child
  (`quick-child`) and exits at once. `MODE=exit`, or `MODE=hang` (a sub-command does it
  while the stand-in waits, for `--timeout`).
- `bad-status`: review F7 (T1682): puts directories where the anchor writes the
  command's exit status, so the write fails, and exits 0 at once. The run must end
  (exit 1), not wait forever.
- `status-cases`: review F7 (T1682): not a stand-in xcodebuild. It runs the script, each
  run bounded at 90 s:
  - with `E2E_ANCHOR_HOLD`, a fifo nothing writes, pausing the status write while the
    watchdog fires (`quick-exit`, `--timeout 4`): it must exit 124;
  - with `bad-status`: it must exit 1.

  Either way no stand-in process may be left, running or stopped. Run it directly
  (optionally with another script as `$1`); it exits 1 if a case fails.
- `no-identity/ps`: review r3 F6 (T1682): a `ps` that can't read the run anchor's start
  time. With its folder first on `PATH`, a run must fail ("can't read the identity of the
  run's anchor") without starting the stand-in it names.
- `scan-cases`: review F4 (T1682, rounds 2 and 3): not a stand-in xcodebuild. It runs the
  script's own scan on synthetic `ps` snapshots: a group kept while its leader, alive or
  a zombie, is the one recorded, retired once it's reaped, never adopted when its id is
  reused (the new leader alive, a zombie or reaped), the anchored group, pid identity by
  start time, and the app path matched literally. Run it directly; it exits 1 if a case
  fails.
- `group-reuse`: review r3 F4 (T1682): not a stand-in xcodebuild. It runs the script's
  own `cleanup_run` against a real group whose id a forged record claims (the leader
  reaped, or a zombie), and checks the group's member (`bystander`) survives; a control
  with the zombie leader's real start time checks it is killed. Run it directly; it
  exits 1 if a case fails.
- `hang-ignore-term`, `exit-zero`, `short-run` (needs `MOCK_XCRESULT`, a real result
  bundle): the T1661 record's other stand-ins.
- `resister`, `quitter`, `setsid`, `late-parent`, `late-helper`, `late-child`,
  `spawn-on-term`, `spawner`: their parts. `helper-resist` and `helper-orphan` are links
  to `resister`, and `loop-child`, `quick-child` and `bystander` to `late-child`, under
  names `pgrep` can find.
