local DocumentRegistry = require("document/documentregistry")
local InputContainer = require("ui/widget/container/inputcontainer")
local FileManager = require("apps/filemanager/filemanager")
local UIManager = require("ui/uimanager")
local InfoMessage = require("ui/widget/infomessage")
local Dispatcher = require("dispatcher")
local logger = require("logger")
local _ = require("gettext")
local OfflineAlertDialog = require("OfflineAlertDialog")

local Backend = require("Backend")
local CbzDocument = require("extensions/CbzDocument")
local ErrorDialog = require("ErrorDialog")
local LibraryView = require("LibraryView")
local MangaReader = require("MangaReader")
local Testing = require("testing")

logger.info("Loading Rakuyomi plugin...")
local backendInitialized, logs = Backend.initialize()

local Rakuyomi = InputContainer:extend({
  name = "rakuyomi"
})

Rakuyomi.instance = nil

-- We can get initialized from two contexts:
-- - when the `FileManager` is initialized, we're called
-- - when the `ReaderUI` is initialized, we're also called
-- so we should register to the menu accordingly
function Rakuyomi:init()
  Rakuyomi.instance = self

  if self.ui.name == "ReaderUI" then
    MangaReader:initializeFromReaderUI(self.ui)
  else
    self.ui.menu:registerToMainMenu(self)
  end

  CbzDocument:register(DocumentRegistry)
  Dispatcher:registerAction("start_library_view", {
    category = "none", 
    event = "StartLibraryView", 
    title = _("Rakuyomi"),
    general = true,
    })

  Testing:init()
  Testing:emitEvent('initialized')
end

function Rakuyomi:onStartLibraryView()
  if not Rakuyomi.instance then
    logger.warn("Rakuyomi:onStartLibraryView(): no instance available")
    UIManager:show(InfoMessage:new{
      text = _("Rakuyomi plugin is not ready yet."),
      timeout = 2,
    })
    return
  end

  Rakuyomi.instance:openFromToolbar()
end

function Rakuyomi:addToMainMenu(menu_items)
  menu_items.rakuyomi = {
    text = _("Rakuyomi"),
    sorting_hint = "search",
    callback = function()
      if not backendInitialized then
        self:showErrorDialog()

        return
      end

      self:openLibraryView()
    end
  }
end

function Rakuyomi:showErrorDialog()
  ErrorDialog:show(
    "Oops! Rakuyomi encountered an issue while starting up!\n" ..
    "Here are some messages that might help identify the problem:\n\n" ..
    logs
  )
end

function Rakuyomi:openLibraryView()
  LibraryView:fetchAndShow()
  OfflineAlertDialog:showIfOffline()
end

function Rakuyomi:openFromToolbar()
  -- Prevent re-entrancy (rapid taps / gestures / toolbar)
  if self._otb_busy then
    return
  end
  self._otb_busy = true

  local function done()
    self._otb_busy = false
  end

  if not backendInitialized then
    self:showErrorDialog()
    done()
    return
  end

  if self.ui and self.ui.name == "ReaderUI" then
    MangaReader:initializeFromReaderUI(self.ui)

    MangaReader:closeReaderUi(function()
      self:openLibraryView()
      done()
    end)
  else
    self:openLibraryView()
    done()
  end
end

package.loaded["rakuyomi"] = Rakuyomi
return Rakuyomi
