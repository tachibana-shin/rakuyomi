local hasValue = function(list, value)
  for __, v in ipairs(list) do if v == value then return true end end
  return false
end

return hasValue
