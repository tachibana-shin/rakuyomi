local InfoMessage = require("ui/widget/infomessage")
local UIManager = require("ui/uimanager")
local Trapper = require("ui/trapper")

local LoadingDialog = {}

--- Shows a message in a info dialog, while running the given `runnable` function.
--- Must be called from inside a function wrapped with `Trapper:wrap()`.
---
--- @generic T: any
--- @param message string The message to be shown on the dialog.
--- @param runnable fun(): T The function to be ran while showing the dialog.
--- @param onCancel fun()?: T An optional function to be called if the dialog is dismissed/cancelled.
--- @return T, boolean
function LoadingDialog:showAndRun(message, runnable, onCancel, bypass_trapper_check)
  if not bypass_trapper_check then
    assert(Trapper:isWrapped(), "expected to be called inside a function wrapped with `Trapper:wrap()`")
  end

  local message_dialog = InfoMessage:new {
    text = message,
    dismissable = onCancel ~= nil,
  }

  local cancelled = false
  if (onCancel ~= nil) then
    -- Override the dismiss handler to call `onCancel`.
    local originalOnTapClose = message_dialog.onTapClose
    message_dialog.onTapClose = function(self)
      cancelled = true
      onCancel()
      originalOnTapClose(self)
    end
  end

  UIManager:show(message_dialog)
  UIManager:forceRePaint()

  local completed, return_values = Trapper:dismissableRunInSubprocess(runnable, message_dialog)
  if onCancel == nil then
    assert(completed, "Expected runnable to run to completion")
  end

  UIManager:close(message_dialog)

  return return_values, cancelled
end

return LoadingDialog
