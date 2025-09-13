local UIManager = require("ui/uimanager")
local ConfirmBox = require("ui/widget/confirmbox")
local Screen = require("device").screen
local Trapper = require("ui/trapper")

local AvailableSourcesListing = require("AvailableSourcesListing")
local Backend = require("Backend")
local Menu = require("widgets/Menu")
local ErrorDialog = require("ErrorDialog")
local SourceSettings = require("SourceSettings")
local Testing = require("testing")

--- @class InstalledSourcesListing: { [any]: any }
--- @field installed_sources SourceInformation[]
--- @field on_return_callback fun(): nil
local InstalledSourcesListing = Menu:extend {
  name = "installed_sources_listing",
  is_enable_shortcut = false,
  is_popout = false,
  title = "Installed sources",
  with_context_menu = true,

  installed_sources = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
}

function InstalledSourcesListing:init()
  self.installed_sources = self.installed_sources or {}
  self.title_bar_left_icon = "plus"
  self.onLeftButtonTap = function()
    self:openAvailableSourcesListing()
  end

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()
  Menu.init(self)

  -- see `ChapterListing` for an explanation on this
  -- FIXME we could refactor this into a single class
  self.paths = {
    { callback = self.on_return_callback },
  }

  self:updateItems()
end

--- Updates the menu item contents with the sources information
--- @private
function InstalledSourcesListing:updateItems()
  if #self.installed_sources > 0 then
    self.item_table = self:generateItemTableFromInstalledSources(self.installed_sources)
    self.multilines_show_more_text = false
    self.items_per_page = nil
  else
    self.item_table = self:generateEmptyViewItemTable()
    self.multilines_show_more_text = true
    self.items_per_page = 1
  end

  Menu.updateItems(self)
end

--- Generates the item table for displaying the search results.
--- @private
--- @param installed_sources SourceInformation[]
--- @return table
function InstalledSourcesListing:generateItemTableFromInstalledSources(installed_sources)
  local item_table = {}
  for _, source_information in ipairs(installed_sources) do
    table.insert(item_table, {
      source_information = source_information,
      text = source_information.name .. " (version " .. source_information.version .. ")",
    })
  end

  return item_table
end

--- @private
function InstalledSourcesListing:generateEmptyViewItemTable()
  return {
    {
      text =
          "No installed sources found. Try installing some by tapping " ..
          "the top-left button to list the available sources.",
      dim = true,
      select_enabled = false,
    }
  }
end

--- @private
function InstalledSourcesListing:onPrimaryMenuChoice(item)
  --- @type SourceInformation
  local source_information = item.source_information

  local on_return_callback = function()
    self:fetchAndShow(self.on_return_callback)
  end

  SourceSettings:fetchAndShow(source_information.id, on_return_callback)

  UIManager:close(self)
end

--- @private
function InstalledSourcesListing:onContextMenuChoice(item)
  --- @type SourceInformation
  local source_information = item.source_information

  UIManager:show(ConfirmBox:new {
    text = "Do you want to remove the \"" .. source_information.name .. "\" source?",
    ok_text = "Remove",
    ok_callback = function()
      local response = Backend.uninstallSource(source_information.id)

      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end

      local response = Backend.listInstalledSources()

      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end

      self.installed_sources = response.body

      self:updateItems()
    end
  })
end

--- @private
function InstalledSourcesListing:onReturn()
  local path = table.remove(self.paths)

  self:onClose()
  path.callback()
end

--- @private
function InstalledSourcesListing:openAvailableSourcesListing()
  Trapper:wrap(function()
    local onReturnCallback = function()
      self:fetchAndShow(self.on_return_callback)
    end

    AvailableSourcesListing:fetchAndShow(onReturnCallback)

    self:onClose()
  end)
end

--- Fetches and shows the installed sources.
--- @param onReturnCallback fun(): nil
function InstalledSourcesListing:fetchAndShow(onReturnCallback)
  local response = Backend.listInstalledSources()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local installed_sources = response.body

  local ui = InstalledSourcesListing:new {
    installed_sources = installed_sources,
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)

  Testing:emitEvent("installed_sources_listing_shown")
end

return InstalledSourcesListing
