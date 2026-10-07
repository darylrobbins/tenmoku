-- Records how Neovim classifies and sizes every Unicode scalar value, for
-- fe-vim's word motions and goal columns. Run by the ignored
-- `record_char_table_with_neovim` test:
--   nvim --clean --headless -l chars.lua <out.txt>
-- Writes runs of code points with the same charclass() (0 blank, 1
-- punctuation, 2 word, 3 emoji, others a script's own class) and
-- strdisplaywidth(), as hex `start:class:width`, eight per line.

local runs = {}
local prev
for cp = 0, 0x10FFFF do
  if cp < 0xD800 or cp > 0xDFFF then
    local class, width
    if cp == 0 then
      -- A NUL in a line, shown as ^@.
      class, width = 1, 2
    else
      local s = vim.fn.nr2char(cp, 1)
      class, width = vim.fn.charclass(s), vim.fn.strdisplaywidth(s)
    end
    local key = class .. ":" .. width
    if key ~= prev then
      table.insert(runs, string.format("%x:%x:%x", cp, class, width))
      prev = key
    end
  end
end

local lines = {}
for i = 1, #runs, 8 do
  table.insert(lines, table.concat(runs, " ", i, math.min(i + 7, #runs)))
end
local file = assert(io.open(arg[1], "wb"))
file:write(table.concat(lines, "\n") .. "\n")
file:close()
