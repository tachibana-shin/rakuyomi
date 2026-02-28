local Trapper = require("ui/trapper")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Menu = require("widgets/Menu")

local ConfirmBox = require("ui/widget/confirmbox")
local Device = require("device")
local UIManager = require("ui/uimanager")
local ffiUtil = require("ffi/util")
local _ = require("gettext+")
local ChapterListing = require("ChapterListing")
local InfoMessage = require("ui/widget/infomessage")

local MenuUpdateItems = require("patch/MenuUpdateItems")
local MenuItemCover = require("patch/MenuItemCover")

local Screen = Device.screen
local T = ffiUtil.template

--- @class Menu
--- @field new fun(self: Menu): Menu
--- @field dimen any
--- @field item_group any
--- @field page_info any
--- @field return_button any
--- @field content_group any
--- @field _recalculateDimen fun(bool)
--- @field items_max_lines number
--- @field page_items any[]
--- @field perpage number
--- @field items_font_size number|nil
--- @field title_bar any
--- @field no_title boolean|nil
--- @field page_return_arrow any
--- @field page_info_text string|nil
--- @field inner_dimen any
--- @field setupItemHeights fun()
--- @field getPageNumber fun()
--- @field itemnumber number
--- @field is_enable_shortcut boolean
--- @field item_shortcuts any[]
--- @field item_dimen any
--- @field updatePageInfo fun(any)
--- @field mergeTitleBarIntoLayout fun()
--- @field show_parent any
--- @field onClose fun()
--- @field openMenu fun()

--- @class NotificationView : Menu
--- @field notifications Notification[]
--- @field on_return_callback fun()|nil
local NotificationView = Menu:extend {
  name = "notification_view",
  is_enable_shortcut = false,
  is_popout = false,
  title = _("Notification"),
  with_context_menu = true,

  items_per_page = 10,
  notifications = nil,
  on_return_callback = nil
}

function NotificationView:init()
  self.mangas = self.mangas or {}

  self.title_bar_left_icon = "appbar.pokeball"
  self.onLeftButtonTap = function()
    local response = Backend.clearNotifications()
    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)

      return
    end

    UIManager:show(InfoMessage:new {
      text = _("Cleared all notifications!")
    })

    self.notifications = {}
    self:updateItems()
  end

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()

  local page = self.page
  Menu.init(self)
  self.page = page

  self:updateItems()
end

--- @param select_number number|nil
---@param no_recalculate_dimen boolean|nil
function NotificationView:updateItems(select_number, no_recalculate_dimen)
  if #self.notifications > 0 then
    local item_table = {}
    for _, notify in ipairs(self.notifications) do
      table.insert(item_table, {
        notify = notify,
        text = notify.manga_title,
        post_text = "Ch." .. (notify.chapter_number or "unknown") .. ": " .. notify.chapter_title,
      })
    end
    self.item_table = item_table
    self.multilines_show_more_text = false
    self.items_per_page = nil
  else
    self.item_table = self:generateEmptyViewItemTable()
    self.multilines_show_more_text = true
    self.items_per_page = 1
  end

  MenuUpdateItems(self, MenuItemCover, select_number, no_recalculate_dimen)
  print(#self.notifications)
end

--- @private
function NotificationView:generateEmptyViewItemTable()
  return {
    {
      text = _("No notification"),
      dim = true,
      select_enabled = false,
    }
  }
end

--- @param onReturnCallback fun()
function NotificationView:fetchAndShow(onReturnCallback)
  local response = Backend.getNotifications()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)

    return
  end

  local notifications = response.body

  ---@diagnostic disable-next-line: redundant-parameter
  local widget = NotificationView:new {
    notifications = notifications,
    covers_fullscreen = true, -- hint for UIManager:_repaint()
    page = self.page,
    on_return_callback = onReturnCallback
  }
  ---@diagnostic disable-next-line: inject-field
  widget.on_return_callback = onReturnCallback
  UIManager:show(widget)
end

--- @private
function NotificationView:onMenuSelect(item)
  local onReturnCallback = function()
    self:fetchAndShow(self.on_return_callback)
  end

  Trapper:wrap(function()
    --- @type Notification
    local notify = item.notify
    local manga = {
      id = notify.chapter_id.manga_id.manga_id,
      source = {
        id = notify.chapter_id.manga_id.source_id
      },
      title = notify.manga_title
    }

    if ChapterListing:fetchAndShow(manga, onReturnCallback, true) then
      self:onClose(false)
    end
  end)
end

function NotificationView:onMenuHold(item)
  local confirm_dialog
  confirm_dialog = ConfirmBox:new {
    text = _("Delete this notification?"),
    ok_text = _("Delete"),
    cancel_text = _("Cancel"),
    ok_callback = function()
      UIManager:close(confirm_dialog)

      local response = Backend.removeNotification(item.id)
      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end

      local response = Backend.getNotifications()
      if response.type == 'ERROR' then
        ErrorDialog:show(response.message)

        return
      end

      self.notifications = response.body
      self:updateItems()
    end,
    cancel_callback = function()
      UIManager:close(confirm_dialog)
    end
  }

  UIManager:show(confirm_dialog)
  return true
end

function NotificationView:onClose(call_return)
  UIManager:close(self)
  if self.on_return_callback and call_return ~= false then
    self.on_return_callback()
  end
end

return NotificationView
