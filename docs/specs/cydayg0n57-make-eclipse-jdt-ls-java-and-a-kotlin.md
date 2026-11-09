---
task_uid: "01m70mmtckgjfmdkcydayg0n57"
task: "T313"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::registry::java::java_and_kotlin_get_their_servers_by_default: with default settings a java document routes to the built-in server jdtls (command jdtls, languages [java], root markers [pom.xml, build.gradle, build.gradle.kts, settings.gradle, settings.gradle.kts, .git]) and a kotlin document to the Kotlin server the decision file names"
    required: true
    manual: false
  - id: "C2"
    text: "tests::registry::java::java_and_kotlin_language_ids_pass_through: didOpen through the routed fake server carries languageId java and kotlin"
    required: true
    manual: false
  - id: "C3"
    text: "tests::registry::java::settings_can_disable_or_override_the_java_servers: enabled = false leaves the document with no server and a command override keeps args, languages and root markers"
    required: true
    manual: false
  - id: "C4"
    text: "tests::registry::java::root_is_the_marker_folder_the_decision_file_names: a Gradle multi-module file and a Maven module resolve to the documented root"
    required: true
    manual: false
  - id: "C5"
    text: "the decision file names the default Kotlin server and the other as an alternative or manual override, says what happens for a missing server, and THIRD_PARTY.md and NOTICE are unchanged"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-lsp -p fe-config passes"
    required: true
    manual: false
---

# Make Eclipse JDT LS (java) and a Kotlin language server built-in servers with root markers for pom.xml, Gradle and .git, found on the login shell's PATH like gopls (Mac T1658)

# Make Eclipse JDT LS (java) and a Kotlin server built-in language servers

User report (2026-11-09): Java ecosystem support isn't great. Java files get no
language server today unless the user writes `[lsp.servers.<name>]` by hand
(`crates/fe-lsp/src/registry.rs` `builtin_specs` has rust-analyzer, pyright,
typescript-language-server, typescript-go, gopls and
vscode-css-language-server; nothing for Java or Kotlin). Precedent: gopls (T1367,
spec docs/specs/1dbnegnewe-make-gopls-a-built-in-language-server.md) and T1644
(`crates/fe-lsp/src/tests/registry/css.rs` is the shape to copy). fastedit only
launches the server from the login shell's PATH; nothing is shipped or vendored,
so THIRD_PARTY.md and NOTICE don't change.

## Do

Add to `builtin_specs`, after the CSS server:

- `jdtls`: command `jdtls` (the Homebrew/Mason wrapper script; it speaks LSP on
  stdio with no args), languages ["java"], root_markers ["pom.xml",
  "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts",
  ".git"]. Verify the wrapper's name and stdio default against its docs (Context7
  or the eclipse-jdtls README) and say in one comment what a non-default is for.
  A multi-module Gradle/Maven repo wants the topmost marker: check how
  `registry.rs` picks the root (nearest vs outermost ancestor, see go.work vs
  go.mod for gopls) and give jdtls the settings.gradle/parent-pom behaviour if
  the existing mechanism allows it without a new field; otherwise note the
  limitation in the decision comment and the finish summary instead of adding one.
- Kotlin: ONE default, decided by you and recorded in
  `docs/decisions/<lane>-java-kotlin-servers.md` (the only doc this card adds):
  JetBrains kotlin-lsp (the maintained one; command `kotlin-lsp`, `--stdio`) or
  fwcd's kotlin-language-server (command `kotlin-language-server`, stdio, the
  common Homebrew install). Check each's current status and install story first.
  The other is listed in the docs as a manual `[lsp.servers.<name>]` override,
  OR added as a second built-in spec if the registry's one-server-per-language
  rule (docs/decisions/lsp-client.md section 3.2; `server_for` takes the first
  spec listing the language) makes that meaningful via `enabled = false` on the
  first. Languages ["kotlin"]; markers same as jdtls minus pom.xml's role but
  including build.gradle(.kts) and settings.gradle(.kts).
- Groovy/Gradle scripts get no server here (out of scope).
- Language names: fe-syntax's built-in java is "java"; kotlin comes from the
  first-party wasm grammar extensions/grammars/kotlin, languageId "kotlin".
  `language_id` passes names through (crates/fe-lsp/src/document.rs): no change.
- The server-missing hint and any docs/README server list: follow what T1367 and
  T1644 changed (rg -n gopls docs README.md crates/fe-workbench/src crates/fe-config);
  add the java and kotlin entries wherever gopls is listed, and
  `crates/fe-config/settings.schema.json` if server names are enumerated there.

## Tests (new files only)

`crates/fe-lsp/src/tests/registry/java.rs`, declared with `mod java;` in
`crates/fe-lsp/src/tests/registry.rs`, using the fake-server pattern of css.rs and
gopls.rs: routing by default, command/args/languages/root markers, languageId
pass-through, `enabled = false` and command override keep the other fields, root
is the nearest (or outermost, per the decision) marker folder, and the other
Kotlin server isn't chosen by default. Optional `#[ignore]` real-server test.

## Depends on / conflicts

T1644 (landed) set the pattern to follow: same registry.rs lines and the css.rs test layout, so the two
don't collide. Scope overlaps T1645 (design, docs only) only conceptually: if
T1645's add-on model lands first, a Java add-on is not needed here.

Platform: Linux-OK.
