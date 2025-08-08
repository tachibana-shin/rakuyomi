-- frontend/rakuyomi.koplugin/handlers/ContinueReadingHandler.lua
local ConfirmBox = require("ui/widget/confirmbox")
local UIManager = require("ui/uimanager")
local Trapper = require("ui/trapper")
local LoadingDialog = require("LoadingDialog")
local InfoMessage = require("ui/widget/infomessage")
local util = require("util")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local MangaReader = require("MangaReader")
local DownloadChapter = require("jobs/DownloadChapter")
local findNextChapterLogic = require("chapters/findNextChapter")

local ContinueReadingHandler = {}

-- Constants
local MESSAGES = {
  FINDING = "Finding next chapter...",
  DOWNLOADING = "Downloading chapter...",
  NO_CHAPTERS = "No chapters found for this manga.",
  NO_CHAPTER = "Could not determine which chapter to open.",
  NO_NEXT_CHAPTER = "Sadly, no next chapter available! :c",
  DOWNLOAD_FAILED = "Could not create download for chapter."
}

--- Chapter logic
local ChapterManager = {}

function ChapterManager.prepareChaptersForContinueReading(chapters_from_backend)
  local prepared_chapters = util.tableDeepCopy(chapters_from_backend)
  local reversed_chapters = {}
  for i = #prepared_chapters, 1, -1 do
    table.insert(reversed_chapters, prepared_chapters[i])
  end
  return reversed_chapters
end

function ChapterManager.findChapterToOpen(chapters_in_reading_order)
  if not chapters_in_reading_order or #chapters_in_reading_order == 0 then
    return nil
  end

  local last_read_chapter, first_unread_chapter, last_read_index = nil, nil, nil
  local all_chapters_read = true

  -- Find last read and first unread chapter
  for i, chapter in ipairs(chapters_in_reading_order) do
    if chapter.read then
      last_read_chapter = chapter
      last_read_index = i
    else
      all_chapters_read = false
      if not first_unread_chapter then
        first_unread_chapter = chapter
      end
    end
  end

  local chapter_to_open = nil

  -- Find next logical chapter after last read (volume-independent)
  if last_read_chapter and last_read_index then
    local preferred_scanlator = last_read_chapter.scanlator
    local next_chapter_num = nil

    -- Iterate only from position after last read chapter
    for i = last_read_index + 1, #chapters_in_reading_order do
      local chapter = chapters_in_reading_order[i]
      if not chapter.read and
          -- Is volume important here?
          -- chapter.volume_num == last_read_chapter.volume_num and
          chapter.chapter_num and last_read_chapter.chapter_num and
          chapter.chapter_num > last_read_chapter.chapter_num then
        -- Find the lowest next chapter number (volume-independent)
        if not next_chapter_num or chapter.chapter_num < next_chapter_num then
          next_chapter_num = chapter.chapter_num
          chapter_to_open = chapter -- Take first one we find

          -- But if we find preferred scanlator for this chapter number, prefer it
          if chapter.scanlator == preferred_scanlator then
            break -- Found perfect match, stop searching
          end
        elseif chapter.chapter_num == next_chapter_num and chapter.scanlator == preferred_scanlator then
          chapter_to_open = chapter -- Upgrade to preferred scanlator
          break                     -- Found perfect match, stop searching
        end
      end
    end
  end

  -- Fallbacks
  if chapter_to_open then
    return chapter_to_open
  elseif all_chapters_read and last_read_chapter then
    return last_read_chapter            -- Re-read last chapter if everything is read
  elseif last_read_chapter then
    return nil, "no_next_available"     -- Signal that no next chapter is available
  elseif first_unread_chapter then
    return first_unread_chapter         -- Only suggest Ch 1 if no chapters read yet
  else
    return chapters_in_reading_order[1] -- Final fallback
  end
end

--- UI utilities
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

local function showChapterConfirmation(chapter, on_confirm, on_cancel)
  local confirm_dialog = ConfirmBox:new {
    text = "Resume reading with:\n" .. getChapterDisplayName(chapter) .. "?",
    ok_text = "Read",
    cancel_text = "Cancel",
    ok_callback = function()
      UIManager:close(confirm_dialog)
      if on_confirm then on_confirm() end
    end,
    cancel_callback = function()
      UIManager:close(confirm_dialog)
      if on_cancel then on_cancel() end
    end
  }
  UIManager:show(confirm_dialog)
end

--- Reader management
local function downloadChapter(chapter)
  local download_job = DownloadChapter:new(chapter.source_id, chapter.manga_id, chapter.id, chapter.chapter_num)
  if not download_job then
    return { type = 'ERROR', message = MESSAGES.DOWNLOAD_FAILED }
  end
  return LoadingDialog:showAndRun(MESSAGES.DOWNLOADING, function()
    return download_job:runUntilCompletion()
  end)
end

local function openChapterInReader(chapter, all_chapters, callbacks)
  Trapper:wrap(function()
    local download_response = downloadChapter(chapter)

    if download_response.type == 'ERROR' then
      if callbacks.onError then callbacks.onError(download_response.message) end
      return
    end

    chapter.downloaded = true

    -- Create reader callbacks
    local onEndOfBookCallback = function()
      Backend.markChapterAsRead(chapter.source_id, chapter.manga_id, chapter.id)

      if callbacks.onChapterRead then callbacks.onChapterRead(chapter) end

      local next_chapter = findNextChapterLogic(all_chapters, chapter)
      if next_chapter then
        MangaReader:closeReaderUi(function()
          openChapterInReader(next_chapter, all_chapters, callbacks)
        end)
      else
        MangaReader:closeReaderUi(callbacks.onReturn)
      end
    end

    -- Open in reader
    MangaReader:show({
      path = download_response.body,
      on_end_of_book_callback = onEndOfBookCallback,
      on_return_callback = callbacks.onReturn,
    })

    -- Close original view
    if callbacks.original_view and UIManager:getNWidgets() > 0 and
        UIManager:getTopmostWidget() == callbacks.original_view then
      UIManager:close(callbacks.original_view)
    end
  end)
end

--- Main handler function
function ContinueReadingHandler.handle(manga, original_view, custom_callbacks)
  Trapper:wrap(function()
    -- Setup callbacks
    local callbacks = {
      onReturn = function()
        if original_view and original_view.fetchAndShow then
          original_view:fetchAndShow()
        end
      end,
      onError = function(message) ErrorDialog:show(message) end,
      onChapterRead = function(chapter) end,
      original_view = original_view
    }

    if custom_callbacks then
      callbacks.onReturn = custom_callbacks.onReturn or callbacks.onReturn
      callbacks.onError = custom_callbacks.onError or callbacks.onError
      callbacks.onChapterRead = custom_callbacks.onChapterRead or callbacks.onChapterRead
    end

    -- Fetch and validate chapters
    local chapter_list_response = LoadingDialog:showAndRun(MESSAGES.FINDING, function()
      return Backend.listCachedChapters(manga.source.id, manga.id)
    end)

    if chapter_list_response.type == 'ERROR' then
      callbacks.onError(chapter_list_response.message)
      return
    end

    local chapters = chapter_list_response.body
    if #chapters == 0 then
      callbacks.onError(MESSAGES.NO_CHAPTERS)
      return
    end

    -- Find chapter to open
    local chapters_for_finding = ChapterManager.prepareChaptersForContinueReading(chapters)
    local chapter_to_open, status = ChapterManager.findChapterToOpen(chapters_for_finding)

    if status == "no_next_available" then
      UIManager:show(InfoMessage:new { text = MESSAGES.NO_NEXT_CHAPTER })
      return
    end

    if not chapter_to_open then
      UIManager:show(InfoMessage:new { text = MESSAGES.NO_CHAPTER })
      return
    end

    -- Show confirmation
    showChapterConfirmation(chapter_to_open,
      function() openChapterInReader(chapter_to_open, chapters, callbacks) end,
      function() end
    )
  end)
end

return ContinueReadingHandler
