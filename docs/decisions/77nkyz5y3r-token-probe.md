# The workbench token probe allows no fixed pixels, at any scale (T1273)

- `crates/fe-workbench/examples/screenshots/token_probe.rs:ALLOWED_FIXED_PIXELS` is 0, because that's what the scene
  measures: everything it draws follows the theme, so there's no region to name in the comment or under Gaps in
  `docs/design-language.md`. The hard-coded colours listed there are in things this scene doesn't draw.
- The check isn't excused by `SCREENSHOTS_ALLOW_OTHER_SCALE`. CI sets that variable because its Mac is 1×, so a probe
  that skipped on it would never judge anything on CI. Comparing against 0 needs no scaling. A later allowance above
  zero would be in device pixels and have to be scaled for that Mac.

## Measured

On the Mac, at 2× (`<worktree>` shortens the path).

```zsh
cargo run -p fe-workbench --example screenshots
```

The result, exit 0, was the same with and without `SCREENSHOTS_ALLOW_OTHER_SCALE=1`, and the same count on every run
of the card (seven, the first five before the skip was removed):

```text
ok      workbench
token probe: 0 identical pixels, allowed 0
        diff:   <worktree>/crates/fe-workbench/../../target/screenshots/workbench_tokens.diff.png
ok      workbench tokens
```

## The check can fail

With a 20 px square of `rgb(0x123456)` temporarily added as the last child in
`crates/fe-workbench/src/workbench.rs:Workbench::render`, and `SCREENSHOTS_ALLOW_OTHER_SCALE=1` set as on CI, it found
exactly that square (40 by 40 device pixels) and exited 1:

```text
ok      workbench
token probe: 1600 identical pixels, allowed 0
  (400, 400)..=(439, 439): 1600 pixels
        diff:   <worktree>/crates/fe-workbench/../../target/screenshots/workbench_tokens.diff.png
FAILED  workbench tokens: 1600 pixels are the same under both probe themes, allowed 0: a colour bypasses the theme
```

## Not run

An actual 1× capture: this Mac's display is 2×, so CI's first run is the first at that scale. The check has no
scale-dependent branch, and the probe's reasoning doesn't depend on scale (a blend of theme A's colours can't land on
a blend of theme B's), but if a 1× capture does show fixed pixels the probe will fail CI until they're named.
