#!/usr/bin/env lua
-- A small rate limiter, used as a golden highlighting sample.

local ffi = require("ffi")

ffi.cdef [[
  int usleep(unsigned int usec);
]]

local MAX_TOKENS <const> = 10
local REFILL_PER_SECOND = 2.5

--[[
  A bucket holds up to MAX_TOKENS tokens and refills over time.
  Callers take one token per request.
]]
local Bucket = {}
Bucket.__index = Bucket

function Bucket.new(capacity, now)
  local self = setmetatable({}, Bucket)
  self.capacity = capacity or MAX_TOKENS
  self.tokens = self.capacity
  self.updated = now or os.time()
  return self
end

function Bucket:refill(now)
  local elapsed = now - self.updated
  if elapsed <= 0 then
    return
  end
  self.tokens = math.min(self.capacity, self.tokens + elapsed * REFILL_PER_SECOND)
  self.updated = now
end

function Bucket:take(now, count)
  count = count or 1
  self:refill(now)
  if self.tokens >= count then
    self.tokens = self.tokens - count
    return true
  end
  return false, string.format("need %d, have %.1f", count, self.tokens)
end

local function summarize(buckets)
  local lines, total = {}, 0
  for name, bucket in pairs(buckets) do
    total = total + bucket.tokens
    lines[#lines + 1] = name .. "=" .. tostring(bucket.tokens)
  end
  table.sort(lines)
  return table.concat(lines, ", "), total
end

local function retry(fn, attempts, ...)
  local i = 0
  repeat
    i = i + 1
    local ok, err = pcall(fn, ...)
    if ok then return err end
    if i % 2 == 0 and not err then goto continue end
    ::continue::
  until i >= attempts
  return nil
end

local buckets = {
  api = Bucket.new(5, 0),
  ["static"] = Bucket.new(nil, 0),
}

for t = 0, 3 do
  local ok, reason = buckets.api:take(t, 2)
  print(t, ok, reason ~= nil and reason or "-")
end

print(summarize(buckets))
print(retry(function(x) return x * 0x10 end, 3, 4))
