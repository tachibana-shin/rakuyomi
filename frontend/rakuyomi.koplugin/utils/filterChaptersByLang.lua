-- Filter chapter list by selected languages
---@param raw_chapters Chapter[]
---@param langs_selected string[]
---@return Chapter[]
local function filterChaptersByLang(raw_chapters, langs_selected)
  -- If 0 languages selected, no need to filter
  if not langs_selected or #langs_selected < 1 then
    return raw_chapters
  end

  -- Build fast lookup table for langs
  -- { en = true, jp = true, ... }
  local lang_map = {}
  for _, lang in ipairs(langs_selected) do
    lang_map[lang] = true
  end

  -- Filter chapters
  local result = {}
  for _, chapter in ipairs(raw_chapters) do
    local lang = chapter.lang or "unknown"
    -- chapter.lang may be nil → safe check
    if lang_map[lang] then
      table.insert(result, chapter)
    end
  end

  return result
end


return filterChaptersByLang
