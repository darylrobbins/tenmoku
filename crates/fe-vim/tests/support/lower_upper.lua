-- Records what Neovim's visual `u` and `U` make of every Unicode scalar
-- value, for fe-vim's `u` and `U` over a selection. Run by the ignored
-- `record_lower_upper_tables_with_neovim` test:
--   nvim --clean --headless -l lower_upper.lua <lower.txt> <upper.txt>
-- Puts each code point alone on a row, runs `Vu` (then `VU`) on every row,
-- and writes the ones it changed as hex `from:to`, eight per line, as
-- case.lua does for `~`.

local lines, cps = {}, {}
for cp = 1, 0x10FFFF do
  -- Not NUL or a line break, which a row can't hold as itself.
  if (cp < 0xD800 or cp > 0xDFFF) and cp ~= 10 then
    table.insert(lines, vim.fn.nr2char(cp, 1))
    table.insert(cps, cp)
  end
end

local function record(keys, path)
  vim.api.nvim_buf_set_lines(0, 0, -1, false, lines)
  vim.cmd("silent! %normal! " .. keys)
  local changed = vim.api.nvim_buf_get_lines(0, 0, -1, false)
  local pairs = {}
  for i, cp in ipairs(cps) do
    if changed[i] ~= lines[i] then
      local chars = vim.fn.str2list(changed[i], 1)
      assert(#chars == 1, string.format("%s made %x more than one character", keys, cp))
      table.insert(pairs, string.format("%x:%x", cp, chars[1]))
    end
  end
  local out = {}
  for i = 1, #pairs, 8 do
    table.insert(out, table.concat(pairs, " ", i, math.min(i + 7, #pairs)))
  end
  local file = assert(io.open(path, "wb"))
  file:write(table.concat(out, "\n") .. "\n")
  file:close()
end

record("Vu", arg[1])
record("VU", arg[2])
