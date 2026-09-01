--- Formats a list of language codes for display. BCP-47 regional variants
--- are normalised to their base ISO 639-1 code before display (e.g.
--- `["en", "zh-Hans", "pt-BR"]` becomes `"en, pt, zh"`). Returns `nil`
--- for an empty list so callers can skip the language suffix entirely.
--- @param languages string[]|nil
--- @return string|nil
local langNames = require("utils/languageNames")
return function(languages)
  if not languages or #languages == 0 then
    return nil
  end
  local normalized = {}
  local seen = {}
  for _, lang in ipairs(languages) do
    local key = langNames.normalize(lang)
    if not seen[key] then
      seen[key] = true
      normalized[#normalized + 1] = key
    end
  end
  table.sort(normalized)
  return table.concat(normalized, ", ")
end
