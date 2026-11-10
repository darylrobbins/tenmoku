---
task_uid: "01m73194cydex41xm4p7360b5y"
task: "T567"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "scripts/sccache-wrap.sh passes every environment variable through to sccache unchanged, including names that are not shell identifiers: with the wrapper as RUSTC_WRAPPER, `env \"X_A-B=1\" scripts/sccache-wrap.sh rustc --version` style checks (or an equivalent probe the decision file shows) find X_A-B in the compiler process environment on Linux"
    required: true
    manual: false
  - id: "C2"
    text: "CARGO_TARGET_DIR is still unset for the compile, as the wrapper comment says"
    required: true
    manual: false
  - id: "C3"
    text: "rgt ci run --gate passes on the lane"
    required: true
    manual: false
---

# sccache-wrap.sh drops hyphenated env vars on Linux (dash), so env!("CARGO_BIN_EXE_<bin-with-hyphen>") fails to compile under ci-env

scripts/sccache-wrap.sh is the RUSTC_WRAPPER that scripts/ci-env.sh sets. Its shebang is `#!/bin/sh`. On Linux /bin/sh is dash, and dash drops every environment variable whose name isn't a valid shell identifier before it execs. So a variable such as `CARGO_BIN_EXE_grammar-registry`, which cargo sets for an integration test's `env!()`, never reaches rustc. On macOS /bin/sh is bash, which keeps such variables, so only Linux hosts see it.

Found on helium's sync of mac/main 1241cc27d (Mac T1483). crates/fe-registry/tests/signing.rs and tests/publish_script.rs use `env!("CARGO_BIN_EXE_grammar-registry")`, and both failed vet and test with "environment variable `CARGO_BIN_EXE_grammar-registry` not defined at compile time". Run without the wrapper (RUSTC_WRAPPER=), the same build compiles. Checked: `env 'A-B=1' /bin/sh -c 'env | grep -c ^A-B='` prints 0 on helium, and the same under bash prints 1.

Fix: run the wrapper under bash (`#!/usr/bin/env bash`), or otherwise unset CARGO_TARGET_DIR without passing the environment through a POSIX sh, so that every variable cargo sets reaches sccache and rustc unchanged. Keep the wrapper's existing behaviour (CARGO_TARGET_DIR dropped for the compile, as its comment explains).
