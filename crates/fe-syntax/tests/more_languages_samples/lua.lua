local M = {}

local x = 1
local low, high = 0, 10
counter = 0

M.version = "1.0"

local Account = {
  balance = 0,
  deposit = function(self, amount)
    self.balance = self.balance + amount
  end,
}

function M.new(name)
  local self = { name = name }
  return self
end

function M:describe()
  local label = self.name
  return label
end

local function helper(value)
  local doubled = value * 2
  return doubled
end

function global_helper()
  return helper(1)
end

M.callback = function(event)
  return event
end

return M
