---
task_uid: "01m7g5mpnx0djjzkads5rtgx7s"
task: "T1880"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo run -p fe-workbench --example screenshots -- audit target/ui-audit exits 0 on the card's commit"
    required: true
    manual: false
  - id: "C2"
    text: "the decision file lists each probe that failed on main with its scene, the cause (commit) and whether the probe or the UI was wrong; any real regression found has its own filed bug"
    required: true
    manual: false
---

# UI audit exits 0 again on main: fix or re-point the ~22 probes that drifted (chrome, settings, problems, terminal, git_log, Yohen/Hakuji scenes)

`cargo run -p fe-workbench --example screenshots -- audit target/ui-audit` exits 1 on current main: about 22 probes fail in scenes no recent card touched (chrome, settings, problems, terminal, git_log, and the Yohen/Hakuji theme scenes). Two stack checks (stack 185 run, T1849's lane) saw the same 22 while their own new scenes passed. Earlier notes also recorded audit drift on main around T1571. The audit isn't a gate stage, so nothing catches the drift, and each card that adds a scene has to argue its failures away.

Do: run the audit on main, list each failing probe with its scene, the expected and found value; for each, find the commit that moved it (git log -S on the scene/probe, or bisect on the scene) and decide whether the probe or the UI is wrong. Fix the probes whose expectations are stale (update the expected token/point to the intended design), and file a bug for any probe that caught a real regression. Record the list and each decision in a decision file. End state: the audit exits 0 on main.

Platform: Mac (the audit renders through Metal).
