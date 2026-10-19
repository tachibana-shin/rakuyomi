--- Formats a list of language codes for display. BCP-47 regional variants
--- are normalised to their base ISO 639-1 code before display (e.g.
--- `["en", "zh-Hans", "pt-BR"]` becomes `"en, pt, zh"`). Returns `nil`
--- for an empty list so callers can skip the language suffix entirely.
---
--- Only the first `max_languages` codes are listed; the remaining ones are
--- collapsed into a `+N` counter (e.g. `"de, en, fr, it, +61"`). Keeping the
--- text short is important: KOReader's MenuItem subtracts the `post_text`
--- width from the item name and passes a non-positive `max_width` to the
--- name TextWidget once the `post_text` is wider than the item, crashing
--- the paint (see koreader/koreader#13419, only fixed upstream after
--- KOReader 2025.10).
--- @param languages string[]|nil
--- @param max_languages number|nil maximum number of codes to list; defaults to 4
--- @return string|nil
local langNames = require("utils/languageNames")

local DEFAULT_MAX_LANGUAGES = 4

return function(languages, max_languages)
  if not languages or #languages == 0 then
    return nil
  end
  local normalized = {}
  local seen = {}
  for __, lang in ipairs(languages) do
    local key = langNames.normalize(lang)
    if not seen[key] then
      seen[key] = true
      normalized[#normalized + 1] = key
    end
  end
  table.sort(normalized)

  local max = max_languages or DEFAULT_MAX_LANGUAGES
  if #normalized > max then
    local shown = {}
    for i = 1, max do
      shown[i] = normalized[i]
    end
    return table.concat(shown, ", ") .. " +" .. (#normalized - max)
  end
  return table.concat(normalized, ", ")
end
