local ConfirmBox = require("ui/widget/confirmbox")
local CheckButton = require("ui/widget/checkbutton")
local UIManager = require("ui/uimanager")
local NetworkMgr = require("ui/network/manager")
local logger = require("logger")

local OfflineAlertDialog = {}

-- Setting key name for the "do not show again" preference
local SETTINGS_KEY = "rakuyomi_offline_alert_do_not_show_again"

--- Shows an alert dialog if the user is not connected to the internet.
--- @param if_online_callback function|nil Callback to be called if the user is online.
function OfflineAlertDialog:showIfOffline(if_online_callback)
  if NetworkMgr:isConnected() ~= true then
    NetworkMgr:beforeWifiAction(function()
      self:_showIfOffline(if_online_callback)
    end)
  else
    self:_showIfOffline(if_online_callback)
  end
end

--- Shows an alert dialog if the user is not connected to the internet.
--- @param if_online_callback function|nil Callback to be called if the user is online.
function OfflineAlertDialog:_showIfOffline(if_online_callback)
  -- Check if we're connected
  if NetworkMgr:isConnected() then
    if if_online_callback ~= nil then
      if_online_callback()
    end

    return
  end

  -- Check if user doesn't want to see the dialog again
  if G_reader_settings:isTrue(SETTINGS_KEY) then
    logger.info("OfflineAlertDialog:showIfOffline() - Dialog skipped due to user preference")

    return
  end

  logger.info("OfflineAlertDialog:showIfOffline() - Showing offline alert dialog")

  local message =
      "You appear to be offline. Some features may be limited.\n\n" ..
      "• You can still read any downloaded manga chapters\n" ..
      "• Your library content will be available\n" ..
      "• New content and updates require a connection"

  local do_not_show_again = false

  local dialog = ConfirmBox:new {
    icon = "notice-warning",
    text = message,
    cancel_text = "Close",
    no_ok_button = true,
    cancel_callback = function()
      if do_not_show_again then
        G_reader_settings:saveSetting(SETTINGS_KEY, true)
      end
    end,
  }

  local check_button = CheckButton:new {
    text = "Do not show this message again",
    callback = function()
      do_not_show_again = not do_not_show_again
    end,
    parent = dialog,
  }
  dialog:addWidget(check_button)

  UIManager:show(dialog)
end

return OfflineAlertDialog
