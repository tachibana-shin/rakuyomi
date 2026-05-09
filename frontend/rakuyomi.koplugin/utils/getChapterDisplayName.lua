local _ = require("gettext+")

---@param chapter Chapter
local function getChapterDisplayName(chapter)
  local parts = {}
  if chapter.volume_num then
    table.insert(parts, _("Vol.") .. " " .. chapter.volume_num .. " ")
  end
  if chapter.chapter_num then
    table.insert(parts, _("Ch.") .. " " .. chapter.chapter_num .. " ")
  end
  if chapter.title and chapter.title ~= "" then
    table.insert(parts, "\"" .. chapter.title .. "\"")
  elseif #parts == 0 then
    return _("Chapter") .. " " .. (chapter.id or "?")
  end
  return table.concat(parts)
end


return getChapterDisplayName
