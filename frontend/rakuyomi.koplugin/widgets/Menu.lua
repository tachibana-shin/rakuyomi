local BaseMenu = require("ui/widget/menu")
local NetworkMgr = require("ui/network/manager")
local logger = require("logger")

local Icons = require("Icons")

local Menu = BaseMenu:extend {
  with_context_menu = false,
}

function Menu:init()
  if self.with_context_menu then
    self.align_baselines = true
  end

  self:updateOfflineSubtitle(true)

  BaseMenu.init(self)
end

function Menu:updateItems(select_number)
  for _, item in ipairs(self.item_table) do
    if self.with_context_menu and item.select_enabled ~= false then
      item.mandatory = (item.mandatory and (item.mandatory .. " ") or "") .. Icons.FA_ELLIPSIS_VERTICAL
    end
  end

  BaseMenu.updateItems(self, select_number)
end

function Menu:onMenuSelect(entry, pos)
  if entry.select_enabled == false then
    return true
  end

  local selected_context_menu = pos ~= nil and pos.x > 0.8

  if selected_context_menu then
    self:onContextMenuChoice(entry, pos)
  else
    self:onPrimaryMenuChoice(entry, pos)
  end
end

function Menu:onMenuHold(entry, pos)
  self:onContextMenuChoice(entry, pos)
end

--- Defaults to calling the entry's callback.
--- Override this function to change the behavior.
function Menu:onPrimaryMenuChoice(entry, pos)
  if entry.callback then
    entry.callback()
  end

  return true
end

function Menu:onContextMenuChoice(entry, pos)
end

---@private
function Menu:onNetworkConnected()
  logger.info("Menu:onNetworkConnected()")

  self:updateOfflineSubtitle()
end

---@private
function Menu:onNetworkDisconnected()
  logger.info("Menu:onNetworkDisconnected()")

  self:updateOfflineSubtitle()
end

---@private
function Menu:updateOfflineSubtitle(skip_reinit)
  if not NetworkMgr:isConnected() then
    NetworkMgr:beforeWifiAction(function()
      self:_updateOfflineSubtitle(skip_reinit)
    end)
  else
    self:_updateOfflineSubtitle(skip_reinit)
  end
end

---@private
function Menu:_updateOfflineSubtitle(skip_reinit)
  if NetworkMgr:isConnected() then
    self.subtitle = nil
  else
    self.subtitle = Icons.WIFI_OFF .. " Offline mode"
  end

  if not skip_reinit then
    BaseMenu.init(self)
  end
end

return Menu
