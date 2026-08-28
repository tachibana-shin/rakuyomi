local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local Trapper = require("ui/trapper")
local Icons = require("Icons")
local Button = require("ui/widget/button")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local LoadingDialog = require("LoadingDialog")
local Menu = require("widgets/Menu")
local _ = require("gettext+")
local Testing = require("testing")
local CheckboxDialog = require("CheckboxDialog")
local format_languages = require("utils/formatLanguages")
local langNames = require("utils/languageNames")
---@diagnostic disable-next-line: different-requires
local util = require("util")

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

--- Computes the key of a source list from its URL, mirroring the backend's
--- `source_list_key`. For GitHub URLs the key is `owner/repo`; for any other
--- host it is the domain (with the port stripped).
--- @param url string
--- @return string
local function source_list_key(url)
  local scheme, host, path = url:match("^([%w+.-]+)://([^/]+)(.*)$")
  if not scheme then
    host, path = url:match("^([^/]+)(.*)$")
  end
  if not host then
    host = url
  end
  host = host:gsub(":%d+$", ""):lower()
  if host == "github.com" or host == "raw.githubusercontent.com" then
    local owner, repo = (path or ""):match("^/([^/]+)/([^/]+)")
    if owner and repo then
      return owner .. "/" .. repo
    end
  end
  return host
end

--- Drops entries from a persisted filter selection that no longer exist in
--- the current options (e.g. a source list was removed since the selection
--- was made). Keeping them would count them in the filter badge while being
--- impossible to uncheck in the dialog.
--- @param current string[]
--- @param options { id: string }[]
--- @param transform fun(id: string): string
--- @return string[]
local function sanitize_selection(current, options, transform)
  local valid = {}
  for _, option in ipairs(options) do
    valid[option.id] = true
  end

  local cleaned = {}
  local seen = {}
  for _, id in ipairs(current or {}) do
    local key = transform(id)
    if valid[key] and not seen[key] then
      seen[key] = true
      cleaned[#cleaned + 1] = key
    end
  end
  return cleaned
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
  -- selectable repositories, built from `settings.source_lists`
  repos = {},
  -- repositories selected by the user; empty means no repository filter
  repos_selected = {},
  -- the filter button group inserted in the title bar, replaced on refresh
  filter_group = nil,
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
  self:extractAvailableRepos()
  self.langs_selected = sanitize_selection(self.langs_selected, self.langs, langNames.normalize)
  self.repos_selected = sanitize_selection(self.repos_selected, self.repos, function(id) return id end)
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

--- Builds the list of selectable languages from the user-configured
--- languages in the reader settings (managed in the plugin's Settings screen)
--- plus the languages found in the available sources.
---
--- The dialog displays each entry's `name` (e.g. "English", "Chinese"), so
--- the list is sorted by displayed label, with the normalised `id` as a
--- deterministic tie-breaker for any two entries that share a name. Sorting
--- by the underlying code alone would order e.g. "Arabic", "English",
--- "French", "Chinese" — which is not alphabetical by what users see.
--- @private
function AvailableSourcesListing:extractAvailableLangs()
  local langs_set = {}
  local langs_list = {}

  -- The languages managed in the Languages screen are stored in the
  -- global reader settings; read them from there so both screens share one
  -- source of truth.
  for _, lang in ipairs(G_reader_settings:readSetting("rakuyomi_languages", {})) do
    local key = langNames.normalize(lang)
    if not langs_set[key] then
      langs_set[key] = true
      table.insert(langs_list, key)
    end
  end
  for _, source_information in ipairs(self.available_sources) do
    for _, lang in ipairs(source_information.languages) do
      local key = langNames.normalize(lang)
      if not langs_set[key] then
        langs_set[key] = true
        table.insert(langs_list, key)
      end
    end
  end

  -- Build the id+name pairs first so we can sort by the displayed label.
  -- The `id` is the canonical key the dialog stores, the filter compares
  -- against, and `sanitize_selection` validates; ordering is purely a
  -- display concern, so changing it cannot break selection/normalisation.
  self.langs = {}
  for _, lang in ipairs(langs_list) do
    table.insert(self.langs, { id = lang, name = langNames.nameFor(lang) })
  end
  table.sort(self.langs, function(a, b)
    if a.name == b.name then
      return a.id < b.id
    end
    return a.name < b.name
  end)
end

--- Builds the list of selectable repositories (source list keys) from the
--- configured source lists in the settings.
--- @private
function AvailableSourcesListing:extractAvailableRepos()
  local repos_list = {}
  for _, list in ipairs(self.settings.source_lists or {}) do
    local repo = source_list_key(list.url)
    if repo ~= "" then
      table.insert(repos_list, repo)
    end
  end

  table.sort(repos_list)

  self.repos = {}
  for _, repo in ipairs(repos_list) do
    table.insert(self.repos, { id = repo, name = repo })
  end
end

--- Filters the available sources by the selected languages and repositories.
--- When no language is selected, all sources are shown; sources without any
--- language information are always shown. Repositories work the same way:
--- an empty selection keeps every source.
--- @private
--- @return SourceInformation[]
function AvailableSourcesListing:filterAvailableSources()
  local langs_set = {}
  for _, lang in ipairs(self.langs_selected) do
    langs_set[lang] = true
  end
  local repos_set = {}
  for _, repo in ipairs(self.repos_selected) do
    repos_set[repo] = true
  end

  local filtered = {}
  for __, source_information in ipairs(self.available_sources) do
    local lang_matches = #self.langs_selected == 0 or #source_information.languages == 0
    for _, lang in ipairs(source_information.languages) do
      if langs_set[langNames.normalize(lang)] then
        lang_matches = true
        break
      end
    end
    local repo = source_information.source_of_source or _("Unknown")
    local repo_matches = #self.repos_selected == 0 or repos_set[repo]
    if lang_matches and repo_matches then
      table.insert(filtered, source_information)
    end
  end

  return filtered
end

--- Opens the language selection dialog and applies the filter. The
--- selection is persisted in the reader settings under the
--- `rakuyomi_langs_selected` key.
--- @private
function AvailableSourcesListing:showSelectLanguage()
  ---@diagnostic disable-next-line: redundant-parameter
  local dialog = CheckboxDialog:new {
    title = _("Languages"),
    current = self.langs_selected,
    options = self.langs,
    update_callback = function(value)
      self.langs_selected = value
      G_reader_settings:saveSetting("rakuyomi_langs_selected", value)
      self:updateItems()
      self:patchTitleBar()
      UIManager:setDirty(self.show_parent, "ui", self.dimen)
    end,
  }

  UIManager:show(dialog)
end

--- Opens the repository selection dialog and applies the filter. The
--- selection is persisted in the reader settings under the
--- `rakuyomi_repos_selected` key.
--- @private
function AvailableSourcesListing:showSelectRepos()
  ---@diagnostic disable-next-line: redundant-parameter
  local dialog = CheckboxDialog:new {
    title = _("Repositories"),
    current = self.repos_selected,
    options = self.repos,
    update_callback = function(value)
      self.repos_selected = value
      G_reader_settings:saveSetting("rakuyomi_repos_selected", value)
      self:updateItems()
      self:patchTitleBar()
      UIManager:setDirty(self.show_parent, "ui", self.dimen)
    end,
  }

  UIManager:show(dialog)
end

--- Adds the language and repository filter buttons to the title bar.
--- @private
function AvailableSourcesListing:patchTitleBar()
  if #self.langs == 0 and #self.repos == 0 then
    return
  end

  local left_icon_size_ratio = self.title_bar.left_icon_size_ratio
  local left_icon_size = Screen:scaleBySize(DGENERIC_ICON_SIZE * left_icon_size_ratio)

  local buttons = {}

  if #self.langs > 0 then
    local count = #self.langs_selected
    buttons[#buttons + 1] = VerticalGroup:new {
      Button:new {
        text = Icons.LANG .. (count > 0 and " " .. count or ""),
        face = SMALL_FONT_FACE,
        bordersize = 0,
        enabled = true,
        width = left_icon_size,
        height = left_icon_size,
        text_font_size = 16,
        text_font_bold = false,
        callback = function()
          self:showSelectLanguage()
        end,
      },
      VerticalSpan:new {
        width = left_icon_size / 2,
      },
    }
  end

  if #self.repos > 0 then
    local repo_count = #self.repos_selected
    buttons[#buttons + 1] = VerticalGroup:new {
      Button:new {
        text = Icons.REPO .. (repo_count > 0 and " " .. repo_count or ""),
        face = SMALL_FONT_FACE,
        bordersize = 0,
        enabled = true,
        width = left_icon_size,
        height = left_icon_size,
        text_font_size = 16,
        text_font_bold = false,
        callback = function()
          self:showSelectRepos()
        end,
      },
      VerticalSpan:new {
        width = left_icon_size / 2,
      },
    }
  end

  -- Insert the filter buttons on the left side of the title bar. When the
  -- menu has no left icon, the close button lives at [2], so we must insert
  -- instead of replacing it. The buttons must be grouped in a single widget:
  -- the title bar is an OverlapGroup, where separate children would all be
  -- painted at the same position and overlap each other. The group is only
  -- inserted once; later calls replace it in place, otherwise the stale
  -- copies would be painted on top of the fresh one.
  local filter_group = HorizontalGroup:new(buttons)
  self.title_bar.left_button = filter_group
  if self.title_bar[2] ~= nil then
    self.title_bar[2] = filter_group
  else
    table.insert(self.title_bar, 2, filter_group)
  end
  self.filter_group = filter_group
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

  local languages_text = format_languages(source_information.languages)
  local post_text = source_information.source_of_source
      and string.sub(source_information.source_of_source, 1, 6) .. "..." or
      _("Unknown")
  if languages_text then
    post_text = languages_text .. " · " .. post_text
  end

  return {
    source_information = source_information,
    text = source_information.name .. " (" .. _("version") .. " " .. tostring(source_information.version) .. ")",
    mandatory = mandatory,
    post_text = post_text,
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
  self:installSourceWithLanguages(source_information, nil)
end

--- @private
--- @param source_information SourceInformation
--- @param languages string[]|nil
function AvailableSourcesListing:installSourceWithLanguages(source_information, languages)
  Trapper:wrap(function()
    local response = LoadingDialog:showAndRun(
      _("Installing source..."),
      function() return Backend.installSource(source_information.id, source_information.source_of_source, languages) end
    )

    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)

      return
    end

    if response.body.type == 'selection_required' then
      self:showLanguageSelection(source_information, response.body)

      return
    end

    self:refreshAfterInstall(source_information)
  end)
end

--- Asks which languages of a multi-source keiyoushi APK to install, then
--- installs the selection.
---
--- `outcome.languages` carries the raw identifiers the keiyoushi probe
--- bundled (see `install_source.rs` `bundled_languages` and the
--- `bundled.contains(lang.as_str())` validation). Those exact strings are
--- what the backend expects when we call `Backend.installSource`, so we
--- must keep `id = lang` unchanged — normalising here would have the
--- backend reject e.g. `"en-US"` as not bundled.
---
--- The dialog label, however, is `langNames.nameFor(lang)` which strips
--- BCP-47 subtags. When an APK bundles variants like `en` and `en-US`,
--- both rows would display as `"English"` and become indistinguishable.
--- When two or more raw IDs share a displayed name, we append the raw
--- code in parentheses to disambiguate; unique labels stay clean.
--- @private
--- @param source_information SourceInformation
--- @param outcome InstallOutcomeSelectionRequired
function AvailableSourcesListing:showLanguageSelection(source_information, outcome)
  local name_counts = {}
  for _, lang in ipairs(outcome.languages) do
    local display = langNames.nameFor(lang)
    name_counts[display] = (name_counts[display] or 0) + 1
  end

  local options = {}
  for _, lang in ipairs(outcome.languages) do
    local display = langNames.nameFor(lang)
    if name_counts[display] > 1 then
      display = display .. " (" .. lang .. ")"
    end
    table.insert(options, {
      id = lang,
      name = display,
    })
  end

  -- Pre-check the language of the tapped entry; fall back to checking
  -- every language when the entry carries none.
  -- Build a normalized lookup of outcome languages so that equivalent
  -- codes from the source metadata (e.g. "en") match raw outcome IDs
  -- with subtags (e.g. "en-US"). We store the raw outcome IDs (not the
  -- source metadata IDs) in `current` because the backend validates
  -- languages by exact string against `bundled.contains()`.
  local outcome_ids = {}
  for _, lang in ipairs(outcome.languages) do
    local key = langNames.normalize(lang)
    outcome_ids[key] = outcome_ids[key] or {}
    table.insert(outcome_ids[key], lang)
  end

  local current = {}
  local current_set = {}
  if source_information.languages then
    for _, lang in ipairs(source_information.languages) do
      for _, outcome_lang in ipairs(outcome_ids[langNames.normalize(lang)] or {}) do
        if not current_set[outcome_lang] then
          current_set[outcome_lang] = true
          table.insert(current, outcome_lang)
        end
      end
    end
  end
  if #current == 0 then
    current = util.tableDeepCopy(outcome.languages)
  end

  local selected = current
  ---@diagnostic disable-next-line: redundant-parameter
  local dialog = CheckboxDialog:new {
    title = _("Select languages to install") .. ": " .. outcome.name,
    current = current,
    options = options,
    update_callback = function(value)
      selected = value
    end,
    dismiss_callback = function()
      if #selected == 0 then
        self:showLanguageSelection(source_information, outcome)

        return
      end
      self:installSourceWithLanguages(source_information, selected)
    end,
  }

  UIManager:show(dialog)
end

--- @private
--- @param source_information SourceInformation
function AvailableSourcesListing:refreshAfterInstall(source_information)
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
    langs_selected = G_reader_settings:readSetting("rakuyomi_langs_selected", {}),
    repos_selected = G_reader_settings:readSetting("rakuyomi_repos_selected", {}),
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)

  Testing:emitEvent("available_sources_listing_shown")
end

return AvailableSourcesListing
