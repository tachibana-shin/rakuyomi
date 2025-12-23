local InfoMessage = require("ui/widget/infomessage")
local ConfirmBox = require("ui/widget/confirmbox")
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
--- @param onConfirmCancel (fun(any): any) | nil
--- @param bypass_trapper_check boolean?
--- @return T, boolean
function LoadingDialog:showAndRun(message, runnable, onCancel, onConfirmCancel, bypass_trapper_check)
  if not bypass_trapper_check then
    assert(Trapper:isWrapped(), "expected to be called inside a function wrapped with `Trapper:wrap()`")
  end

  local cancelled = false
  local conConfirmCancel = nil
  local message_dialog = onCancel == nil and InfoMessage:new {
    text = message,
    dismissable = false,
  } or ConfirmBox:new {
    text = message,
    icon = "notice-info",
    no_ok_button = true,
    -- dismissable = false,
    cancel_callback = function()
      local cancel = function()
        cancelled = true
        if onCancel ~= nil then
          onCancel()
        end
      end

      if onConfirmCancel ~= nil then
        conConfirmCancel = onConfirmCancel(cancel)
      else
        cancel()
      end
    end
  }

  UIManager:show(message_dialog)
  UIManager:forceRePaint()

  local completed, return_values = Trapper:dismissableRunInSubprocess(runnable, message_dialog)
  if onCancel == nil then
    assert(completed, "Expected runnable to run to completion")
  end

  if conConfirmCancel ~= nil then
    UIManager:close(conConfirmCancel)
  end
  UIManager:close(message_dialog)

  return return_values, cancelled
end

--- @param message string The message to be shown on the dialog.
--- @param onCancel fun()?: T An optional function to be called if the dialog is dismissed/cancelled.
--- @param onConfirmCancel (fun(any): any)?
--- @diagnostic disable-next-line: undefined-doc-name
--- @return InfoMessage|ConfirmBox, InfoMessage|ConfirmBox|nil
function LoadingDialog:simple(message, onCancel, onConfirmCancel)
  local conConfirmCancel = nil
  local message_dialog = onCancel == nil and InfoMessage:new {
    text = message,
    dismissable = false,
  } or ConfirmBox:new {
    text = message,
    icon = "notice-info",
    no_ok_button = true,
    dismissable = false,
    cancel_callback = function()
      local cancel = function()
        if onCancel ~= nil then
          onCancel()
        end
      end

      if onConfirmCancel ~= nil then
        conConfirmCancel = onConfirmCancel(cancel)
      else
        cancel()
      end
    end
  }

  UIManager:show(message_dialog)

  return message_dialog, conConfirmCancel
end

return LoadingDialog
