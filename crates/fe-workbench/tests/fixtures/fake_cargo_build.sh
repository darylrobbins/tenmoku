#!/bin/sh
# A fake `cargo` for the Runs build tests (src/tests/build_runner.rs): it
# records its arguments, one run a line, in `args` beside it, then prints
# recorded `cargo build --message-format=json-render-diagnostics` output of a
# package `p` in the folder it runs in, which depends on `itoa` (up to date)
# and `ryu` (built): cargo's JSON messages on stdout, its own lines and the
# diagnostics it renders on stderr, as cargo does. `p`'s `src/lib.rs` has a
# warning at 2:9 and an error at 6:5, so it fails as cargo does.
#
# Files beside it change what it does:
#
# - `gate`: it waits after `ryu`'s artifact for a file `go` there, so a test
#   can see the counts mid-run.
# - `fixed`: the error is gone: `p` builds, with its warning.

bin=$(dirname "$0")
echo "$*" >> "$bin/args"

wait_for_go() {
    if [ -e "$bin/gate" ]; then
        while [ ! -e "$bin/go" ]; do
            sleep 0.01
        done
    fi
}

registry='registry+https://github.com/rust-lang/crates.io-index'
here=$(pwd)

printf '%s\n' '{"reason":"compiler-artifact","package_id":"'"$registry"'#itoa@1.0.11","manifest_path":"/cargo/registry/src/itoa-1.0.11/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"itoa","src_path":"/cargo/registry/src/itoa-1.0.11/src/lib.rs","edition":"2018","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":["/p/target/debug/deps/libitoa-0123456789abcdef.rlib"],"executable":null,"fresh":true}'
echo '   Compiling ryu v1.0.18' >&2
printf '%s\n' '{"reason":"compiler-artifact","package_id":"'"$registry"'#ryu@1.0.18","manifest_path":"/cargo/registry/src/ryu-1.0.18/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"ryu","src_path":"/cargo/registry/src/ryu-1.0.18/src/lib.rs","edition":"2018","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":["/p/target/debug/deps/libryu-0123456789abcdef.rlib"],"executable":null,"fresh":false}'
wait_for_go
echo "   Compiling p v0.1.0 ($here)" >&2
cat >&2 <<'EOF'
warning: unused variable: `x`
 --> src/lib.rs:2:9
  |
2 |     let x = 1;
  |         ^ help: if this is intentional, prefix it with an underscore: `_x`
  |
  = note: `#[warn(unused_variables)]` on by default

EOF
if [ -e "$bin/fixed" ]; then
    printf '%s\n' '{"reason":"compiler-artifact","package_id":"path+file://'"$here"'#p@0.1.0","manifest_path":"'"$here"'/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"p","src_path":"'"$here"'/src/lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":["/p/target/debug/deps/libp-0123456789abcdef.rlib"],"executable":null,"fresh":false}'
    echo 'warning: `p` (lib) generated 1 warning' >&2
    echo '    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.42s' >&2
    printf '%s\n' '{"reason":"build-finished","success":true}'
    exit 0
fi
cat >&2 <<'EOF'
error[E0308]: mismatched types
 --> src/lib.rs:6:5
  |
5 | pub fn two() -> u32 {
  |                 --- expected `u32` because of return type
6 |     "two"
  |     ^^^^^ expected `u32`, found `&str`

For more information about this error, try `rustc --explain E0308`.
warning: `p` (lib) generated 1 warning
error: could not compile `p` (lib) due to 1 previous error; 1 warning emitted
EOF
printf '%s\n' '{"reason":"build-finished","success":false}'
exit 101
