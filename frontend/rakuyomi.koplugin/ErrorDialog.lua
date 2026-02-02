local InfoMessage = require("ui/widget/infomessage")
local ConfirmBox = require("ui/widget/confirmbox")
local UIManager = require("ui/uimanager")
local _ = require("gettext+")

local ErrorDialog = {}

---@param message string
---@param try_refresh fun()?
function ErrorDialog:show(message, try_refresh)
  local dialog
  dialog = try_refresh and ConfirmBox:new({
    text = message,
    icon = "notice-warning",
    ok_text = _("Retry"),
    ok_callback = try_refresh,
    cancel_callback = function()
      UIManager:close(dialog)
    end
  }) or InfoMessage:new({
    text = message,
    icon = "notice-warning",
  })

  UIManager:show(dialog)
end

return ErrorDialog
