---
task_uid: "01m6ezvby1bxgk5rj3khcj00hp"
task: "T416"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test task_problems_tests::rustc_blocks_are_matched (cargo nextest run -p fe-workbench) passes: 'error[E0308]: mismatched types' followed by ' --\u003e src/main.rs:12:5' yields one Error at src/main.rs 12:5 with the message 'mismatched types (E0308)'; a warning likewise; a help/note line yields nothing"
    required: true
    manual: false
  - id: "C2"
    text: "Test task_problems_tests::single_line_matchers (cargo nextest run -p fe-workbench) passes: tsc 'src/a.ts(3,7): error TS2322: msg', gcc/clang 'a.c:10:3: warning: msg [-Wx]', go 'main.go:5:2: msg' and eslint-unix 'a.js:1:2: msg [Error/rule]' each yield the right path, 1-based line and column, severity and message"
    required: true
    manual: false
  - id: "C3"
    text: "Test task_problems_tests::python_traceback_uses_the_last_frame (cargo nextest run -p fe-workbench) passes: a traceback yields one Error at the innermost 'File \"x.py\", line N' with the exception line as the message"
    required: true
    manual: false
  - id: "C4"
    text: "Test task_problems_tests::ansi_and_paths (cargo nextest run -p fe-workbench) passes: ANSI colour sequences are stripped before matching, relative paths resolve against the task's cwd, and a path outside the project root or that does not exist (via a fake exists) is dropped"
    required: true
    manual: false
  - id: "C5"
    text: "Test task_problems_tests::unknown_matcher_and_noise (cargo nextest run -p fe-workbench) passes: an unknown matcher name returns an error value (not a panic), 20,000 lines of non-matching text return nothing in under a second, and at most 500 problems are returned with a truncated flag"
    required: true
    manual: false
---

# Parse task output into file, line, column, severity and message with built-in problem matchers for rustc/cargo, tsc, gcc/clang, go, eslint-unix and python (Mac T1330)

## Problem
The user asked for task output to reach the Problems panel if cheap. This is the pure half: text in, problems out (f8e integrates).

## Change
New crates/fe-workbench/src/task_problems.rs: `Matcher` enum chosen by name (`rustc`, `tsc`, `gcc`, `go`, `eslint`, `python`; default for a Cargo task is `rustc`, npm tasks none, unset otherwise none), `match_output(matcher, text, cwd, root, exists: &dyn Fn(&Path)->bool) -> MatchResult { problems: Vec<TaskProblem{path,line,column,severity,message}>, truncated }`. Hand-written line parsers (no regex crate if not already a fe-workbench dependency; check Cargo.toml and use `regex` only if present). No custom user-defined matchers in this card.
