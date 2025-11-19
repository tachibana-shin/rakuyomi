local UIManager = require("ui/uimanager")
local Trapper = require("ui/trapper")
local LoadingDialog = require("LoadingDialog")
local InfoMessage = require("ui/widget/infomessage")
local ErrorDialog = require("ErrorDialog")
local ConfirmBox = require("ui/widget/confirmbox")

local Backend = require("Backend")
local ChapterListing = require("ChapterListing")
-- local findNextChapter = require("chapters/findNextChapter")
local getChapterDisplayName = require("utils/getChapterDisplayName")

local ContinueReadingHandler = {}

local MESSAGES = {
  FINDING = "Finding next chapter...",
  NO_CHAPTERS = "No chapters found for this manga.",
  NO_NEXT_CHAPTER = "Sadly, no next chapter available! :c"
}

local function showChapterConfirmation(chapter, onConfirm, onCancel)
  local confirm_dialog
  confirm_dialog = ConfirmBox:new {
    text = "Resume reading with:\n" .. getChapterDisplayName(chapter) .. "?",
    ok_text = "Read",
    cancel_text = "Cancel",
    ok_callback = function()
      UIManager:close(confirm_dialog)
      if onConfirm then onConfirm() end
    end,
    cancel_callback = function()
      UIManager:close(confirm_dialog)
      if onCancel then onCancel() end
    end
  }
  UIManager:show(confirm_dialog)
end

local function findChapterToOpen(chapters)
  local last_read_chapter = nil
  local last_read_chapter_num = -math.huge
  local next_chapter = nil
  local first_chapter = nil
  local first_chapter_num = math.huge

  for _, chapter in ipairs(chapters) do
    local num = chapter.chapter_num
    if num then
      if num < first_chapter_num then
        first_chapter = chapter
        first_chapter_num = num
      end

      if chapter.read and num > last_read_chapter_num then
        last_read_chapter = chapter
        last_read_chapter_num = num
      end
    end
  end

  if not last_read_chapter then
    next_chapter = first_chapter
  else
    next_chapter = last_read_chapter
  end

  return next_chapter
end

function ContinueReadingHandler.handle(manga, original_view, custom_callbacks)
  Trapper:wrap(function()
    local callbacks = {
      onReturn = function()
        if original_view and original_view.fetchAndShow then
          original_view:fetchAndShow()
        end
      end,
      onError = function(msg) ErrorDialog:show(msg) end,
      original_view = original_view
    }
    if custom_callbacks then
      callbacks.onReturn = custom_callbacks.onReturn or callbacks.onReturn
      callbacks.onError = custom_callbacks.onError or callbacks.onError
    end

    local resp = LoadingDialog:showAndRun(MESSAGES.FINDING, function()
      return Backend.listCachedChapters(manga.source.id, manga.id)
    end)
    if resp.type == 'ERROR' then
      callbacks.onError(resp.message)
      return
    end

    local chapters = resp.body
    if #chapters == 0 then
      callbacks.onError(MESSAGES.NO_CHAPTERS)
      return
    end

    local chapter_to_open = findChapterToOpen(chapters)
    if not chapter_to_open then
      UIManager:show(InfoMessage:new { text = MESSAGES.NO_NEXT_CHAPTER })
      return
    end

    showChapterConfirmation(chapter_to_open, function()
      local temp_listing = ChapterListing:new { manga = manga }
      temp_listing.chapters = chapters
      temp_listing:openChapterOnReader(chapter_to_open)
    end, function() end)
  end)
end

return ContinueReadingHandler
