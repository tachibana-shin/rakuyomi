local DocumentRegistry = require("document/documentregistry")
local InputContainer = require("ui/widget/container/inputcontainer")
local Dispatcher = require("dispatcher")
local logger = require("logger")
local _ = require("gettext+")
local OfflineAlertDialog = require("OfflineAlertDialog")

local Backend = require("Backend")
local CbzDocument = require("extensions/CbzDocument")
local ErrorDialog = require("ErrorDialog")
local LibraryView = require("LibraryView")
local MangaReader = require("MangaReader")
local Testing = require("testing")

require("RakuyomiShared")

local disable_logging = G_reader_settings:isTrue("rakuyomi_disable_logging")
if disable_logging then logger:setLevel(logger.levels.err) end

local ok, android = pcall(require, "android")
local Rakuyomi = InputContainer:extend({
  name = "rakuyomi"
})

-- We can get initialized from two contexts:
-- - when the `FileManager` is initialized, we're called
-- - when the `ReaderUI` is initialized, we're also called
-- so we should register to the menu accordingly
function Rakuyomi:init()
  if self.ui.name == "ReaderUI" then
    MangaReader:initializeFromReaderUI(self.ui)
  else
    self.ui.menu:registerToMainMenu(self)
  end

  if not ok or not android then
    CbzDocument:register(DocumentRegistry)
  end
  Dispatcher:registerAction("start_library_view", {
    category = "none",
    event = "StartLibraryView",
    title = _("Rakuyomi"),
    general = true
  })

  Testing:init()
  Testing:emitEvent('initialized')
end

function Rakuyomi:onStartLibraryView()
  if self.ui.name == "ReaderUI" then
    MangaReader:initializeFromReaderUI(self.ui)
  else
    self:openLibraryView()
  end
end

function Rakuyomi:addToMainMenu(menu_items)
  menu_items.rakuyomi = {
    text = _("Rakuyomi"),
    sorting_hint = "search",
    callback = function()
      self:openLibraryView()
    end
  }
end

function Rakuyomi:showErrorDialog()
  ErrorDialog:show(
    _("Oops!") .. _("Rakuyomi encountered an issue while starting up!") .. "\n" ..
    _("Here are some messages that might help identify the problem:") .. "\n\n" ..
    Backend.getLogs(),
    function()
      Backend.cleanup()
      Backend.getBackend()
    end
  )
end

---@class OpenOptions
---@field hideTopClose? boolean - Whether to hide the top close button
---@field focus_manga_id? string
---@field focus_manga_source_id? string
---@param options OpenOptions?
function Rakuyomi:openLibraryView(options)
  Backend.getBackend()
  if not Backend.getInitialized() then
    self:showErrorDialog()

    return
  end

  LibraryView:fetchAndShow(nil, nil, options)
  OfflineAlertDialog:showIfOffline()
end

function Rakuyomi:openFromToolbar()
  self:openLibraryView()
end

return Rakuyomi
