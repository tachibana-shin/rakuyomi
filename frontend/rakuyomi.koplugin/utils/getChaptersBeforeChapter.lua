local isBeforeChapter = require("utils/isBeforeChapter")

--- Returns all chapters from `chapters` that come before `chapter` in reading order,
--- using the same ordering rules as the chapter list display.
--- @param chapters Chapter[]
--- @param chapter Chapter
--- @return Chapter[]
local function getChaptersBeforeChapter(chapters, chapter)
  local indexed = {}
  local target
  for i, ch in ipairs(chapters) do
    local entry = { index = i, volume_num = ch.volume_num, chapter_num = ch.chapter_num, ch = ch }
    indexed[i] = entry
    if ch.id == chapter.id then
      target = entry
    end
  end

  local result = {}
  if not target then
    return result
  end

  for _, entry in ipairs(indexed) do
    if entry.ch.id ~= chapter.id and isBeforeChapter(entry, target) then
      table.insert(result, entry.ch)
    end
  end

  return result
end

return getChaptersBeforeChapter
