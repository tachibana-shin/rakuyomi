local ButtonDialog = require("ui/widget/buttondialog")
local InputDialog = require("ui/widget/inputdialog")
local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local _ = require("gettext+")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Menu = require("widgets/Menu")
local Testing = require("testing")

--- @class SourceListsListing: { [any]: any }
--- @field settings Settings
--- @field on_return_callback fun(): nil
local SourceListsListing = Menu:extend {
  name = "source_lists_listing",
  is_enable_shortcut = false,
  is_popout = false,
  title = _("Source lists"),

  settings = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
}

--- @type table<string, string>
local SOURCE_LIST_TYPE_LABELS = {
  aidoku = "Aidoku",
  lnreader = "LNReader",
  mangayomi = "MangaYomi",
  keiyoushi = "Keiyoushi",
}

function SourceListsListing:init()
  self.title_bar_left_icon = "plus"
  self.onLeftButtonTap = function()
    self:showAddSourceList()
  end

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()
  Menu.init(self)

  self:updateItems()

  -- see `ChapterListing` for an explanation on this
  -- FIXME we could refactor this into a single class
  self.paths = { 0 }
end

function SourceListsListing:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

--- Updates the menu item contents with the source lists information.
--- @private
function SourceListsListing:updateItems()
  local source_lists = self.settings.source_lists or {}
  if #source_lists > 0 then
    self.item_table = self:generateItemTable(source_lists)
    self.multilines_show_more_text = false
    self.items_per_page = nil
    -- post_text (the list type) is only rendered on single-line items
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
--- @param source_lists SourceList[]
--- @return table
function SourceListsListing:generateItemTable(source_lists)
  local item_table = {}
  for index, source_list in ipairs(source_lists) do
    table.insert(item_table, {
      source_list = source_list,
      index = index,
      text = source_list.url,
      post_text = SOURCE_LIST_TYPE_LABELS[source_list.type] or SOURCE_LIST_TYPE_LABELS.aidoku,
    })
  end

  return item_table
end

--- @private
function SourceListsListing:generateEmptyViewItemTable()
  return {
    {
      text = _("No source lists configured.") .. " " .. _("Tap the top-left button to add one."),
      dim = true,
      select_enabled = false,
    }
  }
end

--- @private
function SourceListsListing:onPrimaryMenuChoice(item)
  local dialog_context_menu

  dialog_context_menu = ButtonDialog:new {
    title = item.source_list.url,
    buttons = {
      {
        {
          text = _("Remove"),
          callback = function()
            UIManager:close(dialog_context_menu)

            self:removeSourceList(item.index)
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

--- Opens the input dialog to add a new source list.
--- @private
function SourceListsListing:showAddSourceList()
  local input_dialog

  input_dialog = InputDialog:new {
    title = _("Add source list"),
    input_hint = _("URL of the source list (index.json or plugins index)"),
    description = _("For example:") .. "\n" ..
        "https://tachibana-shin.github.io/aidoku-sources-next/index.min.json\n" ..
        "https://github.com/lnreader/lnreader-plugins\n" ..
        "https://kodjodevf.github.io/mangayomi-extensions/index.json\n" ..
        "https://kodjodevf.github.io/mangayomi-extensions/novel_index.json\n" ..
        "https://keiyoushi.github.io/extensions/index.min.json",
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
          text = _("Next"),
          is_enter_default = true,
          callback = function()
            local url = input_dialog:getInputText()
            UIManager:close(input_dialog)

            if url == '' then
              ErrorDialog:show(_("Please enter a source list URL."))

              return
            end

            self:chooseSourceListType(url)
          end,
        },
      }
    }
  }

  UIManager:show(input_dialog)
  input_dialog:onShowKeyboard()
end

--- Lets the user pick the index format of the new source list.
--- @private
--- @param url string
function SourceListsListing:chooseSourceListType(url)
  local dialog_context_menu

  dialog_context_menu = ButtonDialog:new {
    title = url,
    buttons = {
      {
        {
          text = "Aidoku",
          callback = function()
            UIManager:close(dialog_context_menu)

            self:addSourceList(url, "aidoku")
          end
        },
        {
          text = "LNReader",
          callback = function()
            UIManager:close(dialog_context_menu)

            self:addSourceList(url, "lnreader")
          end
        },
        {
          text = "MangaYomi",
          callback = function()
            UIManager:close(dialog_context_menu)

            self:addSourceList(url, "mangayomi")
          end
        },
        {
          text = "Keiyoushi",
          callback = function()
            UIManager:close(dialog_context_menu)

            self:addSourceList(url, "keiyoushi")
          end
        }
      }
    }
  }

  UIManager:show(dialog_context_menu)
end

--- @private
--- @param url string
--- @param source_type "aidoku"|"lnreader"|"mangayomi"|"keiyoushi"
function SourceListsListing:addSourceList(url, source_type)
  self.settings.source_lists = self.settings.source_lists or {}
  table.insert(self.settings.source_lists, {
    url = url,
    type = source_type,
  })

  self:persist()
end

--- @private
--- @param index number
function SourceListsListing:removeSourceList(index)
  table.remove(self.settings.source_lists, index)

  self:persist()
end

--- Saves the settings and refreshes the listing.
--- @private
function SourceListsListing:persist()
  local response = Backend.setSettings(self.settings)
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  self:updateItems()
end

--- @private
function SourceListsListing:onReturn()
  table.remove(self.paths)

  self:onClose()
end

--- Fetches the settings and shows the source lists.
--- @param onReturnCallback fun(): nil
function SourceListsListing:fetchAndShow(onReturnCallback)
  local response = Backend.getSettings()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local ui = SourceListsListing:new {
    settings = response.body,
    on_return_callback = onReturnCallback,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
  }
  ui.on_return_callback = onReturnCallback
  UIManager:show(ui)

  Testing:emitEvent("source_lists_listing_shown")
end

return SourceListsListing
