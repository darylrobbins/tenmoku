//! Arbitrary bytes through the remote protocol's decoder: `decode_all`
//! returns, an error at worst, and never panics or allocates past
//! `MAX_BODY` for a body (docs/decisions/j8wvgszah0-remote-development.md
//! § 2.1). crates/fe-remote-proto/tests/frame_garbage.rs is the same check
//! on mutated frames, in the gate.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = fe_remote_proto::decode_all(data);
});
