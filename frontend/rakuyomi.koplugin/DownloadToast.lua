local Backend = require("Backend")
local Blitbuffer = require("ffi/blitbuffer")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local InputContainer = require("ui/widget/container/inputcontainer")
local Screen = require("device").screen
local TextBoxWidget = require("ui/widget/textboxwidget")
local Trapper = require("ui/trapper")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local time = require("ui/time")
local _ = require("gettext+")

local DownloadToast = { entries = {}, page = 1 }
local ROWS_PER_PAGE = 3
local TOAST_SCALE = 3
local REFRESH_SECONDS = 2
local FINISHED_SECONDS = 6

-- A widget flagged `toast` deliberately propagates even consumed taps. Use a
-- floating window instead, forwarding only unhandled input to the reader, so
-- Cancel never also turns a page. New dialogs still stack above this window.
local DownloadWindow = InputContainer:extend {
    name = "rakuyomi_download_toast",
    modal = true,
    covers_fullscreen = false,
}

function DownloadWindow:handleEvent(event)
    if InputContainer.handleEvent(self, event) then return true end
    if event.handler ~= "onGesture" and event.handler ~= "onKeyPress"
        and event.handler ~= "onKeyRepeat" and event.handler ~= "onKeyRelease" then
        return false
    end
    local below = false
    for widget in UIManager:topdown_widgets_iter() do
        if below and not widget.toast then
            if widget:handleEvent(event) then return true end
            for _, active in ipairs(widget.active_widgets or {}) do
                if active:handleEvent(event) then return true end
            end
            return false
        end
        if widget == self then below = true end
    end
    return false
end

function DownloadWindow:paintTo(bb)
    if self.screen_width ~= Screen:getWidth() or self.screen_height ~= Screen:getHeight() then self.owner:build() end
    InputContainer.paintTo(self, bb, self.dimen.x, self.dimen.y)
end

local function cell(text, width, height, face, callback)
    local widget = InputContainer:new {
        dimen = Geom:new { w = width, h = height },
        TextBoxWidget:new {
            text = text, face = face, width = width, height = height,
            alignment = "left", height_overflow_show_ellipsis = true,
        },
    }
    if callback then
        widget.ges_events = {
            Tap = { GestureRange:new { ges = "tap", range = widget.dimen } },
        }
        widget.onTap = function() callback(); return true end
    end
    return widget
end

local function statusText(entry)
    if entry.cancelling or (entry.cancelled and not entry.finished_at) then return _("Cancelling…") end
    if entry.cancel_failed then return _("Cancel failed; try again") end
    local state = entry.job.download_status or entry.job.result
    if not state or state.type == "PENDING" then
        local data = state and state.body
        if not data or data.type == "INITIALIZING" or not data.total or data.total <= 0 then
            return _("Waiting / preparing…")
        end
        local done = data.processed or data.downloaded or data.current or 0
        local percent = math.min(100, math.floor(done * 100 / data.total))
        local unit = data.downloaded ~= nil and _("chapters") or _("pages")
        return string.format("%d%% · %d/%d %s", percent, done, data.total, unit)
    end
    if entry.cancelled then return _("Cancelled") end
    if state.type == "ERROR" then return _("Failed") .. ": " .. (state.message or _("Unknown error")) end
    local body = state.body
    local errors = type(body) == "table" and (type(body[2]) == "table" and body[2] or body)
    if errors and #errors > 0 then return _("Complete with errors") end
    return _("Complete")
end

function DownloadToast:build()
    local window = self.widget
    local padding = Screen:scaleByDPI(8) * TOAST_SCALE
    local border = TOAST_SCALE
    local width = math.min(
        math.min(math.floor(Screen:getWidth() * 0.36), Screen:scaleByDPI(300)) * TOAST_SCALE,
        Screen:getWidth() - 2 * padding
    )
    local inner_width = width - 2 * padding - 2 * border
    local row_height = Screen:scaleByDPI(48) * TOAST_SCALE
    local header_height = Screen:scaleByDPI(26) * TOAST_SCALE
    local cancel_width = Screen:scaleByDPI(70) * TOAST_SCALE
    local rows_per_page = math.min(ROWS_PER_PAGE, math.max(1,
        math.floor((Screen:getHeight() - 4 * padding - header_height - 2 * border) / row_height)))
    -- Font:getFace scales by screen size; this compact panel uses DPI-sized
    -- rows, so compensate to keep the glyphs inside those rows.
    local font_scale = Screen:scaleByDPI(160) / Screen:scaleBySize(160) * TOAST_SCALE
    local face = Font:getFace("smallinfofont", 15 * font_scale)
    local title_face = Font:getFace("smallinfofont", 16 * font_scale)
    local count = #self.entries
    local pages = math.max(1, math.ceil(count / rows_per_page))
    self.page = math.min(self.page, pages)
    local group = VerticalGroup:new { align = "left" }
    local page_width = pages > 1 and cancel_width or 0
    group[#group + 1] = HorizontalGroup:new {
        align = "center",
        cell(_("Downloads") .. " (" .. count .. ")", inner_width - page_width, header_height, title_face),
        pages > 1 and cell(self.page .. "/" .. pages .. " ›", page_width, header_height, face, function()
            self.page = self.page % pages + 1
            self.signature = nil
            self:render()
        end) or nil,
    }
    local first = (self.page - 1) * rows_per_page + 1
    for index = first, math.min(count, first + rows_per_page - 1) do
        local entry = self.entries[index]
        local pending = not entry.finished_at
        local cancel = pending and not entry.cancelling and not entry.cancelled and _("Cancel") or ""
        group[#group + 1] = HorizontalGroup:new {
            align = "center",
            VerticalGroup:new {
                align = "left",
                cell(entry.label, inner_width - cancel_width, row_height / 2, face),
                cell(statusText(entry), inner_width - cancel_width, row_height / 2, face),
            },
            cell(cancel, cancel_width, row_height, face, cancel ~= "" and function() self:cancel(entry) end or nil),
        }
    end
    if window[1] then window[1]:free() end
    window[1] = FrameContainer:new {
        background = Blitbuffer.COLOR_WHITE, bordersize = border, padding = padding,
        group,
    }
    local size = window[1]:getSize()
    window.dimen = Geom:new {
        x = Screen:getWidth() - size.w - padding,
        y = Screen:getHeight() - size.h - padding,
        w = size.w, h = size.h,
    }
    window.screen_width = Screen:getWidth()
    window.screen_height = Screen:getHeight()
end

function DownloadToast:render()
    if #self.entries == 0 then
        if self.widget then
            UIManager:close(self.widget, "ui", self.widget.dimen)
            self.widget = nil
        end
        self.signature = nil
        return
    end
    local parts = { self.page, Screen:getWidth(), Screen:getHeight() }
    for _, entry in ipairs(self.entries) do
        parts[#parts + 1] = (entry.job.job_id or tostring(entry.job)) .. entry.label .. statusText(entry)
    end
    local signature = table.concat(parts, "\n")
    if signature == self.signature then return end
    self.signature = signature
    if not self.widget then
        self.widget = DownloadWindow:new { owner = self }
        self:build()
        UIManager:show(self.widget, "ui", self.widget.dimen)
    else
        local old = self.widget.dimen:copy()
        self:build()
        -- Include the old panel rectangle when rows shrink; never request a
        -- full-screen refresh or reconstruct a dialog on every poll.
        local new = self.widget.dimen
        local region = Geom:new {
            x = math.min(old.x, new.x), y = math.min(old.y, new.y),
            w = math.max(old.x + old.w, new.x + new.w) - math.min(old.x, new.x),
            h = math.max(old.y + old.h, new.y + new.h) - math.min(old.y, new.y),
        }
        UIManager:setDirty("all", "ui", region)
    end
end

function DownloadToast:add(job, label, on_finished)
    for _, entry in ipairs(self.entries) do
        if entry.job == job or (job.job_id and entry.job.job_id == job.job_id) then
            entry.label = label
            if entry.finished_at and on_finished then
                on_finished(entry.job.result, entry.cancelled)
            elseif on_finished then
                entry.on_finished = on_finished
            end
            self:render()
            return
        end
    end
    job.download_status = job.result or { type = "PENDING", body = { type = "INITIALIZING" } }
    local entry = { job = job, label = label, on_finished = on_finished }
    if job.result then
        entry.finished_at = time.now()
        if on_finished then on_finished(job.result, false) end
    end
    self.entries[#self.entries + 1] = entry
    self:render()
    if not job.job_id and not job.result then self:start(entry) end
    if not self.running then
        self.running = true
        UIManager:scheduleIn(0, self.poll, self)
    end
end

function DownloadToast:start(entry)
    -- Creating a job is also a socket request. Keep it out of the UI thread,
    -- and return the child-assigned ID to the parent before polling/cancelling.
    Trapper:wrap(function()
        local completed, outcome = Trapper:dismissableRunInSubprocess(function()
            local response = entry.job:start()
            return { response = response, job_id = entry.job.job_id }
        end, self.widget)
        entry.job.started = true
        entry.job.start_result = outcome and outcome.response
        entry.job.job_id = outcome and outcome.job_id
        if not completed or not entry.job.job_id then
            local response = outcome and outcome.response
            local message = type(response) == "table" and response.message or _("Could not start download")
            local state = entry.job:updateFromResponse { type = "ERROR", message = message }
            entry.job.download_status = state
            entry.finished_at = time.now()
            entry.cancelling = false
            if entry.on_finished then entry.on_finished(state, false) end
        elseif entry.cancelling then
            entry.cancelling = false
            self:cancel(entry)
        end
        self:render()
    end)
end

function DownloadToast:cancel(entry)
    if entry.cancelling or entry.finished_at then return end
    entry.cancel_requested = true
    entry.cancelling = true
    entry.cancel_failed = false
    self:render()
    if not entry.job.job_id then return end -- Cancel as soon as creation returns its ID.
    Trapper:wrap(function()
        local completed, cancelled = Trapper:dismissableRunInSubprocess(function()
            return entry.job:requestCancellation()
        end, self.widget)
        if entry.finished_at then return end -- Completion won the backend race.
        entry.cancelling = false
        entry.cancelled = completed and cancelled
        entry.cancel_failed = not entry.cancelled
        self:render()
    end)
end

function DownloadToast:poll()
    -- This is the sole backend poller for registered download jobs. Callers
    -- read Job:poll's shared snapshot; completion is consumed only once.
    Trapper:wrap(function()
        local entries = {}
        for _, entry in ipairs(self.entries) do entries[#entries + 1] = entry end
        for _, entry in ipairs(entries) do
            if not entry.finished_at and entry.job.job_id then
                local completed, response = Trapper:dismissableRunInSubprocess(function()
                    return Backend.getJobDetails(entry.job.job_id)
                end, self.widget)
                if completed and response then
                    local state = entry.job:updateFromResponse(response)
                    entry.job.download_status = state
                    if state.type ~= "PENDING" then
                        entry.finished_at = time.now()
                        entry.cancelling = false
                        if entry.on_finished then
                            entry.on_finished(state, entry.cancelled or entry.cancel_requested)
                        end
                    end
                end
            end
        end
        for index = #self.entries, 1, -1 do
            local finished = self.entries[index].finished_at
            if finished and time.to_s(time.now() - finished) >= FINISHED_SECONDS then
                table.remove(self.entries, index)
            end
        end
        self:render()
        self.running = #self.entries > 0
        if self.running then UIManager:scheduleIn(REFRESH_SECONDS, self.poll, self) end
    end)
end

return DownloadToast
