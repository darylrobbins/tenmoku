#!/bin/sh
# A fake `cargo` for the Runs test-run tests (src/tests/test_runner.rs): it
# records its arguments, one run a line, in `args` beside it, then prints
# recorded output of a package `p`'s library tests, one passing and one
# failing, and exits as cargo does when a test fails. With `--workspace` its
# integration test `more` follows, one test that passes.
#
# `cargo test …` prints libtest's human output, cargo's own lines on stderr
# and the test binaries' on stdout, as cargo does; `cargo nextest run …`
# prints nextest's libtest-json. Files beside it change what it does:
#
# - `gate`: it waits after the first result for a file `go` there, so a test
#   can see the counts mid-run.
# - `pass`: `tests::fails` passes.
# - `abort`: the library's binary ends after `tests::fails ... FAILED`, with
#   no failure output and no `test result:` line, as an abort ends it.
# - `big`: nextest's failure output is 70000 `x`s, a line longer than a run
#   reads as one.

bin=$(dirname "$0")
echo "$*" >> "$bin/args"

wait_for_go() {
    if [ -e "$bin/gate" ]; then
        while [ ! -e "$bin/go" ]; do
            sleep 0.01
        done
    fi
}

if [ "$1" = "nextest" ]; then
    printf '%s\n' '{"type":"suite","event":"started","test_count":2}'
    printf '%s\n' '{"type":"test","event":"started","name":"p$tests::adds"}'
    printf '%s\n' '{"type":"test","event":"ok","name":"p$tests::adds","exec_time":0.001}'
    wait_for_go
    printf '%s\n' '{"type":"test","event":"started","name":"p$tests::fails"}'
    if [ -e "$bin/big" ]; then
        big=$(head -c 70000 /dev/zero | tr '\0' x)
        printf '%s\n' '{"type":"test","event":"failed","name":"p$tests::fails","exec_time":0.002,"stdout":"'"$big"'\n"}'
    else
        printf '%s\n' '{"type":"test","event":"failed","name":"p$tests::fails","exec_time":0.002,"stdout":"thread '"'"'tests::fails'"'"' panicked at src/lib.rs:16:9:\nassertion `left == right` failed\n  left: 3\n right: 4\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n"}'
    fi
    printf '%s\n' '{"type":"suite","event":"failed","passed":1,"failed":1,"ignored":0,"measured":0,"filtered_out":0,"exec_time":0.003}'
    exit 100
fi

echo '   Compiling p v0.1.0 (/p)' >&2
echo '    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s' >&2
echo '     Running unittests src/lib.rs (target/debug/deps/p-0123456789abcdef)' >&2
echo ''
echo 'running 2 tests'
echo 'test tests::adds ... ok'
wait_for_go
if [ -e "$bin/pass" ]; then
    echo 'test tests::fails ... ok'
    echo ''
    echo 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
    echo ''
    failed=
else
    echo 'test tests::fails ... FAILED'
    if [ -e "$bin/abort" ]; then
        echo 'error: test failed, to rerun pass `-p p --lib`' >&2
        echo '' >&2
        echo 'Caused by:' >&2
        echo '  process didn'"'"'t exit successfully: `target/debug/deps/p-0123456789abcdef` (signal: 6, SIGABRT: process abort signal)' >&2
        exit 101
    fi
    echo ''
    echo 'failures:'
    echo ''
    echo '---- tests::fails stdout ----'
    echo "thread 'tests::fails' panicked at src/lib.rs:16:9:"
    echo 'assertion `left == right` failed'
    echo '  left: 3'
    echo ' right: 4'
    echo 'note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace'
    echo ''
    echo ''
    echo 'failures:'
    echo '    tests::fails'
    echo ''
    echo 'test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
    echo ''
    echo 'error: test failed, to rerun pass `-p p --lib`' >&2
    failed=1
fi

case " $* " in
*" --workspace "*)
    echo '     Running tests/more.rs (target/debug/deps/more-fedcba9876543210)' >&2
    echo ''
    echo 'running 1 test'
    echo 'test subtracts ... ok'
    echo ''
    echo 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
    echo ''
    ;;
esac

if [ -n "$failed" ]; then
    echo 'error: 1 target failed:' >&2
    echo '    `-p p --lib`' >&2
    exit 101
fi
exit 0
