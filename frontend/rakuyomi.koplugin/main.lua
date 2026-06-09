local DocumentRegistry = require("document/documentregistry")
local InputContainer = require("ui/widget/container/inputcontainer")
local UIManager = require("ui/uimanager")
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

logger.info("Loading Rakuyomi plugin...")
local backendInitialized, logs
function getBackend() 
  if backendInitialized then return end
  backendInitialized, logs = Backend.initialize()
end

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

    local orig_onShow = self.ui.onShow
    self.ui.onShow = function(fm_self, ...)
      if orig_onShow then orig_onShow(fm_self, ...) end
      if not self._rakuyomi_started then
        self._rakuyomi_started = true
        if G_reader_settings:readSetting("start_with") == "rakuyomi" then
          UIManager:scheduleIn(0, function()
            getBackend()
            if not backendInitialized then
              self:showErrorDialog()
              return
            end
            LibraryView:fetchAndShow()
            OfflineAlertDialog:showIfOffline()
          end)
        end
      end
    end

    -- ---------------------------------------------------------------------------
    -- "Start with Rakuyomi" menu entry
    -- Injects the Rakuyomi radio item into KOReader's Start With submenu.
    -- Patched once per session; a flag on the class prevents double-patching.
    -- ---------------------------------------------------------------------------
    local ok_fmm, FileManagerMenu = pcall(require, "apps/filemanager/filemanagermenu")
    if ok_fmm and FileManagerMenu and not FileManagerMenu._rakuyomi_startwith_patched then
      local orig_fn = FileManagerMenu.getStartWithMenuTable
      if orig_fn then
        FileManagerMenu._rakuyomi_startwith_patched = true
        FileManagerMenu._rakuyomi_startwith_orig    = orig_fn

        --- Overrides the default Start With menu table to inject a Rakuyomi option.
        --- @param fmm_self FileManagerMenu The FileManagerMenu instance.
        --- @return table result The modified menu table with the Rakuyomi entry added.
        FileManagerMenu.getStartWithMenuTable = function(fmm_self)
          local result = orig_fn(fmm_self)
          local sub    = result.sub_item_table
          if type(sub) ~= "table" then return result end

          -- Add the entry only if it is not already present.
          local rakuyomi_label = _("Rakuyomi")
          local found = false
          for _, item in ipairs(sub) do
            if item.text == rakuyomi_label and item.radio then found = true; break end
          end
          if not found then
            table.insert(sub, math.max(1, #sub), {
              text         = rakuyomi_label,
              -- Read the setting directly as ground truth; fall back to the
              -- cache only when the setting hasn't been written yet.
              checked_func = function()
                return G_reader_settings:readSetting("start_with") == "rakuyomi"
              end,
              callback = function()
                G_reader_settings:saveSetting("start_with", "rakuyomi")
              end,
              radio = true,
            })
          end

          -- Update the parent item label when Rakuyomi is the active choice.
          local orig_text_func = result.text_func
          result.text_func = function(...)
            if G_reader_settings:readSetting("start_with") == "rakuyomi" then
              return _("Start with") .. ": " .. _("Rakuyomi")
            end
            return orig_text_func and orig_text_func(...) or _("Start with")
          end

          return result
        end
      end
    end
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
    getBackend()
    if not backendInitialized then
      self:showErrorDialog()

      return
    end

    self:openLibraryView()
  end
end

function Rakuyomi:addToMainMenu(menu_items)
  menu_items.rakuyomi = {
    text = _("Rakuyomi"),
    sorting_hint = "search",
    callback = function()
      getBackend()
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
    _("Oops!") .. _("Rakuyomi encountered an issue while starting up!") .. "\n" ..
    _("Here are some messages that might help identify the problem:") .. "\n\n" ..
    logs,
    function()
      Backend.cleanup()
      backendInitialized, logs = nil, nil
      getBackend()
    end
  )
end

function Rakuyomi:openLibraryView()
  LibraryView:fetchAndShow()
  OfflineAlertDialog:showIfOffline()
end

function Rakuyomi:openFromToolbar()
  self:openLibraryView()
end

return Rakuyomi
