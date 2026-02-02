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

  -- This is _very_ flaky, but we assume that source order is _always_ from newer chapters -> older chapters.
  -- Unfortunately we need to make some kind of assumptions here to handle edgecases (e.g. chapters without a chapter number)
  return a.index > b.index
end


return isBeforeChapter
