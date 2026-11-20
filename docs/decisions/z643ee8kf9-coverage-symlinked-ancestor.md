# Coverage paths through a symlinked ancestor (T704)

A report that spells a file through a symlinked ancestor of the project root
(`/var` -> `/private/var` on macOS, a symlinked home) matched neither the root
as opened nor its canonical form, so `Resolver::canonical` treated it as
outside the project and the record was lost.

`Resolver::canonical` now canonicalizes an absolute path that isn't inside by
prefix, when it exists. If the result is under the canonical root it is used;
otherwise the normalized path is kept as spelled, so "outside the root: never
canonicalized" still holds for paths that stay outside.

Covered by `coverage_tests::a_report_through_a_symlinked_ancestor_of_the_root_resolves_inside`
(lcov `SF:` and Cobertura `filename`), which reproduces it on Linux.
