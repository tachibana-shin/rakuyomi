local InfoMessage = require("ui/widget/infomessage")
local InputContainer = require("ui/widget/container/inputcontainer")
local UIManager = require("ui/uimanager")

local logger = require('logger')

--- @class DownloadUnreadChaptersJobDialog
--- @field job DownloadUnreadChapters
--- @field show_parent unknown
--- @field cancellation_requested boolean
--- @field dismiss_callback fun():nil|nil
--- @field new any
local DownloadUnreadChaptersJobDialog = InputContainer:extend {
  show_parent = nil,
  modal = true,
  -- The `DownloadUnreadChapters` job.
  job = nil,
  -- If cancellation was requested.
  cancellation_requested = false,
  -- A callback to be called when dismissed.
  dismiss_callback = nil,
}

function DownloadUnreadChaptersJobDialog:init()
  local widget, _ = self:pollAndCreateTextWidget()
  self[1] = widget
end

local function overrideInfoMessageDismissHandler(widget, new_dismiss_handler)
  -- Override the default `onTapClose`/`onAnyKeyPressed` actions
  local originalOnTapClose = widget.onTapClose
  widget.onTapClose = function(messageSelf)
    new_dismiss_handler()

    originalOnTapClose(messageSelf)
  end

  local originalOnAnyKeyPressed = widget.onAnyKeyPressed
  widget.onAnyKeyPressed = function(messageSelf)
    new_dismiss_handler()

    originalOnAnyKeyPressed(messageSelf)
  end
end

--- @private
function DownloadUnreadChaptersJobDialog:pollAndCreateTextWidget()
  local state = self.job:poll()
  local message = ''

  if state.type == 'SUCCESS' then
    message = self.cancellation_requested and 'Download cancelled!' or 'Download complete!'
  elseif state.type == 'PENDING' then
    if self.cancellation_requested then
      message = 'Waiting until downloads are cancelled…'
    elseif state.body.type == 'INITIALIZING' then
      message = 'Downloading chapters, this will take a while…'
    else
      message = 'Downloading chapters, this will take a while… (' ..
          state.body.downloaded .. '/' .. state.body.total .. ')'
    end
  elseif state.type == 'ERROR' then
    message = 'An error occurred while downloading chapters: ' .. state.message
  end

  local is_cancellable = state.type == 'PENDING' and not self.cancellation_requested
  local is_finished = state.type ~= 'PENDING'

  local widget = InfoMessage:new {
    modal = false,
    text = message,
    dismissable = is_cancellable or is_finished,
  }

  overrideInfoMessageDismissHandler(widget, function()
    if is_cancellable then
      self:onCancellationRequested()

      return
    end

    self:onDismiss()
  end)

  return widget, is_finished
end

function DownloadUnreadChaptersJobDialog:show()
  UIManager:show(self)

  UIManager:nextTick(self.updateProgress, self)
end

--- @private
function DownloadUnreadChaptersJobDialog:updateProgress()
  -- Unschedule any remaining update calls we might have.
  UIManager:unschedule(self.updateProgress)

  local old_message_size = self[1]:getVisibleArea()
  -- Request a redraw of the component we're drawing over.
  UIManager:setDirty(self.show_parent, function()
    return 'ui', old_message_size
  end)

  local widget, is_finished = self:pollAndCreateTextWidget()
  self[1] = widget
  self.dimen = nil

  -- Request a redraw of ourselves.
  UIManager:setDirty(self, 'ui')

  if not is_finished then
    UIManager:scheduleIn(1, self.updateProgress, self)
  end
end

--- @private
function DownloadUnreadChaptersJobDialog:onCancellationRequested()
  self.job:requestCancellation()
  self.cancellation_requested = true

  UIManager:nextTick(self.updateProgress, self)
end

--- @private
function DownloadUnreadChaptersJobDialog:onDismiss()
  UIManager:close(self)

  if self.dismiss_callback ~= nil then
    self.dismiss_callback()
  end
end

return DownloadUnreadChaptersJobDialog
