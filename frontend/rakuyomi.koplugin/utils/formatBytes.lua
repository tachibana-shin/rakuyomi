--- Formats a byte count into a human-readable string (e.g. "1.5 MB").
--- @param bytes number|nil
--- @return string
local function formatBytes(bytes)
  if not bytes or bytes <= 0 then
    return "0 B"
  end

  local units = { "B", "KB", "MB", "GB", "TB" }
  local size = bytes
  local i = 1
  while size >= 1024 and i < #units do
    size = size / 1024
    i = i + 1
  end

  if i == 1 then
    return string.format("%d %s", size, units[i])
  end

  return string.format("%.1f %s", size, units[i])
end

return formatBytes
