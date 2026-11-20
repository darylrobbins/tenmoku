# Coverage watch: symlinked and glob folders (T705)

A live watcher only owed a report check for paths in searched places. A removal of a
symlinked report folder's target, or of a folder a `coverage.report` glob matched, named a
path that no longer canonicalized, so the old report stayed loaded.

`CoverageSource` now remembers (`known_dirs`, refreshed at each look for a report) every
folder a searched path passes through, wildcard matches listed, with their canonical
targets. `fs_changed` treats a path at or under one as owing a check and a rediscovery, even
once it is gone. Public API is unchanged.
