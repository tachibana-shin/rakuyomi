--- Finds the index of the given chapter on the chapter listing.
---
--- @param haystack Chapter[] The chapter listing.
--- @param needle Chapter The chapter being looked for.
--- @return number|nil The index of the chapter on the listing, or nil, if it could not be found.
--- @private
local function findChapterIndex(haystack, needle)
  local function isSameChapter(a, b)
    return a.source_id == b.source_id and a.manga_id == b.manga_id and a.id == b.id
  end

  for i, chapter in ipairs(haystack) do
    if isSameChapter(chapter, needle) then
      return i
    end
  end

  return nil
end

--- Attempts to find the next chapter from the given chapter, comparing by chapter number.
--- If multiple candidates are found, we'll attempt to pick a chapter belonging to
--- the same scanlation group.
--- If no candidate is found, the publish date is used, then the position in the
--- chapter list.
---
--- @param chapters Chapter[] The list of chapters of the manga.
--- @param current_chapter Chapter The current chapter.
--- @param is_desc boolean? Whether the chapter list is sorted newest first
--- (descending). Defaults to `true`, keeping the historical assumption that
--- the chapter list is ordered newest first.
--- @return Chapter|nil chapter The next chapter, if found, or nil.
local function findNextChapter(chapters, current_chapter, is_desc)
  local best_candidate = nil

  for _, candidate in ipairs(chapters) do
    if candidate.chapter_num ~= nil and current_chapter.chapter_num ~= nil then
      if candidate.chapter_num <= current_chapter.chapter_num then
        goto continue
      end

      if best_candidate == nil then
        best_candidate = candidate
      elseif candidate.chapter_num < best_candidate.chapter_num then
        best_candidate = candidate
      elseif candidate.chapter_num == best_candidate.chapter_num
          and current_chapter.scanlator ~= nil
          and candidate.scanlator == current_chapter.scanlator then
        best_candidate = candidate
      end

      goto continue
    end

    -- Both chapter numbers are unknown: compare by publish date instead,
    -- picking the closest chapter published after the current one.
    if candidate.last_updated ~= nil and current_chapter.last_updated ~= nil
        and candidate.last_updated > current_chapter.last_updated then
      if best_candidate == nil or candidate.last_updated < best_candidate.last_updated then
        best_candidate = candidate
      end
    end

    ::continue::
  end

  if best_candidate ~= nil then
    return best_candidate
  end

  -- Fallback to the position in the chapter list. With descending order the
  -- next chapter sits right before the current one; with ascending order it
  -- sits right after it.
  local index = findChapterIndex(chapters, current_chapter)
  assert(index ~= nil)

  if is_desc == false then
    if index < #chapters then
      return chapters[index + 1]
    end
  elseif index > 1 then
    return chapters[index - 1]
  end

  -- Everything failed. We have no next chapter 🤷.
  return nil
end

return findNextChapter
