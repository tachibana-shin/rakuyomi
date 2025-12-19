--- @generic T: any
--- @param definitions [string, T][]
--- @param key string
--- @return T
local function findEntries(definitions, key)
  for _, tuple in ipairs(definitions) do
    if tuple[1] == key then
      return tuple[2] -- { key, definition }
    end
  end
  return nil
end

return findEntries
