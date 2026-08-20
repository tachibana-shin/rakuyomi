local ButtonDialog = require("ui/widget/buttondialog")
local InputDialog = require("ui/widget/inputdialog")
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local _ = require("gettext+")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Menu = require("widgets/Menu")
local Testing = require("testing")

--- @class LanguagesListing: { [any]: any }
--- @field settings Settings
--- @field on_return_callback fun(): nil
local LanguagesListing = Menu:extend {
  name = "languages_listing",
  is_enable_shortcut = false,
  is_popout = false,
  title = _("Languages"),

  settings = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
}

function LanguagesListing:init()
  self.title_bar_left_icon = "plus"
  self.onLeftButtonTap = function()
    self:showAddLanguage()
  end

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()
  Menu.init(self)

  self:updateItems()

  -- see `ChapterListing` for an explanation on this
  -- FIXME we could refactor this into a single class
  self.paths = { 0 }
end

function LanguagesListing:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

--- Updates the menu item contents with the configured languages.
--- @private
function LanguagesListing:updateItems()
  local languages = self.settings.languages or {}
  if #languages > 0 then
    self.item_table = self:generateItemTable(languages)
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

--- @private
--- @param languages string[]
--- @return table
function LanguagesListing:generateItemTable(languages)
  local item_table = {}
  for index, language in ipairs(languages) do
    table.insert(item_table, {
      language = language,
      index = index,
      text = language,
    })
  end

  return item_table
end

--- @private
function LanguagesListing:generateEmptyViewItemTable()
  return {
    {
      text = _("No languages configured.") .. " " .. _("Tap the top-left button to add one."),
      dim = true,
      select_enabled = false,
    }
  }
end

--- @private
function LanguagesListing:onPrimaryMenuChoice(item)
  local dialog_context_menu

  dialog_context_menu = ButtonDialog:new {
    title = item.language,
    buttons = {
      {
        {
          text = _("Remove"),
          callback = function()
            UIManager:close(dialog_context_menu)

            self:removeLanguage(item.index)
          end
        },
        {
          text = _("Cancel"),
          callback = function()
            UIManager:close(dialog_context_menu)
          end
        }
      }
    }
  }

  UIManager:show(dialog_context_menu)
end

--- Opens the input dialog to add a new language code.
--- @private
function LanguagesListing:showAddLanguage()
  local input_dialog

  input_dialog = InputDialog:new {
    title = _("Add language"),
    input_hint = _("Language code (e.g. en, vi, ja)"),
    buttons = {
      {
        {
          text = _("Cancel"),
          id = "close",
          callback = function()
            UIManager:close(input_dialog)
          end,
        },
        {
          text = _("Add"),
          is_enter_default = true,
          callback = function()
            local code = input_dialog:getInputText()
            UIManager:close(input_dialog)

            if code == '' then
              ErrorDialog:show(_("Please enter a language code."))

              return
            end

            self:addLanguage(code)
          end,
        },
      }
    }
  }

  UIManager:show(input_dialog)
  input_dialog:onShowKeyboard()
end

--- @private
--- @param code string
function LanguagesListing:addLanguage(code)
  local languages = self.settings.languages or {}
  for _, existing in ipairs(languages) do
    if existing == code then
      ErrorDialog:show(_("This language is already in the list."))

      return
    end
  end

  table.insert(languages, code)
  table.sort(languages)
  self.settings.languages = languages

  self:persist()
end

--- @private
--- @param index number
function LanguagesListing:removeLanguage(index)
  local languages = self.settings.languages or {}
  table.remove(languages, index)
  self.settings.languages = languages

  self:persist()
end

--- Saves the settings and refreshes the listing.
--- @private
function LanguagesListing:persist()
  local response = Backend.setSettings(self.settings)
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  self:updateItems()
end

--- @private
function LanguagesListing:onReturn()
  table.remove(self.paths)

  self:onClose()
end

--- Fetches the settings and shows the languages listing.
--- @param onReturnCallback fun(): nil
function LanguagesListing:fetchAndShow(onReturnCallback)
  local response = Backend.getSettings()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local ui = LanguagesListing:new {
    settings = response.body,
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)

  Testing:emitEvent("languages_listing_shown")
end

return LanguagesListing
