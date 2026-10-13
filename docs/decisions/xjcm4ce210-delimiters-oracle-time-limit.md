# delimiters_matches_neovim raises the search time limit (T622)

- `delimiters_matches_neovim` calls `fe_vim::search::set_time_limit(60 s)` first. The oracle compares results with
  nvim, not the time limit; T582 (make `yit_count_2_over_pattern_names` fast) is the real fix.
- Proof, 10 runs with all 14 cores loaded by 28 `yes > /dev/null`:

  ```zsh
  for i in $(seq 10); do cargo nextest run -p fe-vim --test delimiters_oracle; done
  ```

  Result: 10 of 10 passed, 2.9–4.3 s each (0.56 s unloaded).
