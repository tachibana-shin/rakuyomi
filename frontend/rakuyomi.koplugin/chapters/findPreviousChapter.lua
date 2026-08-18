--- Finds the previous chapter (earlier in story) from the given chapter.
--- This mirrors findNextChapter but in the opposite direction.
---
--- @param chapters Chapter[] The list of chapters of the manga.
--- @param current_chapter Chapter The current chapter.
--- @param is_desc boolean? Whether the chapter list is sorted newest first
--- (descending). Defaults to `true`, keeping the historical assumption that
--- the chapter list is ordered newest first.
--- @return Chapter|nil chapter The previous chapter, if found, or nil.
local function findPreviousChapter(chapters, current_chapter, is_desc)
  local best_candidate = nil

  for _, candidate in ipairs(chapters) do
    if candidate.chapter_num ~= nil and current_chapter.chapter_num ~= nil then
      if candidate.chapter_num >= current_chapter.chapter_num then
        goto continue
      end

      if best_candidate == nil then
        best_candidate = candidate
      elseif candidate.chapter_num > best_candidate.chapter_num then
        best_candidate = candidate
      elseif candidate.chapter_num == best_candidate.chapter_num
          and current_chapter.scanlator ~= nil
          and candidate.scanlator == current_chapter.scanlator then
        best_candidate = candidate
      end

      goto continue
    end

    -- Both chapter numbers are unknown: compare by publish date instead,
    -- picking the closest chapter published before the current one.
    if candidate.last_updated ~= nil and current_chapter.last_updated ~= nil
        and candidate.last_updated < current_chapter.last_updated then
      if best_candidate == nil or candidate.last_updated > best_candidate.last_updated then
        best_candidate = candidate
      end
    end

    ::continue::
  end

  if best_candidate ~= nil then
    return best_candidate
  end

  -- Fallback to the position in the chapter list. With descending order the
  -- previous chapter sits right after the current one; with ascending order
  -- it sits right before it.
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

  if index ~= nil then
    if is_desc == false then
      if index > 1 then
        return chapters[index - 1]
      end
    elseif index < #chapters then
      return chapters[index + 1]
    end
  end

  return nil
end

return findPreviousChapter
