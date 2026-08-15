local UIManager = require("ui/uimanager")
local Screen = require("device").screen
local InfoMessage = require("ui/widget/infomessage")
local Trapper = require("ui/trapper")
local _ = require("gettext+")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Menu = require("widgets/Menu")
local Testing = require("testing")

--- @class TaskManagerView: { [any]: any }
--- @field installed_sources SourceInformation[]
--- @field usages table<string, SourceUsage>
--- @field on_return_callback fun(): nil
local TaskManagerView = Menu:extend {
  name = "task_manager_view",
  is_enable_shortcut = false,
  is_popout = false,
  title = _("Task manager"),

  installed_sources = nil,
  usages = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
}

local function humanizeBytes(bytes)
  if bytes < 1024 then
    return bytes .. " B"
  elseif bytes < 1024 * 1024 then
    return string.format("%.1f KiB", bytes / 1024)
  elseif bytes < 1024 * 1024 * 1024 then
    return string.format("%.1f MiB", bytes / (1024 * 1024))
  else
    return string.format("%.1f GiB", bytes / (1024 * 1024 * 1024))
  end
end

--- @private
local function humanizeDuration(ms)
  if ms < 1000 then
    return ms .. " ms"
  elseif ms < 60 * 1000 then
    return string.format("%.1f s", ms / 1000)
  else
    return string.format("%.1f min", ms / (60 * 1000))
  end
end

function TaskManagerView:init()
  self.installed_sources = self.installed_sources or {}
  self.usages = self.usages or {}

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()
  local page = self.page
  Menu.init(self)
  self.page = page

  self.paths = { 0 }

  self:updateItems()
end

function TaskManagerView:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

--- @private
function TaskManagerView:updateItems()
  local item_table = {}
  for __, source_information in ipairs(self.installed_sources) do
    local usage = self.usages[source_information.id]
    if usage then
      local parts = {}
      table.insert(parts, _("Calls") .. ": " .. usage.invokes .. " | "
        .. _("Total") .. ": " .. humanizeDuration(usage.total_duration_ms))
      if usage.peak_wasm_memory_bytes > 0 then
        table.insert(parts, _("Wasm memory") .. ": " .. humanizeBytes(usage.peak_wasm_memory_bytes))
      end
      if usage.disk_bytes > 0 then
        table.insert(parts, _("Disk") .. ": " .. humanizeBytes(usage.disk_bytes))
      end
      table.insert(item_table, {
        source_information = source_information,
        usage = usage,
        text = source_information.name,
        post_text = table.concat(parts, "\n"),
        dim = false,
      })
    else
      table.insert(item_table, {
        source_information = source_information,
        text = source_information.name,
        post_text = _("No usage recorded yet"),
        dim = true,
      })
    end
  end

  if #item_table == 0 then
    item_table = {
      {
        text = _("No installed sources found"),
        dim = true,
        select_enabled = false,
      }
    }
  end

  self.item_table = item_table
  self.multilines_show_more_text = false
  self.items_per_page = nil
  Menu.updateItems(self)
end

--- @private
function TaskManagerView:onPrimaryMenuChoice(item)
  if not item.usage then
    return
  end

  local usage = item.usage
  local parts = {
    item.source_information.name,
    "",
    _("Calls") .. ": " .. usage.invokes,
    _("Last call") .. ": " .. humanizeDuration(usage.last_duration_ms),
    _("Total time") .. ": " .. humanizeDuration(usage.total_duration_ms),
    _("Disk usage") .. ": " .. humanizeBytes(usage.disk_bytes),
  }
  if usage.peak_wasm_memory_bytes > 0 then
    table.insert(parts, _("Peak wasm memory") .. ": " .. humanizeBytes(usage.peak_wasm_memory_bytes))
  end
  if usage.last_error then
    table.insert(parts, "")
    table.insert(parts, _("Last error") .. ": " .. usage.last_error)
  end

  UIManager:show(InfoMessage:new {
    text = table.concat(parts, "\n"),
  })
end

--- @private
function TaskManagerView:onReturn()
  table.remove(self.paths)

  self:onClose()
end

--- Fetches the usage of every installed source and shows the task manager.
--- @param onReturnCallback fun(): nil
function TaskManagerView:fetchAndShow(onReturnCallback)
  Trapper:wrap(function()
    local response = Backend.listInstalledSources()
    if response.type == 'ERROR' then
      ErrorDialog:show(response.message)

      return
    end

    local usages = {}
    local usage_response = Backend.getSourceUsages()
    if usage_response.type ~= 'ERROR' then
      usages = usage_response.body
    end

    local ui = TaskManagerView:new {
      installed_sources = response.body,
      usages = usages,
      on_return_callback = onReturnCallback,
      covers_fullscreen = true, -- hint for UIManager:_repaint()
    }
    UIManager:show(ui)

    Testing:emitEvent("task_manager_view_shown")
  end)
end

return TaskManagerView
