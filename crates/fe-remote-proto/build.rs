//! Sets `TENMOKU_BUILD`, the build id both ends of the remote handshake must
//! agree on (docs/decisions/j8wvgszah0-remote-development.md § 2.5):
//! `<version>+<commit>[-dirty]`. See `build_id.rs`.

#[path = "build_id.rs"]
mod build_id;

use std::env;

fn main() {
    let version = env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_id.rs");
    println!("cargo:rerun-if-env-changed=TENMOKU_BUILD_COMMIT");
    let commit_override = env::var("TENMOKU_BUILD_COMMIT").ok();
    let git = build_id::Git {
        program: "git".into(),
        dir: env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR")
            .into(),
    };
    let build = build_id::build_id(&version, commit_override.as_deref(), &git)
        .unwrap_or_else(|e| panic!("{e}"));
    for path in &build.watch {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rustc-env=TENMOKU_BUILD={}", build.id);
}
