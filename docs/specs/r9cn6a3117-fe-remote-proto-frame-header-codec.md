---
task_uid: "01m7xbb5t4qapx11r9cn6a3117"
task: "T394"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The header is D § 2.1's 12 bytes (u32 LE len, u8 kind, u8 flags, u16 LE chan, u32 LE id) with `MAX_BODY` 1 MiB and `CHUNK` 64 KiB; crates/fe-remote-proto/tests/frame_bounds.rs shows the decoder refuses a length over the bound before allocating, an unknown kind, non-zero flags and a short length"
    required: true
    manual: false
  - id: "C2"
    text: "`decode_all(\u0026[u8]) -\u003e Result\u003cVec\u003cHeader\u003e\u003e` only ever returns an error on 200,000 mutated and truncated frames (tests/frame_garbage.rs), and fuzz/fuzz_targets/remote_frames.rs drives the same function and is listed in fuzz/Cargo.toml and fuzz/README.md"
    required: true
    manual: false
  - id: "C3"
    text: "A proptest round trip covers every body type (tests/body_roundtrip.rs), including a path that is not UTF-8 (paths are `Vec\u003cu8\u003e` on the wire)"
    required: true
    manual: false
  - id: "C4"
    text: "`fe_remote_proto::BUILD` is `\u003cworkspace version\u003e+\u003ccommit\u003e[-dirty]` from a build script, and a test checks it starts with the workspace version; the crate is not in scripts/check-no-gpui.sh's UI_CRATES and that script passes"
    required: true
    manual: false
---

# fe-remote-proto: frame header codec, postcard bodies, byte paths, the build id, a fuzz target and round-trip proptests (remote 2a) (Mac T2040)

# fe-remote-proto: frames and bodies (remote 2a)

D § 2.1 (framing, bounds, fuzzable decoder, byte paths), § 2.2 (kinds, channel numbers: 0 control, 1 fs, 2 watch, 3 search, 4 git, 5 process control, dynamic from 64), § 2.3 (postcard, already in Cargo.lock through wasmtime: add no new encoding crate), § 2.5 (Hello/Welcome/Refused bodies, the build string).
Start from the spike: `tools/remote-spike/src/frame.rs` and `codec.rs` (which used `String` paths: use bytes here). Body types here are control (Hello, Welcome, Refused, Open, Ping/Pong, Credit, Cancel, End status incl. `Cancelled` and `LinkLost`) plus the fs, watch and search op bodies the server card needs (D § 5 FsHost/WatchHost/SearchHost, § 7 `WriteExpect`, `Conflict{current}`). Later phases add their own bodies; the strict build match (§ 2.5) means no compatibility rule.
Stream channels' `Chunk{offset, bytes}` and `Resume`/`StreamResume` bodies (§ 7) belong to T1991; define the types if they fall out naturally, else leave them.
`tenmoku --version` is not changed here (open question for the lead; D § 2.5 only needs the build id inside the handshake).

## Rules
Read the decision sections named above in `docs/decisions/j8wvgszah0-remote-development.md` (and the design file `docs/designs/remote-development.md` where cited) before starting; they are the spec, this card only cuts a slice of them. Do not widen --scope; if a file outside it is needed, stop and respec (notes and choices go in your lane's `docs/decisions/<lane>-*.md`, which is exempt). New tests go in new files, one fn per behaviour, small fixtures, no sleeps for timing (D § 12 spike notes). Gate tests on the affected crates only. Not here: anything the decision assigns to another phase (D § 13).

## Implementation notes

The choices this card made where D left room are in
`docs/decisions/r9cn6a3117-remote-proto-frames.md`: per-channel body checks in
`decode_all` (dynamic-channel `Data` stays opaque), uploads as `FsData::Bytes`
chunks after `WriteAtomic{len}`, `Location` as Hello's wire form of the remote
location, `Chunk` and stream `Credit` acks left to T1991, and the build
script's watch list and its refusal to build without a commit. The fuzz workspace now repeats the root's
`tree-sitter` patch, since without it none of its targets compiled.
