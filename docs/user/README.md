# Tenmoku user guide

The sources of the user documentation, an [mdBook](https://rust-lang.github.io/mdBook/).
It is published at <https://tenmoku.dev/docs>.

## Preview

```sh
mdbook serve docs/user --open
```

## Structure check

`scripts/check-user-docs.sh` needs no mdBook. It checks that every `SUMMARY.md`
entry has a file and every file is listed, that relative links resolve, that
each page has an H1, and that each page has a status line (generated `reference/` pages are exempt).

Link `#fragments` are not checked: only the file part of a relative link must exist.

The checker reads only inline links with the destination on one line, not wrapped in `<...>`; reference-style definitions, `<...>` destinations and unclosed links and destinations containing `(` fail it, as do SUMMARY list lines other than `- [Title](path)`, and fenced code is skipped (indented code blocks are not recognised). Known limit: links in SUMMARY.md outside chapter entries are not resolved (tracked as T721).

## Stub convention

A page that is not written yet is a stub: an H1, a `> Status: stub` line, and
one paragraph saying what the page will cover. Change the status line (for
example to `> Status: draft`) when real content lands; keep a `> Status:` line
on every page.

## Generated pages

Pages under `src/reference/` are generated. Do not edit them by hand; the
placeholders there keep a stub status line until the generator replaces them.

## Why mdBook

mdBook over a plain Markdown tree or a site generator, because it is one static
binary with no Node, builds the SUMMARY into a navigable book with search, the
sources stay plain Markdown that renders on GitHub, and CI installs it in
seconds.
