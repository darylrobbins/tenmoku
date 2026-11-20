# User docs scaffold: mdBook (T715)

- **mdBook** for `docs/user`, over a plain Markdown tree or a site generator:
  one static binary with no Node, a SUMMARY that builds into a navigable book
  with search, sources that stay plain Markdown and render on GitHub, and a CI
  install that takes seconds.
- **Published at https://tenmoku.dev/docs**, so `book.toml` sets
  `site-url = "/docs/"` (mdBook's 404 page needs it under a subpath).
- **The gate is `scripts/check-user-docs.sh`**, plain POSIX sh, not mdbook.
  `reference/` pages are generated (HD2) and exempt from the Status line.
- The Plugins stub links the authoring guide by its GitHub URL, since a
  relative link out of `src/` would break in the built book.
