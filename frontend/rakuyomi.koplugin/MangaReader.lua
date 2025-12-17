local ReaderUI = require("apps/reader/readerui")
local UIManager = require("ui/uimanager")
local WidgetContainer = require("ui/widget/container/widgetcontainer")
local ConfirmBox = require("ui/widget/confirmbox")
local logger = require("logger")
local _ = require("gettext")

local Testing = require('testing')

--- @class MangaReader
--- This is a singleton that contains a simpler interface with ReaderUI.
local MangaReader = {
  on_return_callback = nil,
  on_end_of_book_callback = nil,
  chapter = nil,
  on_close_book_callback = nil,
  is_showing = false,
}

--- @class MangaReaderOptions
--- @field path string Path to the file to be displayed.
--- @field on_return_callback fun(): nil Function to be called when the user selects "Go back to Rakuyomi".
--- @field on_end_of_book_callback fun(): nil Function to be called when the user reaches the end of the file.
--- @field chapter? Chapter The chapter being read.
--- @field on_close_book_callback? fun(Chapter): nil Function to be called when the user closes the manga reader.

--- Displays the file located in `path` in the KOReader's reader.
--- If a file is already being displayed, it will be replaced.
---
--- @param options MangaReaderOptions
function MangaReader:show(options)
  self.on_return_callback = options.on_return_callback
  self.on_end_of_book_callback = options.on_end_of_book_callback
  self.chapter = options.chapter
  self.on_close_book_callback = options.on_close_book_callback

  if self.is_showing then
    -- if we're showing, just switch the document
    ReaderUI.instance:switchDocument(options.path)
  else
    -- took this from opds reader
    local Event = require("ui/event")
    UIManager:broadcastEvent(Event:new("SetupShowReader"))

    ReaderUI:showReader(options.path)
  end

  self.is_showing = true
  Testing:emitEvent('manga_reader_shown')
end

--- @param ui unknown The `ReaderUI` instance we're being called from.
function MangaReader:initializeFromReaderUI(ui)
  if self.is_showing then
    ui.menu:registerToMainMenu(MangaReader)
    self:overrideBtnFileManager(ui.menu)

    ui:registerPostInitCallback(function()
      self:hookWithPriorityOntoReaderUiEvents(ui)
    end)
  end
end

--- @private
--- @param ui unknown The currently active `ReaderUI` instance.
function MangaReader:hookWithPriorityOntoReaderUiEvents(ui)
  -- We need to reorder the `ReaderUI` children such that we are the first children,
  -- in order to receive events before all other widgets
  assert(ui.name == "ReaderUI", "expected to be inside ReaderUI")

  local eventListener = WidgetContainer:new({})
  eventListener.onEndOfBook = function()
    -- FIXME this makes `self:onEndOfBook()` get called twice if it does not
    -- return true in the first invocation...
    return self:onEndOfBook()
  end
  eventListener.onCloseWidget = function()
    self:onReaderUiCloseWidget()
  end

  table.insert(ui, 2, eventListener)
end

--- Used to add the "Go back to Rakuyomi" menu item. Is called from `ReaderUI`, via the
--- `registerToMainMenu` call done in `initializeFromReaderUI`.
--- @private
function MangaReader:addToMainMenu(menu_items)
  menu_items.go_back_to_rakuyomi = {
    text = "Go back to Rakuyomi...",
    sorting_hint = "main",
    callback = function()
      self:onReturn()
    end
  }
end

--- @private
function MangaReader:onReturn()
  self:closeReaderUi(function()
    self.on_return_callback()
  end)
end

function MangaReader:closeReaderUi(done_callback)
  -- Let all event handlers run before closing the ReaderUI, because
  -- some stuff might break if we just remove it ASAP
  UIManager:nextTick(function()
    local FileManager = require("apps/filemanager/filemanager")

    -- we **have** to reopen the `FileManager`, because
    -- apparently this is the only way to get out of the `ReaderUI` without shit
    -- completely breaking (koreader really does not like when there's no `ReaderUI`
    -- nor `FileManager`)
    ReaderUI.instance:onClose()
    if FileManager.instance then
      FileManager.instance:reinit()
    else
      FileManager:showFiles()
    end

    (done_callback or function() end)()
  end)
end

--- To be called when the last page of the manga is read.
function MangaReader:onEndOfBook()
  if self.is_showing then
    logger.info("Got end of book")

    self.on_end_of_book_callback()
    return true
  end
end

--- @private
function MangaReader:onReaderUiCloseWidget()
  if self.on_close_book_callback ~= nil then
    self.on_close_book_callback(self.chapter)
  end

  self.is_showing = false
end

--- @private
function MangaReader:overrideBtnFileManager(menu)
  local old_callback = menu.menu_items.filemanager.callback

  if self.is_showing then
    menu.menu_items.filemanager.callback = function()
      local key = "allow_commaneer_filemanager"
      if G_reader_settings:nilOrFalse(key) then
        local confirm_dialog
        confirm_dialog = ConfirmBox:new {
          text = "どーも\nDo you want Rakuyomi to commandeer this button when you open it?\n\nThis setting only affects when you open it with Rakuyomi.",
          dismissable = false,
          ok_text = _("Yes"),
          cancel_text = _("No"),
          ok_callback = function()
            UIManager:close(confirm_dialog)

            G_reader_settings:saveSetting(key, true)
            self:onReturn()
          end,
          cancel_callback = function()
            UIManager:close(confirm_dialog)

            old_callback()
          end
        }

        UIManager:show(confirm_dialog)
      else
        self:onReturn()
      end
    end
  end
end

return MangaReader
