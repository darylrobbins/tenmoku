---
task_uid: "01m7c01fn1bh29k08bqvp72am3"
task: "T636"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax --features regex,csharp,java --test regex_injections_csharp_java passes: csharp_verbatim_regex_is_injected, csharp_new_regex_is_injected, csharp_escaped_string_with_backslash_stays_a_string, java_pattern_compile_without_backslash_is_injected, java_escaped_backslash_string_stays_a_string, other_calls_are_not_injected"
    required: true
    manual: false
  - id: "C2"
    text: "Limit stated in both file headers: verbatim/raw strings injected whole; escaped strings and Java text blocks only when their content has no backslash"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-syntax --features csharp --test wasm_parity csharp_extension_is_as_built_in passes; csharp and java goldens regenerated and highlights_match_golden_files passes"
    required: true
    manual: false
---

# Inject regex into C# Regex.* and Java Pattern.compile / String.matches strings (Mac T1825)

# Inject regex into C# Regex.* and Java Pattern.compile / String.matches

Depends on F (built-in `regex` grammar), C (csharp query/golden) and D1 (java `Def`, `language.rs`, golden).
Inject the `regex` language into:

- C#: new `crates/fe-syntax/queries/csharp-injections.scm` wired as the csharp `Def`'s `injections:
  Some(include_str!("../queries/csharp-injections.scm"))` in `crates/fe-syntax/src/language.rs`:
  `Regex.IsMatch/Match/Matches/Replace/Split` static calls (pattern argument, 2nd for the static
  overloads with the input first: `Regex.IsMatch(input, pattern)`; the instance form `new Regex(pattern)` and
  `new Regex(pattern, options)` first argument). Verbatim strings `@"..."` and raw `"""..."""` strings injected
  whole; ordinary strings only when the content has no backslash (host text equals regex text). State the limit
  in the file header. The csharp extension keeps its own `extensions/grammars/csharp/injections.scm`: do not edit
  it here; mention in the finish notes that the wasm csharp extension stays regex-less until someone decides.
- Java: new `crates/fe-syntax/queries/java-injections.scm` wired as the java `Def`'s `injections`:
  `Pattern.compile(...)`, `Pattern.matches(regex, input)` (first argument), `String.matches/replaceAll/replaceFirst/split`
  (matched by method name `matches|replaceAll|replaceFirst|split` with a string-literal first argument; the receiver's type is not
  knowable, so accept the over-match and say so in the header).
  Java has no raw/verbatim string except text blocks `"""..."""`, injected whole only when the content has no
  backslash, like ordinary strings (an escaped `\\d` is two source characters). That makes most Java regexes
  uninjected; the header and criteria say so, and the limit is the chosen one.

Features: `csharp = ["dep:tree-sitter-c-sharp", "regex"]`, `java = ["dep:tree-sitter-java", "regex"]`. Update
THIRD_PARTY.md's csharp/java rows to list the injection files as other rows do (php row: "`queries/php-injections.scm`⁸").
Regenerate `tests/golden/csharp.cs.golden` and `java.java.golden` (add a regex call if the sample lacks one).

**Test.** New file `crates/fe-syntax/tests/regex_injections_csharp_java.rs`,
`#![cfg(all(feature = "regex", feature = "csharp", feature = "java"))]`: `csharp_verbatim_regex_is_injected`
(`Regex.IsMatch(s, @"\d+")`: `\d` escape, `+` operator), `csharp_new_regex_is_injected`,
`csharp_escaped_string_with_backslash_stays_a_string`, `java_pattern_compile_without_backslash_is_injected`
(`Pattern.compile("a+b")`), `java_escaped_backslash_string_stays_a_string` (`Pattern.compile("\\d+")`),
`other_calls_are_not_injected`. Base fails. Run with the wasm parity guard: `cargo nextest run -p fe-syntax
--features csharp --test wasm_parity csharp_extension_is_as_built_in` still passes (highlights unchanged here).

Conflicts: language.rs (A, D1, F before this), csharp.cs/java.java goldens (C, D1 before this). Linux-OK.
