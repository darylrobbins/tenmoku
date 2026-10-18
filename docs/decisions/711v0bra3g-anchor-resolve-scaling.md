# Anchor resolution one by one exceeds the 12x-per-10x-edits budget (T147)

- T147 benched `anchors/resolve 10k through {10,100,1000} edits` (fastest of 3 rounds, helium, load ~25–30):
  one by one 1.25 / 7.85 / 105.8 ms (100/10 = 6.3x, 1000/100 = 13.5x); batched 0.391 / 1.66 / 15.1 ms (4.2x, 9.1x).
- Only one by one at 1000/100 is over 12x, by about 12%. The machine was loaded and the 1,000-edit samples were noisy
  (medians 178–273 ms), so this may be noise.
- Follow-up: re-measure on a quiet machine; if it still exceeds 12x, find the superlinear step in `resolve` for a
  single anchor over a long edit log (fe-buffer; not changed in T147).
