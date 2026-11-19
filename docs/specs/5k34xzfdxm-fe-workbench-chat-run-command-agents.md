---
task_uid: "01m7reb6hngjyqmv5k34xzfdxm"
task: "T1971"
rev: 1
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "GH CI windows Test passes chat_tool_loop::tests::a_stopped_commands_end_never_lands_on_a_later_call, chat_tool_loop::tests::trust_withdrawn_during_a_permission_wait, chat_tool_loop::tests::withdrawal_discards_every_finished_calls_output, agents_panel::agents_panel_tests::stop_ends_lane_1s_turn_with_a_permission_prompt_waiting"
    required: true
    manual: false
  - id: "C2"
    text: "GH CI windows Test passes terminal_panel::terminal_lane_tests::a_terminal_lanes_own_tab_is_silver_blue_and_writable, terminal_panel::terminal_lane_tests::an_agents_terminal_is_silver_blue_named_for_its_lane_and_takes_no_typing, terminal_panel::terminal_lane_tests::lane_tabs_take_the_agent_token_not_the_accent_in_both_themes"
    required: true
    manual: false
  - id: "C3"
    text: "GH CI windows Test passes chat_panel::agent_actions::tests::agent_actions::denied_files_are_refused_as_attachments"
    required: true
    manual: false
  - id: "C4"
    text: "macOS and Linux runs of these tests are unchanged; tests only"
    required: true
    manual: false
---

# fe-workbench chat run_command, agents panel and terminal lane tests on Windows: they run "sh" on CommandPath "/usr/bin:/bin" or /bin/sh as the terminal shell; and agent_actions writes .env only on unix

# Unix shell fixtures in chat, agents and terminal-lane tests

GH CI 44405849017 on 8ba6f008c, windows Test:
- chat_tool_loop_tests.rs `ready()` (:110) and agents_panel_tests.rs (:815) set `CommandPath("/usr/bin:/bin")`, a unix PATH with ':' separators, and run `{"program": "sh", ...}`. On Windows `sh` isn't found, so no permission prompt waits: "a prompt waited about call_1" (:180), `left: []` (:348), `(false, [])` (:853). Use `std::env::join_paths` with the POSIX sh helper's folder on Windows.
- terminal_lane_tests.rs:30 sets the lane terminal's shell to `/bin/sh` with an OSC 133 printf/read loop. On Windows the lane tab never appears ("index out of bounds: the len is 1 but the index is 1", `left: 1 right: 2`). Use the sh helper, or a cmd.exe equivalent as tests::printing_shell does.
- tests/agent_actions.rs:205 creates `.env` only inside `#[cfg(unix)]` (next to the symlinks), so on Windows `file(".env")` refuses nothing and only 2 toasts show (:236, `left: 2 right: 3`). Write `.env` on every platform and keep only the symlinks unix-only.

chat_tool_loop's a_hand_in_under_way_ends_only_its_own_turn and edits_run_in_the_calls_order_whatever_is_answered_first aren't covered here; the "outside the folder" card covers them.
