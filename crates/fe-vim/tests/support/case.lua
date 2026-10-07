-- Records what Neovim's `~` makes of every Unicode scalar value, for
-- fe-vim's vim.toggle_case. Run by the ignored `record_case_table_with_neovim`
-- test:
--   nvim --clean --headless -l case.lua <out.txt>
-- Puts each code point alone on a row, runs `~` on every row, and writes
-- the ones it changed as hex `from:to`, eight per line.

local lines, cps = {}, {}
for cp = 1, 0x10FFFF do
  -- Not NUL or a line break, which a row can't hold as itself.
  if (cp < 0xD800 or cp > 0xDFFF) and cp ~= 10 then
    table.insert(lines, vim.fn.nr2char(cp, 1))
    table.insert(cps, cp)
  end
end
vim.api.nvim_buf_set_lines(0, 0, -1, false, lines)
vim.cmd("silent! %normal! ~")
local toggled = vim.api.nvim_buf_get_lines(0, 0, -1, false)

local toggles = {}
for i, cp in ipairs(cps) do
  if toggled[i] ~= lines[i] then
    local chars = vim.fn.str2list(toggled[i], 1)
    assert(#chars == 1, string.format("~ made %x more than one character", cp))
    table.insert(toggles, string.format("%x:%x", cp, chars[1]))
  end
end

local out = {}
for i = 1, #toggles, 8 do
  table.insert(out, table.concat(toggles, " ", i, math.min(i + 7, #toggles)))
end
local file = assert(io.open(arg[1], "wb"))
file:write(table.concat(out, "\n") .. "\n")
file:close()
