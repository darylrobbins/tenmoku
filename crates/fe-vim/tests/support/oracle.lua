-- Records what Neovim does with each corpus case, for fe-vim's oracle tests.
-- Run by the ignored `record_corpus_with_neovim` test:
--   nvim --clean --headless -l oracle.lua <cases.json> <results.json>
-- Each case is {name, buffer, cursor = {row, byte_col}, keys}, rows from 0.
-- Keys after a failed command record nonsense: nvim flushes the keys queued
-- after an error (a failed search's E486 among them), so end a case there.
-- A result also holds `visual = {row, byte_col}`, the visual anchor
-- (getpos('v')), only when the case ends in a visual mode (blockwise too), and
-- `marks = {"<": {row, byte_col}, ">": ...}` for the marks that are set.

local input, output = arg[1], arg[2]

local function read(path)
  local file = assert(io.open(path, "rb"))
  local text = file:read("*a")
  file:close()
  return text
end

-- Match fe-editor's default tab size; everything else is --clean's default.
vim.o.tabstop = 4
vim.o.shiftwidth = 4
vim.o.softtabstop = 0
vim.o.expandtab = true
vim.o.clipboard = ""
-- No clipboard provider, so no keys can read or write the real macOS
-- clipboard: `"+`, `"*` and the like fail instead. A global, not an option;
-- set before anything loads the provider.
vim.g.loaded_clipboard_provider = 1
assert(vim.fn.has("clipboard") == 0, "a clipboard provider is still reachable")
-- --clean still loads the matchit plugin, whose `%` (in normal, visual and
-- operator-pending modes) isn't Vim's: `d%` within a line then skips
-- register 1. Record the builtin `%` (`:help %`, `:help quote_number`).
for _, mode in ipairs({ "n", "x", "o" }) do
  for _, lhs in ipairs({ "%", "g%", "[%", "]%" }) do
    pcall(vim.keymap.del, mode, lhs)
  end
end
assert(vim.fn.maparg("%", "o") == "", "matchit still maps %")

local cleared = {}
for reg in ('"0123456789-abcdefghijklmnopqrstuvwxyz/'):gmatch(".") do
  table.insert(cleared, reg)
end
-- `/` is the last search pattern.
local recorded = {}
for reg in ('"0123456789-abcdefghijklmnopqrstuvwxyz/'):gmatch(".") do
  table.insert(recorded, reg)
end

local results = {}
for _, case in ipairs(vim.json.decode(read(input))) do
  -- A fresh buffer, no registers and no last character search.
  local old = vim.api.nvim_get_current_buf()
  vim.cmd("enew!")
  vim.cmd("bwipeout! " .. old)
  for _, reg in ipairs(cleared) do
    vim.fn.setreg(reg, {})
  end
  vim.fn.setcharsearch({ char = "" })

  vim.api.nvim_buf_set_lines(0, 0, -1, false, vim.split(case.buffer, "\n", { plain = true }))
  vim.api.nvim_win_set_cursor(0, { case.cursor[1] + 1, case.cursor[2] })
  -- do_lt: `<lt>` is `<`, as fe-vim's parse_keys reads it.
  local keys = vim.api.nvim_replace_termcodes(case.keys, true, true, true)
  vim.api.nvim_feedkeys(keys, "xt", false)

  local cursor = vim.api.nvim_win_get_cursor(0)
  local mode = vim.api.nvim_get_mode().mode
  local visual = nil
  if mode == "v" or mode == "V" or mode == "\22" then
    local pos = vim.fn.getpos("v")
    visual = { pos[2] - 1, pos[3] - 1 }
  end
  local marks = nil
  for _, name in ipairs({ "<", ">" }) do
    local pos = vim.fn.getpos("'" .. name)
    if pos[2] ~= 0 then
      local col = pos[3] - 1
      -- A linewise '> is at v:maxcol: record its row's last byte.
      if col >= vim.v.maxcol - 1 then
        local line = vim.api.nvim_buf_get_lines(0, pos[2] - 1, pos[2], false)[1] or ""
        col = math.max(#line - 1, 0)
      end
      marks = marks or vim.empty_dict()
      marks[name] = { pos[2] - 1, col }
    end
  end
  local registers = vim.empty_dict()
  for _, reg in ipairs(recorded) do
    local text = vim.fn.getreg(reg)
    if text ~= "" then
      local regtype = vim.fn.getregtype(reg)
      local register = { text = text, linewise = regtype == "V" }
      -- A blockwise register's type is `<C-v>` and its width.
      if regtype:sub(1, 1) == "\22" then
        register.block_width = tonumber(regtype:sub(2))
      end
      registers[reg] = register
    end
  end
  table.insert(results, {
    name = case.name,
    buffer = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n"),
    cursor = { cursor[1] - 1, cursor[2] },
    mode = mode,
    registers = registers,
    visual = visual,
    marks = marks,
  })
end

local file = assert(io.open(output, "wb"))
file:write(vim.json.encode(results))
file:close()
