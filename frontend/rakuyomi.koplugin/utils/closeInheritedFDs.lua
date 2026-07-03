local ffi = require("ffi")

ffi.cdef [[
  int close_range(unsigned int first, unsigned int last, unsigned int flags);
]]

local FALLBACK_MAX_FD = 0x40

local has_close_range = pcall(function()
  local _ = ffi.C.close_range
end)

--- Closes file descriptors inherited from the parent process, to avoid leaking
--- them into a child process that's about to exec another binary.
---
--- Uses the `close_range` syscall when available, falling back to closing a
--- fixed range of file descriptors (`first` through `FALLBACK_MAX_FD`) otherwise.
---
--- @param first number The first file descriptor to close (inclusive).
local function closeInheritedFDs(first)
  if has_close_range then
    local ret = ffi.C.close_range(first, 0xffffffff, 0)
    if ret == 0 then
      return
    end
  end

  for fd = first, FALLBACK_MAX_FD do
    ffi.C.close(fd)
  end
end

return closeInheritedFDs
