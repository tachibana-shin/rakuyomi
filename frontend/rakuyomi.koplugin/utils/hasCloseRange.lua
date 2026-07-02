local ffi = require("ffi")

ffi.cdef [[
  int close_range(unsigned int first, unsigned int last, unsigned int flags);
]]

local has_close_range = pcall(function()
  local _ = ffi.C.close_range
end)

return has_close_range
