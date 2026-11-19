# fe-workbench's test coverage reports (coverage.rs, T700):
# `workbench-coverage-…`. What the gutter's coverage toggle says when a
# report can't be shown.

## Why a coverage report didn't load

# $path is the report's path; $error is the system's reason.
workbench-coverage-read-failed = Couldn't read the coverage report { $path }: { $error }
workbench-coverage-empty = The coverage report is empty
# $line is the report's first line, cut short.
workbench-coverage-unsupported = Unsupported coverage report: { $line } (lcov and Cobertura XML are read)
workbench-coverage-doctype = Coverage reports with a DOCTYPE aren't read: it can declare entities
# $name is the entity's name, as in &name;.
workbench-coverage-entity = Coverage reports with entities aren't read: &{ $name };
# $line is the 1-based line where the XML goes wrong.
workbench-coverage-malformed = The coverage report isn't well-formed XML at line { $line }
# $line is the 1-based line of an lcov record that doesn't parse.
workbench-coverage-lcov-malformed = The coverage report's record at line { $line } doesn't parse
workbench-coverage-truncated = The coverage report ends inside a file's record: it may still be being written
# $limit is in MiB.
workbench-coverage-too-large = The coverage report is over { $limit } MiB
workbench-coverage-too-many-files = The coverage report covers more than { $limit } files
workbench-coverage-too-many-lines = The coverage report has more than { $limit } line records
# $limit is in KiB.
workbench-coverage-line-too-long = Line { $line } of the coverage report is over { $limit } KiB
workbench-coverage-load-failed = Loading the coverage report stopped unexpectedly

## Why no coverage report was found

workbench-coverage-none = No coverage report found: write lcov.info or coverage.xml at the project's top, or set coverage.report
# Only coverage.py's .coverage data file is there, which isn't a report.
workbench-coverage-database = Only coverage.py's .coverage data was found: Run coverage xml or coverage lcov
# $setting is the coverage.report setting, as written.
workbench-coverage-setting-no-match = coverage.report { $setting } matches no file in the project
# $limit is how many paths a glob may match.
workbench-coverage-setting-too-broad = coverage.report { $setting } matches more than { $limit } paths: narrow it
workbench-coverage-setting-outside = coverage.report { $setting } is outside the project
