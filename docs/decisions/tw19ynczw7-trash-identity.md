# T91: trash identity on macOS

`trash_target` compares URL ids with URL ids and `st_dev` with `st_dev` through
`names_the_entry`; no URL volume id is compared with a `st_dev` or `f_fsid`.

## C3: macOS cross clippy at revision 1f021bf

```
CC_aarch64_apple_darwin=true AR_aarch64_apple_darwin=true CARGO_TARGET_DIR=<scratch dir> \
  cargo clippy -p fe-project --all-targets --target aarch64-apple-darwin -- -D warnings
```

Output (passed, exit 0):

```
    Checking fe-project v0.1.0 (/home/daryl/code/fastedit-lanes/.worktrees/tw19ynczw7/crates/fe-project)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.76s
```
