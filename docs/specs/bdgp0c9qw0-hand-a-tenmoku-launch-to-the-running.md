---
task_uid: "01m6enxrj5eeftatbdgp0c9qw0"
task: "T1311"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "ipc::tests::request_round_trips_json_with_targets_positions_new_window_wait_cwd"
    required: true
    manual: false
  - id: "C2"
    text: "ipc::tests::stale_socket_with_no_listener_is_removed_and_the_process_becomes_the_server"
    required: true
    manual: false
  - id: "C3"
    text: "ipc::tests::socket_dir_is_0700_and_socket_0600_and_path_under_104_bytes"
    required: true
    manual: false
  - id: "C4"
    text: "instance_handoff::second_launch_routes_files_to_the_live_window_and_exits_0 (GPUI test, server side with a fake client)"
    required: true
    manual: false
  - id: "C5"
    text: "instance_handoff::new_window_opens_a_second_window_on_the_target_folder"
    required: true
    manual: false
  - id: "C6"
    text: "instance_handoff::tenmoku_no_ipc_env_skips_both_sides"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Hand a tenmoku launch to the running instance over a Unix socket, with --new-window, and detach from the terminal

## Problem
Each `tenmoku` launch is a new process: `main.rs` always calls `kiln::application().run`. No single-instance or IPC mechanism exists (rg for sockets/locks finds none). The only way into a running app is macOS open-documents events (T1272: `application.on_open_urls`, `CFBundleDocumentTypes`, `fe_workbench::open_documents`), which the user's zsh `tk()` uses via `open -na Tenmoku --args <abs path>` (a second process, by design of `-n`). Those events carry only a path: no `line:col`, no `--new-window`, no completion signal. A terminal `tenmoku file` also blocks its terminal while the app runs.

## Change
New `crates/fastedit/src/ipc.rs`: a Unix-domain-socket handoff, `<std::env::temp_dir()>/tenmoku-<uid>/ipc.sock` (parent dir mode 0700, socket mode 0600; the macOS 104-byte path limit holds under $TMPDIR). Protocol: one JSON line request `{targets:[{path,line,col}], new_window, wait, cwd}`, one JSON line reply `{ok}`. Startup in `main.rs`: parse argv (the B1 `cli::Cli`); try to connect. Connected: send the request, print nothing, exit 0 once replied (`wait` is carried in the request but the server answers immediately until the --wait card). Connection refused or no socket: remove the stale socket and become the server. A server launched from a terminal without `--wait` detaches first (re-exec itself with `TENMOKU_FOREGROUND=1`, new process group, stdio null, then the parent exits 0), so the terminal returns at once; with `--wait` the parent stays as the client. The server listens on a background thread and hands requests to the GPUI main thread over the existing `async_channel` pattern; a request is routed by `fe_workbench::open_documents` rules plus `open_document_at`; `--new-window` always opens a new window on the target's folder; the app is activated. A launch by Finder/`open` (no `--perf`, started by LaunchServices) still works through `on_open_urls` unchanged. Add one `dev` escape: `TENMOKU_NO_IPC=1` skips both sides, so tests and `cargo run` stay isolated.
