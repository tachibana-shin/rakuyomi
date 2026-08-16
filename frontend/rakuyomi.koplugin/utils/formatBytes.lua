--- Formats a byte count into a human-readable string (e.g. "1.5 KiB").
--- @param bytes number|nil
--- @return string
local function formatBytes(bytes)
  if not bytes or bytes < 1024 then
    return (bytes or 0) .. " B"
  elseif bytes < 1024 * 1024 then
    return string.format("%.1f KiB", bytes / 1024)
  elseif bytes < 1024 * 1024 * 1024 then
    return string.format("%.1f MiB", bytes / (1024 * 1024))
  else
    return string.format("%.1f GiB", bytes / (1024 * 1024 * 1024))
  end
end

return formatBytes