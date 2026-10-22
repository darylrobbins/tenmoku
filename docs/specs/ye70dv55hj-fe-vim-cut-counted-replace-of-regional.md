---
task_uid: "01m5fzzdfwpj7kwfye70dv55hj"
task: "T214"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "replace_regional::counted_replace_of_regional_indicators_stays_bounded, virtual_replace_breaks_oracle::huge_virtual_counts_stay_bounded and virtual_replace_oracle::a_huge_virtual_count_stays_bounded each take under 8 s in a debug build on an idle machine (cargo nextest run -p fe-vim stays_bounded stay_bounded)"
    required: true
    manual: false
  - id: "C2"
    text: "Each still compares the counted session against the same count of plain R copies (or a bound on bytes written), so per-character history or a replay of the count one copy at a time still fails it"
    required: true
    manual: false
  - id: "C3"
    text: "One new test per file, named counted_replace_of_regional_indicators_caps_at_max_put, huge_virtual_counts_cap_at_max_put and a_huge_virtual_count_caps_at_max_put, feeds the u32::MAX count once for one key sequence and asserts only the byte cap (text.len() \u003c= 65 \u003c\u003c 20 and \u003e 1 \u003c\u003c 20) with no timing; the lead puts these three in the nextest slow group"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-vim passes"
    required: true
    manual: false
---

# fe-vim: cut counted_replace_of_regional_indicators_stays_bounded, huge_virtual_counts_stay_bounded and a_huge_virtual_count_stays_bounded (73 to 88 s each) to a mid-size count (Mac T1068)

Test-only speed-up, user-approved 2026-10-21. Do not touch fe-vim/src.

What costs: with count 4294967295 the session writes the whole 64 MiB MAX_PUT in a debug build, per run. huge_virtual_counts_stay_bounded makes 3 runs (R baseline + 2 gR); a_huge_virtual_count 2; counted_replace_of_regional_indicators runs, for 3 key sets x 2 forms, a plain baseline PLUS the keys, repeating up to ROUNDS=3 when over the bound, and recomputes the baseline each round (12 to 36 runs of 64 MiB).

Plan:
1. The ratio ("took < plain * 4 + 1 s", "plain * 20 + 5 s") proves "no per-character history", which holds at any large count. Run the ratio at count 1 << 20 (about 1 MiB of text, ~64x cheaper) instead of u32::MAX. Keep the existing relative-to-R design (T661 made it load-proof); do not turn it into an absolute wall time.
2. Measure the plain R baseline once per test (not per key set and per round); keep the ROUNDS retry for the counted side.
3. The "count above MAX_PUT is clamped, text.len() <= 65 << 20" property needs the huge count, so move it to the three new cap-only tests (one key sequence each: the cheapest, e.g. `4294967295gR<Tab><Esc>`, `4294967295gRx<CR>y<Esc>` is covered by one of them, and for regional `R🇦<CR><BS>🇧<BS>🇨<Esc>` with the count form only), each one run of 64 MiB, no baseline, no clock. The lead lists them in the nextest slow group, which another card edits; do not edit .config/nextest.toml or regatta.toml here.
4. Keep the corpus checks (virtual_replace_matches_neovim, virtual_replace_breaks_match_neovim) as they are.
Notes: the fe-vim oracle recorder shares one nvim and its regex is quadratic; nothing here uses it. If a 1 << 20 run is still over the target, lower to 1 << 18.
Pure fe-vim headless, runs on Linux.
