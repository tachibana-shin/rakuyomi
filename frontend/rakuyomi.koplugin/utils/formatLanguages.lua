--- Formats a list of language codes for display, e.g. `["en", "vi"]` becomes
--- `"en, vi"`. Returns `nil` for an empty list so callers can skip the
--- language suffix entirely.
--- @param languages string[]|nil
--- @return string|nil
return function(languages)
  if not languages or #languages == 0 then
    return nil
  end
  return table.concat(languages, ", ")
end