---
task_uid: "01m5h4xbhzdsk70enxbmmpxhpx"
task: "T1083"
rev: 5
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Root cause fixed in production code: one consent grant (or one changed spec) yields exactly one relaunch of the granted spec and retires exactly the old server. The commit message names the duplicate path."
    required: true
    manual: false
  - id: "C2"
    text: "Regression test lsp_consent_relaunch_tests::one_grant_relaunches_the_server_exactly_once in crates/fe-workbench/src/lsp_consent_relaunch_tests.rs drives the duplicating ordering deterministically: before changing and granting the spec it waits for a protocol event from the first launch that happens after ConsentGate::record (its initialize request or didOpen), not a launch count; it keeps the stubborn retirement barrier and the ServerId, argument, launches == 2 and kills == 1 assertions. It fails at base by assertion and passes on the candidate."
    required: true
    manual: false
  - id: "C3"
    text: "a_running_server_restarts_only_once_its_change_is_granted passes 40 runs with --retries 0 under load (24 yes burners), via cargo nextest run -p fe-workbench -E 'test(a_running_server_restarts_only_once_its_change_is_granted)'."
    required: true
    manual: false
---

# One language-server consent grant can launch the granted spec twice and kill only the old server, so lsp_consent_tests::a_running_server_restarts_only_once_its_change_is_granted times out under load

Bugfix (was a flake chore; the worker found the cause is production code).

Evidence (T1083 attempt 1): under load (24 yes burners, nextest --stress-count 300 -j 16) the test reproduced 2 times in about 750 runs. At the timeout a diagnostic in Lsp::until showed launches=3 kills=1 pending=0, launched args=[[server.js],[server.js --verbose],[server.js --verbose]]. One grant produced TWO launches of the same granted spec and only the old server was killed, so the 'taplo to restart' wait for launches()==2 overshoots and times out at 60 s. A test-side wait can't fix it.

Suspects: (1) LspManager::reconcile (crates/fe-lsp/src/manager.rs:1101) retires the old server and calls start_server (:452) while retire (:1163) / place_for (:1040) also start a new instance for the same documents; (2) crates/fe-workbench/src/lsp_consent.rs decide() apply() followed by the async evaluate() apply() hands the spec over twice. Under load the second apply lands after the first relaunch.

Context: this flake got T1038 (consent prompt) reverted after its post-merge test_all; T1038 waits on the unfreeze. A real double launch also leaks a server process per grant in the app, so this is a user-visible bug, not just a test problem.

Keep the change minimal: make the relaunch idempotent per granted spec (or apply once), no new machinery. The regression test goes in its own new file so the base run can carry it; record that file with --test-support-path if the regression run needs it.
