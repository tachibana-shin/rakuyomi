local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local Trapper = require("ui/trapper")
local Icons = require("Icons")
local Button = require("ui/widget/button")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local LoadingDialog = require("LoadingDialog")
local Menu = require("widgets/Menu")
local _ = require("gettext+")
local Testing = require("testing")
local CheckboxDialog = require("CheckboxDialog")

local DGENERIC_ICON_SIZE = G_defaults:readSetting("DGENERIC_ICON_SIZE")
local Font = require("ui/font")
local SMALL_FONT_FACE = Font:getFace("smallffont")

--- Compares two source versions. Versions may be numbers (Aidoku) or
--- strings (LNReader). When both sides are numeric the comparison is
--- numeric; otherwise the numeric parts are compared segment by segment,
--- so that e.g. "2.10.0" sorts after "2.9.0".
--- @param a string|number
--- @param b string|number
--- @return boolean true when `a` is older than `b`
local function version_less(a, b)
  local na, nb = tonumber(a), tonumber(b)
  if na ~= nil and nb ~= nil then
    return na < nb
  end

  local function parts(value)
    local result = {}
    for part in tostring(value):gmatch("%d+") do
      result[#result + 1] = tonumber(part)
    end
    return result
  end

  local pa, pb = parts(a), parts(b)
  for i = 1, math.max(#pa, #pb) do
    local x, y = pa[i] or 0, pb[i] or 0
    if x ~= y then
      return x < y
    end
  end
  return false
end

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
  -- the application settings, used to persist the language filter
  settings = nil,
  -- selectable languages, built from `available_sources`
  langs = {},
  -- languages selected by the user; empty means no language filter
  langs_selected = {},
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

  self:extractAvailableLangs()
  self:patchTitleBar()

  -- self:updateItems()
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
  local available_sources = self:filterAvailableSources()
  if #available_sources > 0 then
    self.item_table = self:generateItemTableFromInstalledAndAvailableSources(self.installed_sources,
      available_sources)
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

--- Builds the list of selectable languages from all available sources.
--- @private
function AvailableSourcesListing:extractAvailableLangs()
  local langs_set = {}
  local langs_list = {}
  for _, source_information in ipairs(self.available_sources) do
    for _, lang in ipairs(source_information.languages) do
      if not langs_set[lang] then
        langs_set[lang] = true
        table.insert(langs_list, lang)
      end
    end
  end

  table.sort(langs_list)

  self.langs = {}
  for _, lang in ipairs(langs_list) do
    table.insert(self.langs, { id = lang, name = lang })
  end
end

--- Filters the available sources by the selected languages. When no
--- language is selected, all sources are shown; sources without any
--- language information are always shown.
--- @private
--- @return SourceInformation[]
function AvailableSourcesListing:filterAvailableSources()
  if #self.langs_selected == 0 then
    return self.available_sources
  end

  local langs_set = {}
  for _, lang in ipairs(self.langs_selected) do
    langs_set[lang] = true
  end

  local filtered = {}
  for _, source_information in ipairs(self.available_sources) do
    local matches = #source_information.languages == 0
    for _, lang in ipairs(source_information.languages) do
      if langs_set[lang] then
        matches = true
        break
      end
    end
    if matches then
      table.insert(filtered, source_information)
    end
  end

  return filtered
end

--- Opens the language selection dialog and applies the filter. The
--- selection is persisted in the application settings via the existing
--- `languages` field.
--- @private
function AvailableSourcesListing:showSelectLanguage()
  local dialog = CheckboxDialog:new {
    title = _("Languages"),
    current = self.langs_selected,
    options = self.langs,
    update_callback = function(value)
      self.langs_selected = value
      self.settings.languages = value
      Backend.setSettings(self.settings)
      self:updateItems()
      self:patchTitleBar()
      UIManager:setDirty(self.show_parent, "ui", self.dimen)
    end,
  }

  UIManager:show(dialog)
end

--- Adds the language filter button to the title bar.
--- @private
function AvailableSourcesListing:patchTitleBar()
  if #self.langs == 0 then
    return
  end

  local left_icon_size_ratio = self.title_bar.left_icon_size_ratio
  local left_icon_size = Screen:scaleBySize(DGENERIC_ICON_SIZE * left_icon_size_ratio)

  local count = #self.langs_selected
  local lang_button = VerticalGroup:new {
    Button:new {
      text = Icons.LANG .. (count > 0 and " " .. count or ""),
      face = SMALL_FONT_FACE,
      bordersize = 0,
      enabled = true,
      text_font_size = left_icon_size,
      text_font_bold = false,
      callback = function()
        self:showSelectLanguage()
      end,
    },
    VerticalSpan:new {
      width = left_icon_size / 2,
    },
  }

  -- Insert the language button on the left side of the title bar. When the
  -- menu has no left icon, the close button lives at [2], so we must insert
  -- instead of replacing it.
  self.title_bar.left_button = lang_button
  if self.title_bar[2] ~= nil then
    table.insert(self.title_bar, 2, lang_button)
  end
end

---@private
---@param source_information SourceInformation
---@param installed_info SourceInformation
function AvailableSourcesListing:makeItem(source_information, installed_info)
  local mandatory
  local callback = nil

  if installed_info then
    -- Installed
    if version_less(installed_info.version, source_information.version) then
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
    text = source_information.name .. " (" .. _("version") .. " " .. tostring(source_information.version) .. ")",
    mandatory = mandatory,
    post_text = source_information.source_of_source
        and string.sub(source_information.source_of_source, 1, 6) .. "..." or
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

  local settings_response = Backend.getSettings()
  if settings_response.type == 'ERROR' then
    ErrorDialog:show(settings_response.message)

    return
  end
  local settings = settings_response.body

  local ui = AvailableSourcesListing:new {
    installed_sources = installed_sources,
    available_sources = available_sources,
    settings = settings,
    langs_selected = settings.languages or {},
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)

  Testing:emitEvent("available_sources_listing_shown")
end

return AvailableSourcesListing
