# remote-spike (T1986)

Throwaway spike for remote development, measured for
[docs/decisions/j8wvgszah0-remote-development.md](../../docs/decisions/j8wvgszah0-remote-development.md) § 12.
It is not shipped code. It has its own `[workspace]`, so it is not a member of
fastedit's workspace and no crate can depend on it.

What it proves:

- **Framing:** a 12-byte header and a postcard body (`src/frame.rs`), with a
  bounded decoder that never panics on garbage.
- **Multiplexing:** two channels over one child process's stdio
  (`src/client.rs`, `src/server.rs`).
- **Credits:** per-channel byte credits that the consumer returns.
- **Cancel:** bounded by the window.
- **Injected latency:** a transport that adds latency, with an optional
  bandwidth cap (`src/transport.rs`, `Delayed`).
- **Handshake:** both peers refuse a protocol or build mismatch.

```sh
cargo test --release            # 4 unit + 8 end-to-end tests
cargo run --release -- codec    # postcard vs bincode 2 vs serde_json vs prost
cargo run --release -- bench    # round trip, throughput, 1 MiB open, 50k-file walk
cargo run --release -- bench --quick --latency 0,25 --window 1048576 --capped 25,100
```

`bench` creates its fixture once: a 50k-file tree and a 1 MiB file, under
`$TMPDIR/remote-spike-fixture`, or `--dir`. The directory must be new, empty,
or one the spike made (it writes a `.remote-spike-fixture` marker). Any other
directory is refused, and the spike never deletes anything it didn't create.

To run over real ssh:

1. Build the spike on the host.
2. Run `remote-spike fixture --dir /tmp/remote-spike-fixture` there.
3. From the client, run
   `remote-spike bench --latency 0 --ssh HOST --remote-bin /path/on/host/remote-spike --dir /tmp/remote-spike-fixture`.
   `--dir` must name the same path on both sides.
