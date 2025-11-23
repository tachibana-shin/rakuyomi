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
--- @return T
function LoadingDialog:showAndRun(message, runnable, onCancel, bypass_trapper_check)
  if not bypass_trapper_check then
    assert(Trapper:isWrapped(), "expected to be called inside a function wrapped with `Trapper:wrap()`")
  end

  local message_dialog = InfoMessage:new {
    text = message,
    dismissable = onCancel ~= nil,
  }

  if (onCancel ~= nil) then
    -- Override the dismiss handler to call `onCancel`.
    local originalOnTapClose = message_dialog.onTapClose
    message_dialog.onTapClose = function(self)
      onCancel()
      originalOnTapClose(self)
    end

    local originalOnAnyKeyPressed = message_dialog.onAnyKeyPressed
    message_dialog.onAnyKeyPressed = function(self)
      onCancel()
      originalOnAnyKeyPressed(self)
    end
  end

  UIManager:show(message_dialog)
  UIManager:forceRePaint()

  local completed, return_values = Trapper:dismissableRunInSubprocess(runnable, message_dialog)
  assert(completed, "Expected runnable to run to completion")

  UIManager:close(message_dialog)

  return return_values
end

return LoadingDialog
