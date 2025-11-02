local function getChapterDisplayName(chapter)
  local name = ""
  if chapter.volume_num then name = name .. "Vol. " .. chapter.volume_num .. " " end
  if chapter.chapter_num then name = name .. "Ch. " .. chapter.chapter_num .. " " end
  if chapter.title and chapter.title ~= "" then
    name = name .. "\"" .. chapter.title .. "\""
  elseif name == "" then
    name = "Chapter " .. (chapter.id or "?")
  end
  return name
end


return getChapterDisplayName
