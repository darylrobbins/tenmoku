---
task_uid: "01m70mmtmsy9bcax2zgvwj4gry"
task: "T314"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "gradle_wrapper_scripts_are_bash in crates/fe-syntax/tests/java_ecosystem_names.rs passes: gradlew, mvnw, sub/gradlew and GRADLEW are bash and gradlew.bat is none"
    required: true
    manual: false
  - id: "C2"
    text: "maven_and_version_catalog_files_are_built_in passes: pom.xml is xml, gradle/libs.versions.toml is toml, Main.java is java"
    required: true
    manual: false
  - id: "C3"
    text: "gradle_files_are_claimed_by_the_first_party_grammars passes: build.gradle and settings.gradle are groovy, *.gradle.kts kotlin, gradle.properties and application.properties properties, with no extension or name clash among the three manifests"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-syntax passes"
    required: true
    manual: false
---

# Pin Gradle, Maven and properties file detection and open gradlew and mvnw as shell scripts (bash file names) (Mac T1659)

# Gradle and Maven wrapper scripts and build files: pin detection, fix gradlew/mvnw

User report (2026-11-09): "We don't support Gradle files or properties files."
Verified at main 9fde11a0c: the grammars exist and their manifests already claim
the right names, so most of this needs a test, not a fix:

- `extensions/grammars/groovy/extension.toml`: extensions groovy, gradle, gvy, gy,
  gsh; file name Jenkinsfile. So build.gradle and settings.gradle are groovy.
- `extensions/grammars/properties/extension.toml`: extension properties, file name
  gradle.properties. So application.properties and gradle.properties are properties.
- `extensions/grammars/kotlin/extension.toml`: kt, kts. build.gradle.kts and
  settings.gradle.kts end in kts, so no compound-extension change is needed.
- Built-in in `crates/fe-syntax/src/language.rs`: xml (pom.xml), toml
  (gradle/libs.versions.toml), java, bash.
- File icons for pom.xml, gradle.properties, gradlew, gradlew.bat and *.gradle(.kts)
  already exist (kiln/crates/kiln-ui/src/file_icons_coloured.rs): no icon work.

The gap: `gradlew` and `mvnw` have no extension and aren't in bash's `file_names`
(language.rs ~:725), so they open as plain text. There is no batch/cmd grammar, so
gradlew.bat and mvnw.cmd stay plain text (named in the doc comment, not a task).

## Do

- Add "gradlew" and "mvnw" to the bash Def's `file_names` in
  `crates/fe-syntax/src/language.rs` (whole names, matched ignoring case like the
  others). Nothing else in that file changes.
- New test file `crates/fe-syntax/tests/java_ecosystem_names.rs`, in the style of
  `config_file_names.rs` and `language_file_names.rs`:
  - `gradle_wrapper_scripts_are_bash`: gradlew, mvnw, sub/gradlew, GRADLEW are
    "bash" (feature-gated on `bash` like the neighbours); gradlew.bat is None.
  - `maven_and_version_catalog_files_are_built_in`: pom.xml is "xml";
    gradle/libs.versions.toml is "toml"; Main.java is "java" (each feature-gated).
  - `gradle_files_are_claimed_by_the_first_party_grammars`: with the groovy,
    properties and kotlin grammars' manifests read from extensions/grammars (use
    the helper wasm_parity.rs or runtime_grammar.rs uses to parse manifests, no
    wasm load), build.gradle and settings.gradle resolve to groovy,
    build.gradle.kts and settings.gradle.kts to kotlin, gradle.properties and
    application.properties to properties, and no two directories claim the same
    name or extension (a clash check scoped to these three).

Depends on nothing; those grammars only highlight once installed, which is card
"record the first-party grammars' hashes" (interim) or T1485's offer.
Scope is exact files; not a Windows/batch grammar, not Groovy/Kotlin servers.
Platform: Linux-OK.
