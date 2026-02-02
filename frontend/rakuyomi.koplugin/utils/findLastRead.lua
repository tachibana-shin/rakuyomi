local isBeforeChapter = require("utils/isBeforeChapter")

---@param chapters Chapter[]
---@return Chapter?
local function findLastRead(chapters)
  if #chapters == 0 then
    return nil
  end

  for _, chapter in ipairs(chapters) do
    if chapter.last_read ~= nil or chapter.read then
      return chapter
    end
  end

  local smallest = chapters[1]
  for i = 2, #chapters do
    if isBeforeChapter(chapters[i], smallest) then
      smallest = chapters[i]
    end
  end

  return smallest
end

return findLastRead
