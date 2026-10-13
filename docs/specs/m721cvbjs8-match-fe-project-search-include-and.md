---
task_uid: "01m4tej0q5d8fqzkm721cvbjs8"
task: "T111"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-project test search_filter_relative_root::globs_under_a_relative_root_match_paths_its_name_prefixes: from relative root w holding wa/x.txt and y.txt, search_filtered finds exactly wa/x.txt with include wa/, exactly y.txt with exclude wa/x.txt, and exactly wa/x.txt with include *.txt and exclude y.txt"
    required: true
    manual: false
  - id: "C2"
    text: "PathFilter::new keeps its signature; the existing fe-project search filter tests pass unchanged"
    required: true
    manual: false
---

# Match fe-project search include and exclude globs against root-relative paths without stripping the root again, so a relative root named w still matches wa/ (Mac T645)

## Problem

`PathFilter` (crates/fe-project/src/search.rs) builds its include and exclude matchers with
`GitignoreBuilder::new(root)` (`build_globs`), but matches paths that are already relative to the root
(`includes`, `excludes`). ignore 0.4.33's `Gitignore::strip` cuts the builder's root off any candidate path
with more than one component as a raw byte prefix (`pathutil::strip_prefix`), not by component. So under a
relative root, a root-relative path whose first name starts with the root's name loses those bytes before
matching: with root `w`, the path `wa/x.txt` is matched as `a/x.txt`. Includes then drop files they name
and excludes let through files they name. An absolute root is safe only because relative paths never start
with `/`.

Checked 2026-10-13 at Mac e6988aba (search.rs is identical on helium main df738a8): in a directory holding
`w/wa/x.txt` and `w/y.txt` (both containing `needle`), with the working directory set to its parent,
`search_filtered("w", literal("needle"), threads 1, PathFilter::new("w", ["wa/"], []))` finds nothing,
where `wa/x.txt` is expected; excluding `wa/x.txt` still returns it.

## Change

In crates/fe-project/src/search.rs `build_globs`, build the matcher with an empty root
(`GitignoreBuilder::new("")`), since every path it is asked about is relative to the search root and
nothing must be stripped. `PathFilter::new` keeps its signature (its `root` argument stays, unused by the
matchers, with its doc saying the globs are relative to it). Matching under absolute roots is unchanged;
the existing search filter tests pass unchanged.

## Tests

New file crates/fe-project/tests/search_filter_relative_root.rs, holding one test only because it sets the
process working directory:

- `globs_under_a_relative_root_match_paths_its_name_prefixes` (regression, fails at base): with the
  working directory at a tempdir holding `w/wa/x.txt` and `w/y.txt`, a `search_filtered` from root `w`
  finds exactly `wa/x.txt` with include `wa/`, exactly `y.txt` with exclude `wa/x.txt`, and exactly
  `wa/x.txt` with include `*.txt` and exclude `y.txt`.
