//! Guard (T1664): a test process must never resolve Tenmoku's state dir to the
//! user's real one, or tests overwrite the user's recents, layouts and backups.
//! The nextest setup script `scripts/nextest-isolate-state.sh` sets
//! `TENMOKU_STATE_DIR`; under plain `cargo test`, set it yourself.

#[test]
fn the_state_dir_is_not_the_users_real_one() {
    let state = fe_config::paths::state_dir().expect("a state dir resolves");
    // The real dir is what the production resolver gives with the process
    // environment minus the TENMOKU_* overrides, so every platform's home
    // fallback (HOME, USERPROFILE on Windows, XDG_STATE_HOME) matches the app's.
    let real = fe_config::paths::state_dir_from(|name| {
        if name.starts_with("TENMOKU_") {
            None
        } else {
            std::env::var_os(name)
        }
    })
    .expect("the real state dir resolves");
    assert!(
        !state.starts_with(&real),
        "state dir {} is the user's real {}: run under nextest, or set TENMOKU_STATE_DIR",
        state.display(),
        real.display()
    );
}
