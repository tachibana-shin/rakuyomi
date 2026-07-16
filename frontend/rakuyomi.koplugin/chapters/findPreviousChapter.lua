--- Finds the previous chapter (earlier in story) from the given chapter.
--- This mirrors findNextChapter but in the opposite direction.
--- The chapters array is ordered newest -> oldest, so "previous" (earlier story)
--- is at a higher index.
---
--- @param chapters Chapter[] The list of chapters of the manga.
--- @param current_chapter Chapter The current chapter.
--- @return Chapter|nil chapter The previous chapter, if found, or nil.
local function findPreviousChapter(chapters, current_chapter)
  local best_candidate = nil

  for _, candidate in ipairs(chapters) do
    if candidate.chapter_num == nil or current_chapter.chapter_num == nil then
      goto continue
    end

    if candidate.chapter_num >= current_chapter.chapter_num then
      goto continue
    end

    if best_candidate == nil then
      best_candidate = candidate
    end

    if candidate.chapter_num < best_candidate.chapter_num then
      goto continue
    end

    -- Prefer a candidate closer to (just below) current chapter number,
    -- and prefer same scanlation group as a tiebreaker.
    if candidate.chapter_num > best_candidate.chapter_num then
      best_candidate = candidate
    elseif current_chapter.scanlator ~= nil and candidate.scanlator == current_chapter.scanlator then
      best_candidate = candidate
    end

    ::continue::
  end

  if best_candidate ~= nil then
    return best_candidate
  end

  -- Fallback: use source order. Previous chapter is the one right after
  -- the current one in the array (since array is newest -> oldest).
  local function isSameChapter(a, b)
    return a.source_id == b.source_id and a.manga_id == b.manga_id and a.id == b.id
  end

  local index = nil
  for i, chapter in ipairs(chapters) do
    if isSameChapter(chapter, current_chapter) then
      index = i
      break
    end
  end

  if index ~= nil and index < #chapters then
    return chapters[index + 1]
  end

  return nil
end

return findPreviousChapter
