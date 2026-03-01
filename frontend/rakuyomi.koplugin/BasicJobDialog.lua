local InfoMessage = require("ui/widget/infomessage")
local InputContainer = require("ui/widget/container/inputcontainer")
local UIManager = require("ui/uimanager")
local ErrorDialog = require("ErrorDialog")
local _ = require("gettext+")
local ffiutil = require("ffi/util")

--- @class BasicJobDialog
--- @field job Job|nil
--- @field show_parent unknown|nil
--- @field cancellation_requested boolean
--- @field dismiss_callback fun()|nil
--- @field title string|nil
--- @field success_message string|nil
--- @field error_prefix string|nil
--- @field format_progress nil|fun(data: any):string|nil
--- @field extend any
--- @field new any
--- @field widget any
local BasicJobDialog = {
    show_parent = nil,
    -- The job.
    job = nil,
    -- If cancellation was requested.
    cancellation_requested = false,
    -- A callback to be called when dismissed.
    dismiss_callback = nil,
    title = nil,
    success_message = nil,
    error_prefix = nil,
    format_progress = nil,
    -- Current active widget (InfoMessage or ProgressbarDialog)
    widget = nil,
    widget_type = nil, -- 'info' or 'progress'
    is_finished = false,
    errors = {},
}

function BasicJobDialog:new(o)
    o = o or {}
    setmetatable(o, self)
    self.__index = self
    o.errors = {}
    return o
end

function BasicJobDialog:extend(o)
    o = o or {}
    setmetatable(o, self)
    self.__index = self
    return o
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
function BasicJobDialog:pollAndCreateTextWidget()
    local state = self.job:poll()
    local message = ''

    if state.type == 'SUCCESS' then
        message = self.cancellation_requested and _("Cancelled!") or (self.success_message or _('Complete!'))
    elseif state.type == 'PENDING' then
        if self.cancellation_requested then
            message = _("Waiting until cancelled…")
        elseif self.format_progress then
            local progress_text = self.format_progress(state.body)
            if progress_text and progress_text ~= "" then
                message = (self.title or '') .. "\n\n" .. progress_text
            else
                message = self.title or _("Processing...")
            end
        else
            message = (self.title or '') .. "\n\n" .. _("Processing...")
        end
    elseif state.type == 'ERROR' then
        message = (self.error_prefix or _("An error occurred")) ..
            ": " .. (state.message or (state.body and state.body.message) or _("Unknown error"))
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

function BasicJobDialog:show()
    self:updateProgress()
end

function BasicJobDialog:updateProgress()
    -- Unschedule any remaining update calls we might have.
    UIManager:unschedule(self.updateProgress)

    local state = self.job:poll()

    if state.type == 'PENDING' then
        local data = state.body
        local wants_progress = data and (data.total or data.progress_max)
        local is_progress = self.widget_type == 'progress'

        if not self.widget or (wants_progress and not is_progress) then
            if self.widget then
                UIManager:close(self.widget)
            end
            self:createWidget(data)
        end

        self:updateWidget(data)

        if not self.is_finished then
            UIManager:scheduleIn(1, self.updateProgress, self)
        end
    else
        self.is_finished = true
        self:onJobFinished(state)
    end
end

function BasicJobDialog:createWidget(data)
    if data and (data.total or data.progress_max) then
        local ProgressbarDialog = require("ui/widget/progressbardialog")
        self.widget = ProgressbarDialog:new {
            title = self.title or _("Processing..."),
            progress_max = data.total or data.progress_max or 100,
            on_cancel = function()
                self:onCancellationRequested()
            end
        }
        self.widget_type = 'progress'
    else
        self.widget = InfoMessage:new {
            modal = false,
            text = self.title or _("Processing..."),
            dismissable = true,
        }
        overrideInfoMessageDismissHandler(self.widget, function()
            self:onCancellationRequested()
        end)
        self.widget_type = 'info'
    end

    UIManager:show(self.widget)
end

local function setWidgetText(widget, text)
    if not widget then return end
    if widget.setText then
        widget:setText(text)
    elseif widget.setMessage then
        widget:setMessage(text)
    end
end

function BasicJobDialog:updateWidget(data)
    if not self.widget then return end

    if self.cancellation_requested then
        setWidgetText(self.widget, _("Waiting until cancelled…"))
        return
    end

    if data and data.current and self.widget.reportProgress then
        self.widget:reportProgress(data.current)
        if self.widget.redrawProgressbarIfNeeded then
            self.widget:redrawProgressbarIfNeeded()
        end
    end

    if data and data.errors then
        for _, err in ipairs(data.errors) do
            local exists = false
            for _, existing_err in ipairs(self.errors) do
                if existing_err == err then
                    exists = true
                    break
                end
            end
            if not exists then
                table.insert(self.errors, err)
            end
        end
    end

    if self.format_progress then
        local progress_text = self.format_progress(data)
        if progress_text then
            setWidgetText(self.widget, (self.title or "") .. "\n\n" .. progress_text)
        end
    end
end

function BasicJobDialog:onJobFinished(state)
    if self.widget then
        UIManager:close(self.widget)
        self.widget = nil
    end

    if state.type == 'SUCCESS' then
        if state.body and type(state.body) == 'table' then
            -- Collect final errors if any
            for _, err in ipairs(state.body) do
                table.insert(self.errors, err)
            end
        end

        if #self.errors > 0 then
            ErrorDialog:show(self.error_prefix .. "\n\n" .. table.concat(self.errors, "\n"))
        elseif not self.cancellation_requested then
            UIManager:show(InfoMessage:new {
                text = self.success_message or _("Complete!")
            })
        else
            UIManager:show(InfoMessage:new {
                text = _("Cancelled!")
            })
        end
    elseif state.type == 'ERROR' then
        ErrorDialog:show((self.error_prefix or _("Error")) .. ": " .. (state.message or _("Unknown error")))
    end

    self:onDismiss()
end

function BasicJobDialog:onCancellationRequested()
    self.job:requestCancellation()
    self.cancellation_requested = true

    UIManager:nextTick(self.updateProgress, self)
end

--- @private
function BasicJobDialog:onDismiss()
    if self.dismiss_callback ~= nil then
        self.dismiss_callback()
    end
end

return BasicJobDialog
