-- Records what Neovim does with each corpus case, for fe-vim's oracle tests.
-- Run by the ignored `record_corpus_with_neovim` test:
--   nvim --clean --headless -l oracle.lua <cases.json> <results.json>
-- Each case is {name, buffer, cursor = {row, byte_col}, keys}, rows from 0,
-- optionally oracle_keys, fed instead of keys (a command nvim doesn't have,
-- swapped for one that does the same), and optionally window_rows: the window is then that many rows high from
-- before the cursor is set, with 'scroll' half of it, and the result holds
-- `top`, the first row it shows (line('w0') - 1). 'scrolloff' is the
-- default, 0.
-- A case may hold `folds`, {first, last} rows to close, and `soft_wrap`.
-- Keys after a failed command record nonsense: nvim flushes the keys queued
-- after an error (a failed search's E486 among them), so end a case there.
-- A result also holds `visual = {row, byte_col}`, the visual anchor
-- (getpos('v')), only when the case ends in a visual mode (blockwise too), and
-- `marks = {"<": {row, byte_col}, ">": ..., "a": ...}` for the marks among
-- `<`, `>` and `a` to `z` that are set.
-- A case may hold `mapping`, lines run with vim.cmd just before the keys
-- (`nnoremap Y y$`, `let mapleader = ","`, `nunmap Y`), which fe-vim reads
-- with its vimrc parser. Maps aren't reset with the buffer, so after each
-- case every global map it added, changed or removed is put back as it was
-- (maplist() saved before, then nvim_del_keymap and mapset), nvim's
-- defaults (`Y` → `y$`) included, and so is g:mapleader. A cycle's E223
-- flushes the keys after it as any error does, so it must be the case's
-- last key. Keys are fed with no delay, so a prefix with more keys after
-- it never times out, and a case must not end on a prefix that could
-- still grow.
-- A case may hold `set`, `:set` arguments (`ignorecase smartcase`) run
-- with vim.cmd just before the keys; 'ignorecase' and 'smartcase' are put
-- back off before each case.

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

-- The window's own height, which cases without window_rows keep.
local default_rows = vim.api.nvim_win_get_height(0)
local function set_rows(rows)
  vim.api.nvim_win_set_height(0, rows)
  assert(vim.api.nvim_win_get_height(0) == rows, "the window is " .. rows .. " rows high")
  vim.wo.scroll = math.max(math.floor(rows / 2), 1)
end

-- The global maps, as maplist() has them, by their modes and keys.
local function global_maps()
  local maps = {}
  for _, map in ipairs(vim.fn.maplist()) do
    if map.buffer == 0 then
      maps[map.mode_bits .. " " .. map.lhsraw] = map
    end
  end
  return maps
end

local function holds(maps, map)
  return vim.deep_equal(maps[map.mode_bits .. " " .. map.lhsraw], map)
end

-- maplist()'s mode_bits, each a mode nvim_del_keymap takes.
local mode_bits = { n = 1, x = 2, o = 4, c = 8, i = 16, l = 32, s = 64, t = 128 }

-- Puts the global maps back to `saved`: each map that isn't one of them
-- is deleted, in each of its modes, then each one missing is set again.
local function restore_maps(saved)
  for _, map in pairs(global_maps()) do
    if not holds(saved, map) then
      for mode, mask in pairs(mode_bits) do
        if bit.band(map.mode_bits, mask) ~= 0 then
          pcall(vim.api.nvim_del_keymap, mode, map.lhs)
        end
      end
    end
  end
  local now = global_maps()
  for _, map in pairs(saved) do
    if not holds(now, map) then
      vim.fn.mapset(map)
    end
  end
  assert(vim.deep_equal(global_maps(), saved), "the maps are put back")
end

local results = {}
for _, case in ipairs(vim.json.decode(read(input))) do
  -- A case's `:set` ends with it.
  vim.o.tabstop = 4
  vim.o.shiftwidth = 4
  vim.o.expandtab = true
  vim.o.clipboard = ""
  vim.o.wrap = true
  vim.o.ignorecase = false
  vim.o.smartcase = false
  -- A fresh buffer, no registers and no last character search.
  local old = vim.api.nvim_get_current_buf()
  vim.cmd("enew!")
  vim.cmd("bwipeout! " .. old)
  for _, reg in ipairs(cleared) do
    vim.fn.setreg(reg, {})
  end
  vim.fn.setcharsearch({ char = "" })
  -- Nor a jumplist, file marks or history from earlier cases: the buffer
  -- is wiped, but these belong to the session.
  vim.cmd("clearjumps")
  vim.cmd("delmarks A-Z0-9")
  for _, history in ipairs({ "search", "cmd", "input" }) do
    vim.fn.histdel(history)
  end

  -- The load isn't a change: a file loads with no undo history, so `u` and
  -- `:undo 0` stop at the buffer as it was loaded.
  local undolevels = vim.bo.undolevels
  vim.bo.undolevels = -1
  vim.api.nvim_buf_set_lines(0, 0, -1, false, vim.split(case.buffer, "\n", { plain = true }))
  vim.bo.undolevels = undolevels
  -- Closed folds (manual folds start closed); soft wrap is nvim's default
  -- 'wrap', at the 80 columns the window has.
  for _, fold in ipairs(case.folds or {}) do
    vim.cmd((fold[1] + 1) .. "," .. (fold[2] + 1) .. "fold")
  end
  if case.soft_wrap then
    assert(vim.api.nvim_win_get_width(0) == 80, "the window is 80 columns wide")
  end
  if case.window_rows then
    set_rows(case.window_rows)
    vim.fn.winrestview({ topline = 1 })
  elseif vim.api.nvim_win_get_height(0) ~= default_rows then
    set_rows(default_rows)
  end
  vim.api.nvim_win_set_cursor(0, { case.cursor[1] + 1, case.cursor[2] })
  -- do_lt: `<lt>` is `<`, as fe-vim's parse_keys reads it.
  local keys = vim.api.nvim_replace_termcodes(case.oracle_keys or case.keys, true, true, true)
  local saved_maps, saved_leader = global_maps(), vim.g.mapleader
  for _, line in ipairs(case.mapping or {}) do
    vim.cmd(line)
  end
  if case.set then
    vim.cmd("set " .. case.set)
  end
  vim.api.nvim_feedkeys(keys, "xt", false)

  local cursor = vim.api.nvim_win_get_cursor(0)
  local mode = vim.api.nvim_get_mode().mode
  local visual = nil
  if mode == "v" or mode == "V" or mode == "\22" then
    local pos = vim.fn.getpos("v")
    visual = { pos[2] - 1, pos[3] - 1 }
  end
  local marks = nil
  for name in ("<>abcdefghijklmnopqrstuvwxyz"):gmatch(".") do
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
  local top = nil
  if case.window_rows then
    top = vim.fn.line("w0") - 1
  end
  restore_maps(saved_maps)
  vim.g.mapleader = saved_leader
  table.insert(results, {
    name = case.name,
    buffer = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n"),
    cursor = { cursor[1] - 1, cursor[2] },
    mode = mode,
    registers = registers,
    visual = visual,
    marks = marks,
    top = top,
  })
end

local file = assert(io.open(output, "wb"))
file:write(vim.json.encode(results))
file:close()
