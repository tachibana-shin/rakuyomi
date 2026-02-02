local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local Trapper = require("ui/trapper")
local Icons = require("Icons")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local LoadingDialog = require("LoadingDialog")
local Menu = require("widgets/Menu")
local _ = require("gettext+")
local Testing = require("testing")

--- @class AvailableSourcesListing: { [any]: any }
--- @field installed_sources SourceInformation[]
--- @field available_sources SourceInformation[]
--- @field on_return_callback fun(): nil
local AvailableSourcesListing = Menu:extend {
  name = "available_sources_listing",
  is_enable_shortcut = false,
  is_popout = false,
  title = _("Available sources"),

  available_sources = nil,
  installed_sources = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
}

function AvailableSourcesListing:init()
  self.available_sources = self.available_sources or {}
  self.installed_sources = self.installed_sources or {}

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()
  Menu.init(self)

  -- see `ChapterListing` for an explanation on this
  -- FIXME we could refactor this into a single class
  self.paths = { 0 }
  self.on_return_callback = nil

  self:updateItems()
end

function AvailableSourcesListing:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

--- Updates the menu item contents with the sources information.
--- @private
function AvailableSourcesListing:updateItems()
  if #self.available_sources > 0 then
    self.item_table = self:generateItemTableFromInstalledAndAvailableSources(self.installed_sources, self
      .available_sources)
    self.multilines_show_more_text = false
    self.items_per_page = nil
    self.single_line = true
  else
    self.item_table = self:generateEmptyViewItemTable()
    self.multilines_show_more_text = true
    self.items_per_page = 1
    self.single_line = false
  end

  Menu.updateItems(self)
end

---@private
---@param source_information SourceInformation
---@param installed_info SourceInformation
function AvailableSourcesListing:makeItem(source_information, installed_info)
  local mandatory = ""
  local callback = nil

  if installed_info then
    -- Installed
    if installed_info.version < source_information.version then
      mandatory = Icons.FA_ARROW_UP .. " " .. _("Update available!")
      callback = function() self:installSource(source_information) end
    else
      mandatory = Icons.FA_CHECK .. " " .. _("Latest version installed")
    end
  else
    -- Not installed
    mandatory = Icons.FA_DOWNLOAD .. " " .. _("Installable")
    callback = function() self:installSource(source_information) end
  end

  return {
    source_information = source_information,
    text = source_information.name .. " (" .. _("version") .. " " .. source_information.version .. ")",
    mandatory = mandatory,
    post_text = source_information.source_of_source and string.sub(source_information.source_of_source, 1, 6) .. "..." or
        _("Unknown"),
    callback = callback,
  }
end

--- Generates the item table for displaying the search results.
--- @private
--- @param installed_sources SourceInformation[]
--- @param available_sources SourceInformation[]
--- @return table
function AvailableSourcesListing:generateItemTableFromInstalledAndAvailableSources(installed_sources, available_sources)
  --- Map installed by unique key (id@source)
  local installed_sources_by_key = {}
  for _, src in ipairs(installed_sources) do
    local key = src.id .. "@" .. (src.source_of_source or "")
    installed_sources_by_key[key] = src
  end

  local items_installed = {}
  local items_available = {}

  --- Generate two lists: installed-first & available-after
  for _, source_information in ipairs(available_sources) do
    local key = source_information.id .. "@" .. (source_information.source_of_source or "")
    local installed_info = installed_sources_by_key[key]

    local item = self:makeItem(source_information, installed_info)

    if installed_info then
      table.insert(items_installed, item)
    else
      table.insert(items_available, item)
    end
  end

  --- Merge: installed first, available later
  local final = {}
  for _, v in ipairs(items_installed) do table.insert(final, v) end
  for _, v in ipairs(items_available) do table.insert(final, v) end

  return final
end

--- @private
function AvailableSourcesListing:generateEmptyViewItemTable()
  return {
    {
      text = _("No available sources found.") .. " " .. _("Try adding some source lists by looking at our README!"),
      dim = true,
      select_enabled = false,
    }
  }
end

--- @private
function AvailableSourcesListing:onReturn()
  table.remove(self.paths, 1)
  self:onClose()
end

--- @private
--- @param source_information SourceInformation
function AvailableSourcesListing:installSource(source_information)
  Trapper:wrap(function()
    local response = LoadingDialog:showAndRun(
      _("Installing source..."),
      function() return Backend.installSource(source_information.id, source_information.source_of_source) end
    )

    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)

      return
    end

    local installed_sources_response = Backend.listInstalledSources()
    if installed_sources_response.type == 'ERROR' then
      ErrorDialog:show(installed_sources_response.message)

      return
    end

    self.installed_sources = installed_sources_response.body

    Testing:emitEvent("source_installed", {
      source = source_information
    })

    self:updateItems()
  end)
end

--- Fetches and shows the available sources. Must be called from a function wrapped with `Trapper:wrap()`.
--- @param onReturnCallback any
function AvailableSourcesListing:fetchAndShow(onReturnCallback)
  local installed_sources_response = Backend.listInstalledSources()
  if installed_sources_response.type == 'ERROR' then
    ErrorDialog:show(installed_sources_response.message)

    return
  end

  local installed_sources = installed_sources_response.body

  local available_sources_response = LoadingDialog:showAndRun("Fetching available sources...", function()
    return Backend.listAvailableSources()
  end)

  if available_sources_response.type == 'ERROR' then
    ErrorDialog:show(available_sources_response.message)

    return
  end

  local available_sources = available_sources_response.body

  local ui = AvailableSourcesListing:new {
    installed_sources = installed_sources,
    available_sources = available_sources,
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)

  Testing:emitEvent("available_sources_listing_shown")
end

return AvailableSourcesListing
