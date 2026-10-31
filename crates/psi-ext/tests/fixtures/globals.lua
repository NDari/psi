counter = 0
function helper() return counter end
local t = { 1, 2 }
local function f(a, ...)
  local up = a + select("#", ...)
  return function() return up + #t end
end
local x = undefined_thing
local a, b = unpack(t)
local n = table.getn(t)
local s = "a" + "b"
pirnt(x, a, b, n, s, f, helper(), "1" + 1)
return { name = "globals", version = "1", description = "d", setup = function() end }
