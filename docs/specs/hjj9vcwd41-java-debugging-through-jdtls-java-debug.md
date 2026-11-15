---
task_uid: "01m7f6tx0qgfvqvnhjj9vcwd41"
task: "T660"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Test tests::debug_java::the_java_debug_jar_is_found_in_order passes (cargo nextest run -p fe-workbench debug_java): [debug.adapters.java] path, then the well-known install locations the decision file lists, else \"not found\" with a hint"
    required: true
    manual: false
  - id: "C2"
    text: "Test tests::debug_java::main_classes_from_jdtls_are_detected_configurations passes against fe-lsp's fake: vscode.java.resolveMainClass lists \"Debug \u003cClass\u003e\" entries"
    required: true
    manual: false
  - id: "C3"
    text: "Test tests::debug_java::starting_one_connects_to_the_port_jdtls_gives passes: vscode.java.startDebugSession's port is connected through D3's Port launch kind (fe-dap-fake listening there), and launch carries mainClass, projectName, and classPaths and modulePaths from vscode.java.resolveClasspath"
    required: true
    manual: false
  - id: "C4"
    text: "Test tests::debug_java::a_launch_json_java_entry_imports passes (type \"java\" maps to the java adapter), and tests::launch_defs pass"
    required: true
    manual: false
  - id: "C5"
    text: "Test tests::debug_java::a_gradle_or_maven_task_runs_as_the_pre_launch_task passes, using T1660's detected tasks"
    required: true
    manual: false
  - id: "C6"
    text: "Test tests::debug_java::without_jdtls_for_the_root_java_debugging_explains_why passes, starting no session"
    required: true
    manual: false
  - id: "C7"
    text: "cargo nextest run -p fe-dap registry passes"
    required: true
    manual: false
  - id: "C8"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist), with this card's strings in crates/fe-workbench/l10n/en-US/debug.ftl"
    required: true
    manual: false
---

# Java debugging through jdtls: java-debug's port from vscode.java.startDebugSession, main classes and classpaths from jdtls, and the java launch.json type (debugger D21b) (Mac T1858)

# Java debugging through jdtls: java-debug's port from vscode.java.startDebugSession, main classes and classpaths from jdtls, and the java launch.json type (debugger D21b)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **l**.

## Problem

With D21, jdtls can carry java-debug and answer commands; the debugger must use it.

## What to build (§13 "Scope", Java)

- **The java adapter** in D3's registry: a `Port` launch (§13: a server-provided
  port, alongside `Stdio`; the new kind crosses into fe-lsp), `type` alias `java`.
- **The jar**: from `[debug.adapters.java] path`, else well-known install locations
  (for example a VS Code `vscjava.vscode-java-debug-*/server/` or a Mason
  `java-debug-adapter` install); given to jdtls through D21's bundles. Tenmoku never
  ships or downloads it (EPL-1.0).
- **Starting**: `vscode.java.startDebugSession` over `workspace/executeCommand` for
  the root's jdtls, then connect fe-dap to the returned port; main class and
  classpath resolution through `vscode.java.resolveMainClass` and
  `vscode.java.resolveClasspath`.
- **Detection**: "Debug <MainClass>" configurations from jdtls; `launch.json` import
  gains the `java` type (§4's mapping).
- Gradle and Maven tasks (T1660, landed) serve as pre-launch tasks.

## Not in this card

Java test debugging, hot code replace, attach to a JVM (D20's attach first).

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Linux-OK** (headless tests only; no screenshot goldens move).

## Implementation notes

- Design calls, the jar's search order and the well-known locations are in
  `docs/decisions/hjj9vcwd41-java-debugging.md`.
- fe-dap: the built-in `java` adapter (`registry::JAVA`), a `Port` spec
  whose placeholder port (`JAVA_PORT`) the workbench replaces with
  `vscode.java.startDebugSession`'s answer at each start.
- fe-workbench: `debug_java.rs` (the jar, detection, the start's jdtls
  steps and their explanations); `launch_defs` gains `JavaMain` and
  "Debug <class>" detection, and a string `args` for a launch.json `java`
  configuration; `debug_session/launch.rs` builds java-debug's arguments
  (`session_options_with`); `lsp.rs` hands the jar to each new manager's
  jdtls.
- Carried T658 F19 and F17's read-failure half are fixed in
  `debug_session.rs` (`show_stop`), with a regression in
  `tests/debug_java.rs`; F16 needs fe-dap's session actor, outside this
  card's scope.
