--- Compares whether chapter `a` is before `b`. Expects the `index` of the chapter in the
--- chapter array to be present inside the chapter object.
---
--- @param a Chapter|{ index: number }
--- @param b Chapter|{ index: number }
--- @return boolean `true` if chapter `a` should be displayed before `b`, otherwise `false`.
local function isBeforeChapter(a, b)
  if a.volume_num ~= nil and b.volume_num ~= nil and a.volume_num ~= b.volume_num then
    return a.volume_num < b.volume_num
  end

  if a.chapter_num ~= nil and b.chapter_num ~= nil and a.chapter_num ~= b.chapter_num then
    return a.chapter_num < b.chapter_num
  end

  -- When both chapters carry a publish date, the newest chapter comes first.
  if a.last_updated ~= nil and b.last_updated ~= nil and a.last_updated ~= b.last_updated then
    return a.last_updated > b.last_updated
  end

  -- Last resort: keep the order the source returned. Source order is not
  -- reliable across sources (some return newest first, some oldest first),
  -- so we do not guess.
  return a.index < b.index
end


return isBeforeChapter
