-- Records what Neovim does with each corpus case, for fe-vim's oracle tests.
-- Run by the ignored `record_corpus_with_neovim` test:
--   nvim --clean --headless -l oracle.lua <cases.json> <results.json>
-- Each case is {name, buffer, cursor = {row, byte_col}, keys}, rows from 0.

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

local cleared = {}
for reg in ('"0123456789-abcdefghijklmnopqrstuvwxyz/'):gmatch(".") do
  table.insert(cleared, reg)
end
local recorded = {}
for reg in ('"0123456789-abcdefghijklmnopqrstuvwxyz'):gmatch(".") do
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
  local keys = vim.api.nvim_replace_termcodes(case.keys, true, false, true)
  vim.api.nvim_feedkeys(keys, "xt", false)

  local cursor = vim.api.nvim_win_get_cursor(0)
  local registers = vim.empty_dict()
  for _, reg in ipairs(recorded) do
    local text = vim.fn.getreg(reg)
    if text ~= "" then
      registers[reg] = { text = text, linewise = vim.fn.getregtype(reg) == "V" }
    end
  end
  table.insert(results, {
    name = case.name,
    buffer = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n"),
    cursor = { cursor[1] - 1, cursor[2] },
    mode = vim.api.nvim_get_mode().mode,
    registers = registers,
  })
end

local file = assert(io.open(output, "wb"))
file:write(vim.json.encode(results))
file:close()
