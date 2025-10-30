local UIManager = require("ui/uimanager")
local Trapper = require("ui/trapper")
local LoadingDialog = require("LoadingDialog")
local InfoMessage = require("ui/widget/infomessage")
local ErrorDialog = require("ErrorDialog")
local ConfirmBox = require("ui/widget/confirmbox")

local Backend = require("Backend")
local ChapterListing = require("ChapterListing")
local findNextChapter = require("chapters/findNextChapter")

local ContinueReadingHandler = {}

local MESSAGES = {
  FINDING = "Finding next chapter...",
  NO_CHAPTERS = "No chapters found for this manga.",
  NO_NEXT_CHAPTER = "Sadly, no next chapter available! :c"
}

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

local function showChapterConfirmation(chapter, onConfirm, onCancel)
  local confirm_dialog = ConfirmBox:new {
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
  local chapters_copy = {}
  for _, ch in ipairs(chapters) do table.insert(chapters_copy, ch) end

  local last_read = nil
  for i, ch in ipairs(chapters_copy) do
    if ch.read then
      last_read = ch
    else
      return ch
    end
  end

  return last_read
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
