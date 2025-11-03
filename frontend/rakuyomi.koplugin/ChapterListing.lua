local BD = require("ui/bidi")
local ButtonDialog = require("ui/widget/buttondialog")
local InfoMessage = require("ui/widget/infomessage")
local InputDialog = require("ui/widget/inputdialog")
local UIManager = require("ui/uimanager")
local Trapper = require("ui/trapper")
local Screen = require("device").screen
local logger = require("logger")
local LoadingDialog = require("LoadingDialog")
local util = require("util")
local ffiutil = require("ffi/util")
local _ = require("gettext")

local Backend = require("Backend")
local DownloadChapter = require("jobs/DownloadChapter")
local DownloadUnreadChapters = require("jobs/DownloadUnreadChapters")
local DownloadUnreadChaptersJobDialog = require("DownloadUnreadChaptersJobDialog")
local Icons = require("Icons")
local Menu = require("widgets/Menu")
local ErrorDialog = require("ErrorDialog")
local MangaReader = require("MangaReader")
local Testing = require("testing")
local calcLastReadText = require("utils/calcLastReadText")

local findNextChapter = require("chapters/findNextChapter")

--- @class ChapterListing : { [any]: any }
--- @field manga Manga
--- @field chapters Chapter[]
--- @field chapter_sorting_mode ChapterSortingMode
local ChapterListing = Menu:extend {
  name = "chapter_listing",
  is_enable_shortcut = false,
  is_popout = false,
  title = "Chapter listing",
  align_baselines = true,

  -- the manga we're listing
  manga = nil,
  -- list of chapters
  chapters = {},
  chapter_sorting_mode = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
  -- scanlator filtering
  selected_scanlator = nil,
  available_scanlators = {},
}

function ChapterListing:init()
  self.title_bar_left_icon = "appbar.menu"
  self.onLeftButtonTap = function()
    self:openMenu()
  end

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()

  -- FIXME `Menu` calls `updateItems()` during init, but we haven't fetched any items yet, as
  -- we do it in `updateChapterList`. Not sure if there's any downside to it, but here's a notice.
  local page = self.page
  Menu.init(self)
  self.page = page

  -- we need to fill this with *something* in order to Koreader actually recognize
  -- that the back button is active, so yeah
  -- we also need to set this _after_ the `Menu.init` call, because it changes
  -- this value to {}
  self.paths = {
    { callback = self.on_return_callback },
  }
  -- idk might make some gc shenanigans actually work
  self.on_return_callback = nil

  -- we need to do this after updating
  self:updateChapterList()
end

function ChapterListing:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

--- Fetches the cached chapter list from the backend and updates the menu items.
function ChapterListing:updateChapterList()
  local response = Backend.listCachedChapters(self.manga.source.id, self.manga.id)

  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local chapter_results = response.body
  self.chapters = chapter_results

  self:extractAvailableScanlators()

  self:loadSavedScanlatorPreference()

  self:updateItems()
end

-- Load saved scanlator preference from backend
function ChapterListing:loadSavedScanlatorPreference()
  local response = Backend.getPreferredScanlator(self.manga.source.id, self.manga.id)

  self.selected_scanlator = nil

  if response.type == 'SUCCESS' and response.body then
    for _, available_scanlator in ipairs(self.available_scanlators) do
      if available_scanlator == response.body then
        self.selected_scanlator = response.body
        break
      end
    end
  end
end

-- Extract unique scanlators
function ChapterListing:extractAvailableScanlators()
  local scanlators = {}
  local scanlator_set = {}

  for _, chapter in ipairs(self.chapters) do
    local scanlator = chapter.scanlator or "Unknown"
    if not scanlator_set[scanlator] then
      scanlator_set[scanlator] = true
      table.insert(scanlators, scanlator)
    end
  end

  table.sort(scanlators)

  self.available_scanlators = scanlators
end

--- Updates the menu item contents with the chapter information
--- @private
function ChapterListing:updateItems()
  if #self.chapters > 0 then
    self.item_table = self:generateItemTableFromChapters(self.chapters)
    self.multilines_show_more_text = false
    self.items_per_page = nil
  else
    self.item_table = self:generateEmptyViewItemTable()
    self.multilines_show_more_text = true
    self.items_per_page = 1
  end

  Menu.updateItems(self)
end

--- @private
function ChapterListing:generateEmptyViewItemTable()
  return {
    {
      text = "No chapters found. Try swiping down to refresh the chapter list.",
      dim = true,
      select_enabled = false,
    }
  }
end

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

--- @private
function ChapterListing:generateItemTableFromChapters(chapters)
  -- Filter chapters by selected scanlator
  local filtered_chapters = chapters
  if self.selected_scanlator then
    filtered_chapters = {}
    for _, chapter in ipairs(chapters) do
      local chapter_scanlator = chapter.scanlator or "Unknown"
      if chapter_scanlator == self.selected_scanlator then
        table.insert(filtered_chapters, chapter)
      end
    end
  end

  --- @type table
  --- @diagnostic disable-next-line: assign-type-mismatch
  local sorted_chapters_with_index = util.tableDeepCopy(filtered_chapters)
  for index, chapter in ipairs(sorted_chapters_with_index) do
    chapter.index = index
  end

  if self.chapter_sorting_mode == 'chapter_ascending' then
    table.sort(sorted_chapters_with_index, isBeforeChapter)
  else
    table.sort(sorted_chapters_with_index, function(a, b) return not isBeforeChapter(a, b) end)
  end

  local item_table = {}

  for _, chapter in ipairs(sorted_chapters_with_index) do
    local text = ""
    if chapter.volume_num ~= nil then
      -- FIXME we assume there's a chapter number if there's a volume number
      -- might not be true but who knows
      text = text .. "Volume " .. chapter.volume_num .. ", "
    end

    if chapter.chapter_num ~= nil then
      text = text .. "Chapter " .. chapter.chapter_num .. " - "
    end

    text = text .. chapter.title

    -- Only show scanlator if not filtering by scanlator
    if chapter.scanlator ~= nil and not self.selected_scanlator then
      text = text .. " (" .. chapter.scanlator .. ")"
    end

    -- The text that shows to the right of the menu item
    local mandatory = ""
    if chapter.read then
      mandatory = mandatory .. Icons.FA_BOOK
    end

    if chapter.downloaded then
      mandatory = (chapter.last_read and calcLastReadText(chapter.last_read) .. " " or "") ..
          mandatory .. Icons.FA_DOWNLOAD
    end

    table.insert(item_table, {
      chapter = chapter,
      text = text,
      mandatory = mandatory,
    })
  end

  return item_table
end

--- @private
function ChapterListing:onReturn()
  local path = table.remove(self.paths)

  self:onClose()
  path.callback()
end

--- Shows the chapter list for a given manga. Must be called from a function wrapped with `Trapper:wrap()`.
---
--- @param manga Manga
--- @param onReturnCallback fun(): nil
--- @param accept_cached_results? boolean If set, failing to refresh the list of chapters from the source
--- will not show an error. Defaults to false.
function ChapterListing:fetchAndShow(manga, onReturnCallback, accept_cached_results)
  accept_cached_results = accept_cached_results or false

  local cancelled = false
  local refresh_chapters_response = LoadingDialog:showAndRun(
    "Refreshing chapters...",
    function()
      return Backend.refreshChapters(manga.source.id, manga.id)
    end,
    function()
      -- No cancellation support for now
      local cancelledMessage = InfoMessage:new {
        text = "Cancelled.",
      }
      UIManager:show(cancelledMessage)

      cancelled = true
    end
  )

  if cancelled then
    return
  end

  if refresh_chapters_response.type == 'ERROR' then
    ErrorDialog:show('Refresh chapter error\n\n' .. refresh_chapters_response.message)

    if not accept_cached_results then
      return
    end
  end

  local response = Backend.getSettings()

  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local settings = response.body

  local ui = ChapterListing:new {
    manga = manga,
    chapter_sorting_mode = settings.chapter_sorting_mode,
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
    page = self.page
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)

  Testing:emitEvent("chapter_listing_shown")
end

--- @private
function ChapterListing:onPrimaryMenuChoice(item)
  local chapter = item.chapter

  self:openChapterOnReader(chapter)
end

--- @private
function ChapterListing:onSwipe(arg, ges_ev)
  local direction = BD.flipDirectionIfMirroredUILayout(ges_ev.direction)
  if direction == "south" then
    self:refreshChapters()

    return
  end

  Menu.onSwipe(self, arg, ges_ev)
end

--- @private
function ChapterListing:refreshChapters()
  Trapper:wrap(function()
    local refresh_chapters_response = LoadingDialog:showAndRun(
      "Refreshing chapters...",
      function()
        return Backend.refreshChapters(self.manga.source.id, self.manga.id)
      end,
      function()
        local cancelledMessage = InfoMessage:new {
          text = "Cancelled.",
        }
        UIManager:show(cancelledMessage)
      end
    )

    if refresh_chapters_response.type == 'ERROR' then
      ErrorDialog:show(refresh_chapters_response.message)

      return
    end

    self:updateChapterList()
  end)
end

--- @private
--- @param chapter Chapter
--- @param download_job DownloadChapter|nil
function ChapterListing:openChapterOnReader(chapter, download_job)
  Trapper:wrap(function()
    -- If the download job we have is already invalid (internet problems, for example),
    -- spawn a new job before proceeding.
    if download_job == nil or download_job:poll().type == 'ERROR' then
      download_job = DownloadChapter:new(chapter.source_id, chapter.manga_id, chapter.id, chapter.chapter_num)
    end

    if download_job == nil then
      ErrorDialog:show('Could not download chapter.')

      return
    end

    local time = require("ui/time")
    local start_time = time.now()
    local response = LoadingDialog:showAndRun(
      "Downloading chapter..."
      .. '\nCh.' .. (chapter.chapter_num or 'unknown')
      .. ' '
      .. (chapter.title or ''),
      function()
        return download_job:runUntilCompletion()
      end,
      function()
        download_job:requestCancellation()

        local cancelledMessage = InfoMessage:new {
          text = "Download cancelled.",
        }
        UIManager:show(cancelledMessage)
      end
    )

    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)

      return
    end

    -- FIXME Mutating here _still_ sucks, we gotta think of a better way.
    chapter.downloaded = true

    local manga_path = ffiutil.realpath(response.body)

    logger.info("Waited ", time.to_ms(time.since(start_time)), "ms for download job to finish.")

    local nextChapter = findNextChapter(self.chapters, chapter)
    local nextChapterDownloadJob = nil

    if nextChapter ~= nil then
      nextChapterDownloadJob = DownloadChapter:new(
        nextChapter.source_id,
        nextChapter.manga_id,
        nextChapter.id,
        nextChapter.chapter_num
      )
    end

    local onReturnCallback = function()
      self:updateItems()

      UIManager:show(self)
    end

    local onEndOfBookCallback = function()
      Backend.markChapterAsRead(chapter.source_id, chapter.manga_id, chapter.id)

      self:updateChapterList()

      if nextChapter ~= nil then
        logger.info("opening next chapter", nextChapter)
        self:openChapterOnReader(nextChapter, nextChapterDownloadJob)
      else
        MangaReader:closeReaderUi(function()
          UIManager:show(self)
        end)
      end
    end

    Backend.updateLastReadChapter(
      chapter.source_id,
      chapter.manga_id,
      chapter.id
    )

    MangaReader:show({
      path = manga_path,
      on_end_of_book_callback = onEndOfBookCallback,
      chapter = chapter,
      on_close_book_callback = function(chapter)
        Backend.updateLastReadChapter(
          chapter.source_id,
          chapter.manga_id,
          chapter.id
        )
      end,
      on_return_callback = onReturnCallback,
    })

    self:onClose()
  end)
end

--- @private
function ChapterListing:openMenu()
  local dialog

  local buttons = {
    {
      {
        text = Icons.REFRESHING .. " Refresh",
        callback = function()
          UIManager:close(dialog)

          self:refreshChapters()
        end
      },
    },
    {
      {
        text = Icons.FA_BELL .. _(" Add to Library"),
        callback = function()
          UIManager:close(dialog)

          self:addToLibrary()
        end
      },
    },
    {
      {
        text = Icons.RESTORE .. " Resume",
        callback = function()
          UIManager:close(dialog)

          self:readContinue(false)
        end
      },
      {
        text = Icons.ANGLES_RIGHT .. " Next Chapter",
        callback = function()
          UIManager:close(dialog)

          self:readContinue(true)
        end
      }
    },
    {
      {
        text = Icons.FA_DOWNLOAD .. " Download unread chapters…",
        callback = function()
          UIManager:close(dialog)

          self:onDownloadUnreadChapters()
        end
      }
    }
  }

  -- Add scanlator filter button if multiple scanlators exist
  if #self.available_scanlators > 1 then
    local scanlator_text = self.selected_scanlator and
        (Icons.FA_FILTER .. " Group: " .. self.selected_scanlator) or
        Icons.FA_FILTER .. " Filter by Group"

    table.insert(buttons, {
      {
        text = scanlator_text,
        callback = function()
          UIManager:close(dialog)
          self:showScanlatorDialog()
        end
      }
    })
  end

  dialog = ButtonDialog:new {
    buttons = buttons,
  }

  UIManager:show(dialog)
end

function ChapterListing:addToLibrary()
  Trapper:wrap(function()
    local response = LoadingDialog:showAndRun(
      _("Adding to library..."),
      function()
        return Backend.addMangaToLibrary(self.manga.source.id, self.manga.id)
      end
    )

    if response.type == 'ERROR' then
      ErrorDialog:show(_("Failed to add to library: ") .. response.message)
      return
    end

    UIManager:show(InfoMessage:new {
      text = _("Added to library."),
    })
  end)
end

function ChapterListing:readContinue(nextChapter)
  local last_read_chapter = nil
  local last_read_chapter_num = -math.huge
  local next_chapter = nil
  local next_chapter_num = math.huge
  local first_chapter = nil
  local first_chapter_num = math.huge

  for _, chapter in ipairs(self.chapters) do
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
    if nextChapter then
      for _, chapter in ipairs(self.chapters) do
        local num = chapter.chapter_num
        if num and not chapter.read and num > last_read_chapter_num and num < next_chapter_num then
          next_chapter = chapter
          next_chapter_num = num
        end
      end
    else
      next_chapter = last_read_chapter
    end
  end

  next_chapter = next_chapter or first_chapter

  if next_chapter == nil then
    UIManager:show(InfoMessage:new {
      text = _("No more chapters to read!"),
      timeout = 2,
    })
    return
  end

  local ConfirmBox = require("ui/widget/confirmbox")
  local getChapterDisplayName = require("utils/getChapterDisplayName")

  local confirm_dialog
  confirm_dialog = ConfirmBox:new {
    text = _(nextChapter and "Next" or "Resume") .. " " .. _("reading with") .. ":\n" .. getChapterDisplayName(next_chapter) .. "?",
    ok_text = _("Read"),
    cancel_text = _("Cancel"),
    ok_callback = function()
      UIManager:close(confirm_dialog)

      self:openChapterOnReader(next_chapter)
    end,
    cancel_callback = function()
      UIManager:close(confirm_dialog)
    end
  }

  UIManager:show(confirm_dialog)
end

-- Scanlator selection dialog with persistence
function ChapterListing:showScanlatorDialog()
  local dialog
  local buttons = {}

  -- Show All option
  table.insert(buttons, {
    {
      text = self.selected_scanlator == nil and Icons.FA_CHECK .. " Show All" or "Show All",
      callback = function()
        UIManager:close(dialog)
        self.selected_scanlator = nil

        Backend.setPreferredScanlator(self.manga.source.id, self.manga.id, nil)

        self:updateItems()
        UIManager:show(InfoMessage:new { text = "Showing all groups", timeout = 1 })
      end
    }
  })

  -- Individual scanlators
  for _, scanlator in ipairs(self.available_scanlators) do
    local is_selected = self.selected_scanlator == scanlator
    local text = is_selected and (Icons.FA_CHECK .. " " .. scanlator) or scanlator

    table.insert(buttons, {
      {
        text = text,
        callback = function()
          UIManager:close(dialog)
          self.selected_scanlator = scanlator

          Backend.setPreferredScanlator(self.manga.source.id, self.manga.id, scanlator)

          self:updateItems()
          UIManager:show(InfoMessage:new { text = "Filtered to: " .. scanlator, timeout = 1 })
        end
      }
    })
  end

  dialog = ButtonDialog:new {
    title = "Filter by Group",
    buttons = buttons,
  }

  UIManager:show(dialog)
end

function ChapterListing:onDownloadUnreadChapters()
  local input_dialog
  input_dialog = InputDialog:new {
    title = "Download unread chapters...",
    input_type = "number",
    input_hint = "Amount of unread chapters (default: all)",
    description = self.selected_scanlator and
        ("Will download from: " .. self.selected_scanlator .. "\n\nSpecify amount or leave empty for all.") or
        "Specify the amount of unread chapters to download, or leave empty to download all of them.",
    buttons = {
      {
        {
          text = "Cancel",
          id = "close",
          callback = function()
            UIManager:close(input_dialog)
          end,
        },
        {
          text = "Download",
          is_enter_default = true,
          callback = function()
            UIManager:close(input_dialog)

            local amount = nil
            if input_dialog:getInputText() ~= '' then
              amount = tonumber(input_dialog:getInputText())

              if amount == nil then
                ErrorDialog:show('Invalid amount of chapters!')

                return
              end
            end

            -- Use scanlator-aware download
            local job = self:createDownloadJob(amount)
            if job then
              local dialog = DownloadUnreadChaptersJobDialog:new({
                show_parent = self,
                job = job,
                dismiss_callback = function()
                  self:updateChapterList()
                end
              })

              dialog:show()
            else
              UIManager:show(InfoMessage:new {
                text = "No unread chapters found for " .. (self.selected_scanlator or "this manga"),
                timeout = 2,
              })
            end
          end,
        },
      }
    }
  }

  UIManager:show(input_dialog)
end

function ChapterListing:createDownloadJob(amount)
  return DownloadUnreadChapters:new({
    source_id = self.manga.source.id,
    manga_id = self.manga.id,
    amount = amount,
    scanlator = self.selected_scanlator
  })
end

function ChapterListing:onDownloadAllChapters()
  local downloadingMessage = InfoMessage:new {
    text = "Downloading all chapters, this will take a while…",
  }

  UIManager:show(downloadingMessage)

  -- FIXME when the backend functions become actually async we can get rid of this probably
  UIManager:nextTick(function()
    local time = require("ui/time")
    local startTime = time.now()
    local response = Backend.downloadAllChapters(self.manga.source.id, self.manga.id)

    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)

      return
    end

    local onDownloadFinished = function()
      -- FIXME I don't think mutating the chapter list here is the way to go, but it's quicker
      -- than making another call to list the chapters from the backend...
      -- this also behaves wrong when the download fails but manages to download some chapters.
      -- some possible alternatives:
      -- - return the chapter list from the backend on the `downloadAllChapters` call
      -- - biting the bullet and making the API call
      for _, chapter in ipairs(self.chapters) do
        chapter.downloaded = true
      end

      logger.info("Downloaded all chapters in ", time.to_ms(time.since(startTime)), "ms")

      self:updateItems()
    end

    local updateProgress = function() end

    local cancellationRequested = false
    local onCancellationRequested = function()
      local response = Backend.cancelDownloadAllChapters(self.manga.source.id, self.manga.id)
      -- FIXME is it ok to assume there are no errors here?
      assert(response.type == 'SUCCESS')

      cancellationRequested = true

      updateProgress()
    end

    local onCancelled = function()
      local cancelledMessage = InfoMessage:new {
        text = "Cancelled.",
      }

      UIManager:show(cancelledMessage)
    end

    updateProgress = function()
      -- Remove any scheduled `updateProgress` calls, because we do not want this to be
      -- called again if not scheduled by ourselves. This may happen when `updateProgress` is called
      -- from another place that's not from the scheduler (eg. the `onCancellationRequested` handler),
      -- which could result in an additional `updateProgress` call that was already scheduled previously,
      -- even if we do not schedule it at the end of the method.
      UIManager:unschedule(updateProgress)
      UIManager:close(downloadingMessage)

      local response = Backend.getDownloadAllChaptersProgress(self.manga.source.id, self.manga.id)
      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end

      local downloadProgress = response.body

      local messageText = nil
      local isCancellable = false
      if downloadProgress.type == "INITIALIZING" then
        messageText = "Downloading all chapters, this will take a while…"
      elseif downloadProgress.type == "FINISHED" then
        onDownloadFinished()

        return
      elseif downloadProgress.type == "CANCELLED" then
        onCancelled()

        return
      elseif cancellationRequested then
        messageText = "Waiting for download to be cancelled…"
      elseif downloadProgress.type == "PROGRESSING" then
        messageText = "Downloading all chapters, this will take a while… (" ..
            downloadProgress.downloaded .. "/" .. downloadProgress.total .. ")." ..
            "\n\n" ..
            "Tap outside this message to cancel."

        isCancellable = true
      else
        logger.err("unexpected download progress message", downloadProgress)

        error("unexpected download progress message")
      end

      downloadingMessage = InfoMessage:new {
        text = messageText,
        dismissable = isCancellable,
      }

      -- Override the default `onTapClose`/`onAnyKeyPressed` actions
      if isCancellable then
        local originalOnTapClose = downloadingMessage.onTapClose
        downloadingMessage.onTapClose = function(messageSelf)
          onCancellationRequested()

          originalOnTapClose(messageSelf)
        end

        local originalOnAnyKeyPressed = downloadingMessage.onAnyKeyPressed
        downloadingMessage.onAnyKeyPressed = function(messageSelf)
          onCancellationRequested()

          originalOnAnyKeyPressed(messageSelf)
        end
      end
      UIManager:show(downloadingMessage)

      UIManager:scheduleIn(1, updateProgress)
    end

    UIManager:scheduleIn(1, updateProgress)
  end)
end

return ChapterListing
