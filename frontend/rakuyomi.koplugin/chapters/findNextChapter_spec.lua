---@diagnostic disable: undefined-global, undefined-field
local findNextChapter = require('chapters/findNextChapter')

--- @return Chapter
local function makeChapter(fields)
  local chapter = {
    source_id = 'test',
    manga_id = 'test',
    downloaded = false,
    read = false,
    id = 'id-' .. (fields.volume_num or 'unknown') .. '-' .. (fields.chapter_num or 'unknown'),
    scanlator = 'test'
  }

  for key, value in pairs(fields) do
    chapter[key] = value
  end

  return chapter
end

describe('findNextChapter', function()
  it('should be nil when there is a single chapter', function()
    --- @type Chapter[]
    local chapters = {
      makeChapter({ volume_num = 1, chapter_num = 1 }),
    }
    local current_chapter = chapters[1]

    local next_chapter = findNextChapter(chapters, current_chapter)

    assert.is_nil(next_chapter)
  end)

  it('should be nil when current chapter is the last one', function()
    --- @type Chapter[]
    local chapters = {
      makeChapter({ volume_num = 1, chapter_num = 3 }),
      makeChapter({ volume_num = 1, chapter_num = 2 }),
      makeChapter({ volume_num = 1, chapter_num = 1 }),
    }
    local current_chapter = chapters[1]

    local next_chapter = findNextChapter(chapters, current_chapter)

    assert.is_nil(next_chapter)
  end)

  it('should find chapter with the closest chapter number after the current one when it exists', function()
    --- @type Chapter[]
    local chapters = {
      makeChapter({ volume_num = 1, chapter_num = 1.5 }),
      makeChapter({ volume_num = 1, chapter_num = 2 }),
      makeChapter({ volume_num = 1, chapter_num = 1 }),
    }
    local current_chapter = chapters[3]

    local next_chapter = findNextChapter(chapters, current_chapter)

    assert.is_not_nil(next_chapter)
    ---@diagnostic disable-next-line: need-check-nil
    assert.equal(1.5, next_chapter.chapter_num)
  end)

  it('should prefer chapters belonging to the same scanlation group', function()
    --- @type Chapter[]
    local chapters = {
      makeChapter({ volume_num = 1, chapter_num = 2, scanlator = 'cool scans' }),
      makeChapter({ volume_num = 1, chapter_num = 2, scanlator = 'fine scans' }),
      makeChapter({ volume_num = 1, chapter_num = 1, scanlator = 'cool scans' }),
    }
    local current_chapter = chapters[3]

    local next_chapter = findNextChapter(chapters, current_chapter)

    assert.is_not_nil(next_chapter)
    ---@diagnostic disable-next-line: need-check-nil
    assert.equal('cool scans', next_chapter.scanlator)
  end)

  it('should fallback to source order when chapter has no chapter number', function()
    --- @type Chapter[]
    local chapters = {
      makeChapter({ volume_num = 1, chapter_num = 2 }),
      makeChapter({ volume_num = 1, chapter_num = 1 }),
      makeChapter({ volume_num = 1, title = 'Prologue' }),
    }
    local current_chapter = chapters[3]
    local expected_next_chapter = chapters[2]

    local next_chapter = findNextChapter(chapters, current_chapter)

    assert.equal(expected_next_chapter, next_chapter)
  end)
end)
