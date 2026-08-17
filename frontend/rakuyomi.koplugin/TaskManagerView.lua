local Blitbuffer = require("ffi/blitbuffer")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local GestureRange = require("ui/gesturerange")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local InfoMessage = require("ui/widget/infomessage")
local InputContainer = require("ui/widget/container/inputcontainer")
local LineWidget = require("ui/widget/linewidget")
local OverlapGroup = require("ui/widget/overlapgroup")
local Screen = require("device").screen
local ScrollableContainer = require("ui/widget/container/scrollablecontainer")
local Size = require("ui/size")
local SortWidget = require("ui/widget/sortwidget")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TitleBar = require("ui/widget/titlebar")
local Trapper = require("ui/trapper")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local _ = require("gettext+")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local Testing = require("testing")
local TopZoneHandler = require("widgets/TopZoneHandler")
local formatBytes = require("utils/formatBytes")

--- The columns of the table, left to right. The `weight` fields are used to
--- split the available width; the name column takes whatever is left. The
--- display order and visibility are user-configurable (task manager
--- settings), persisted in `G_reader_settings`.
local COLUMNS = {
  { key = "name",    label = _("Name"),       align = "left",  weight = 0.30 },
  { key = "invokes", label = _("Calls"),      align = "right", weight = 0.14 },
  { key = "total",   label = _("Total time"), align = "right", weight = 0.16 },
  { key = "last",    label = _("Last call"),  align = "right", weight = 0.14 },
  { key = "disk",    label = _("Disk"),       align = "right", weight = 0.13 },
  { key = "memory",  label = _("Memory"),     align = "right", weight = 0.13 },
}

local REFRESH_INTERVALS = { 3, 5, 10, 30 }

local SORT_ASC = " \u{25B2}"
local SORT_DESC = " \u{25BC}"

local SETTINGS_PREFIX = "rakuyomi_task_manager_"
local KEY_COLUMN_ORDER = SETTINGS_PREFIX .. "column_order"
local KEY_HIDDEN_COLUMNS = SETTINGS_PREFIX .. "hidden_columns"
local KEY_AUTO_REFRESH = SETTINGS_PREFIX .. "auto_refresh"
local KEY_REFRESH_INTERVAL = SETTINGS_PREFIX .. "refresh_interval"
local KEY_SORT_COLUMN = SETTINGS_PREFIX .. "sort_column"
local KEY_SORT_ASCENDING = SETTINGS_PREFIX .. "sort_ascending"

--- @class TaskManagerView: { [any]: any }
--- @field installed_sources SourceInformation[]
--- @field usages table<string, SourceUsage>
--- @field on_return_callback fun(): nil
--- @field sort_column string
--- @field sort_ascending boolean
--- @field column_order string[]
--- @field hidden_columns table<string, boolean>
--- @field parent any
local TaskManagerView = InputContainer:extend {
  name = "task_manager_view",
  covers_fullscreen = true, -- hint for UIManager:_repaint()
  title = _("Task manager"),

  installed_sources = nil,
  usages = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
  sort_column = "memory",
  sort_ascending = false,
  auto_refresh = false,
  refresh_interval_s = 5,

  key_events = {
    Exit = { { "Back" } },
  },
}

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

--- Line height of a font face in pixels, matching the formula
--- TextBoxWidget uses (`(1 + line_height) * face.size` with the default
--- line height of 0.3 em).
--- @private
local function lineHeightPx(face)
  return math.floor((1 + 0.3) * face.size + 0.5)
end

--- @private
local function columnByKey(key)
  for _, column in ipairs(COLUMNS) do
    if column.key == key then
      return column
    end
  end

  return nil
end

function TaskManagerView:init()
  self.installed_sources = self.installed_sources or {}
  self.usages = self.usages or {}
  self:loadColumnPrefs()
  self.auto_refresh = G_reader_settings:readSetting(KEY_AUTO_REFRESH, true)
  self.refresh_interval_s = G_reader_settings:readSetting(KEY_REFRESH_INTERVAL, 5)
  -- Persisted sort, defaulting to memory descending (heaviest first).
  local sort_column = G_reader_settings:readSetting(KEY_SORT_COLUMN, "memory")
  if columnByKey(sort_column) then
    self.sort_column = sort_column
  end
  self.sort_ascending = G_reader_settings:readSetting(KEY_SORT_ASCENDING, false)

  self.dimen = Geom:new {
    x = 0,
    y = 0,
    w = Screen:getWidth(),
    h = Screen:getHeight(),
  }

  -- The sortable header sits right below the title bar, so the tap zone
  -- must not cover it (0.06 * screen height stays above the header on every
  -- device; the header taps are consumed by the header cells anyway).
  TopZoneHandler.enableTopZoneHandler(self, 0.06)

  local border_size = Size.border.window
  local padding = Size.padding.large

  self.inner_dimen = Geom:new {
    w = self.dimen.w - 2 * border_size,
    h = self.dimen.h - 2 * border_size,
  }
  self.content_width = self.inner_dimen.w - 2 * padding
  self:computeColumnWidths()

  self.header_face = Font:getFace("tfont", 18)
  self.row_face = Font:getFace("ffont", 18)
  self.header_height = lineHeightPx(self.header_face)
  self.row_height = lineHeightPx(self.row_face)

  local title_bar = TitleBar:new {
    width = self.dimen.w,
    title = self.title,
    left_icon = "chevron.left",
    left_icon_tap_callback = function()
      self:onReturn()
    end,
    right_icon = "appbar.settings",
    right_icon_tap_callback = function()
      self:showSettings()
    end,
  }
  self.title_bar = title_bar

  self.scrollable = ScrollableContainer:new {
    dimen = Geom:new {
      w = self.dimen.w,
      h = self.dimen.h - title_bar.dimen.h,
    },
    ScrollThroughPages = true,
  }
  self.table_group = VerticalGroup:new {
    align = "left",
  }
  self:rebuildTable()

  local content = OverlapGroup:new {
    allow_mirroring = false,
    dimen = self.inner_dimen:copy(),
    VerticalGroup:new {
      align = "left",
      title_bar,
      HorizontalGroup:new {
        HorizontalSpan:new { width = padding },
        self.table_group,
      },
    },
  }

  self[1] = FrameContainer:new {
    show_parent = self,
    width = self.dimen.w,
    height = self.dimen.h,
    padding = 0,
    margin = 0,
    bordersize = border_size,
    focusable = true,
    background = Blitbuffer.COLOR_WHITE,
    content,
  }

  self:scheduleRefresh()

  UIManager:setDirty(self, "ui")
end

--- @private
function TaskManagerView:onClose()
  UIManager:close(self)
end

--- @private
function TaskManagerView:onCloseWidget()
  self:unscheduleRefresh()
  if not self._return_called then
    self._return_called = true
    if self.on_return_callback then
      self.on_return_callback()
    end
  end

  return true
end

--- @private
function TaskManagerView:onReturn()
  self:onClose()
end

--- @private
function TaskManagerView:onExit()
  self:onClose()
end

--- Loads the persisted column order and visibility. Unknown keys are
--- dropped and missing ones appended, so the saved prefs survive column
--- list changes across versions.
--- @private
function TaskManagerView:loadColumnPrefs()
  local order = G_reader_settings:readSetting(KEY_COLUMN_ORDER)
  local hidden = G_reader_settings:readSetting(KEY_HIDDEN_COLUMNS) or {}
  self.column_order = {}
  self.hidden_columns = {}
  for _, key in ipairs(hidden) do
    self.hidden_columns[key] = true
  end
  local seen = {}
  if order then
    for _, key in ipairs(order) do
      if columnByKey(key) and not seen[key] then
        table.insert(self.column_order, key)
        seen[key] = true
      end
    end
  end
  for _, column in ipairs(COLUMNS) do
    if not seen[column.key] then
      table.insert(self.column_order, column.key)
    end
  end
end

--- @private
function TaskManagerView:saveColumnPrefs()
  G_reader_settings:saveSetting(KEY_COLUMN_ORDER, self.column_order)
  local hidden = {}
  for key, is_hidden in pairs(self.hidden_columns) do
    if is_hidden then
      table.insert(hidden, key)
    end
  end
  G_reader_settings:saveSetting(KEY_HIDDEN_COLUMNS, hidden)
end

--- The columns currently displayed, in display order.
--- @private
function TaskManagerView:visibleColumns()
  local visible = {}
  for _, key in ipairs(self.column_order) do
    if not self.hidden_columns[key] then
      table.insert(visible, columnByKey(key))
    end
  end

  return visible
end

--- @private
function TaskManagerView:countVisibleColumns()
  local count = 0
  for _, key in ipairs(self.column_order) do
    if not self.hidden_columns[key] then
      count = count + 1
    end
  end

  return count
end

--- Splits the available width between the visible columns, the name column
--- getting whatever the others leave behind.
--- @private
function TaskManagerView:computeColumnWidths()
  self.column_widths = {}
  local used = 0
  for _, column in ipairs(self:visibleColumns()) do
    if column.key ~= "name" then
      local width = math.floor(self.content_width * column.weight)
      self.column_widths[column.key] = width
      used = used + width
    end
  end
  self.column_widths.name = self.content_width - used
end

--- @private
function TaskManagerView:getItems()
  local items = {}
  for _, source_information in ipairs(self.installed_sources) do
    table.insert(items, {
      source_information = source_information,
      usage = self.usages[source_information.id],
    })
  end

  return items
end

--- @private
function TaskManagerView:sortValue(item, column_key)
  local usage = item.usage
  if column_key == "name" then
    return item.source_information.name
  end
  if not usage then
    return 0
  end
  if column_key == "invokes" then
    return usage.invokes
  elseif column_key == "total" then
    return usage.total_duration_ms
  elseif column_key == "last" then
    return usage.last_duration_ms
  elseif column_key == "disk" then
    return usage.disk_bytes
  elseif column_key == "memory" then
    return usage.peak_wasm_memory_bytes
  end

  return 0
end

--- @private
function TaskManagerView:getSortedItems()
  local items = self:getItems()
  local column_key = self.sort_column
  local ascending = self.sort_ascending
  table.sort(items, function(a, b)
    -- keep sources without usage at the bottom of numeric sorts
    if column_key ~= "name" and (a.usage == nil) ~= (b.usage == nil) then
      return a.usage ~= nil
    end
    local va = self:sortValue(a, column_key)
    local vb = self:sortValue(b, column_key)
    if va == vb then
      return a.source_information.name < b.source_information.name
    end
    if ascending then
      return va < vb
    end

    return va > vb
  end)

  return items
end

--- @private
function TaskManagerView:cellText(item, column)
  local source_information = item.source_information
  local usage = item.usage
  if column.key == "name" then
    return source_information.name
  end
  if not usage then
    return "-"
  end
  if column.key == "invokes" then
    return tostring(usage.invokes)
  elseif column.key == "total" then
    return humanizeDuration(usage.total_duration_ms)
  elseif column.key == "last" then
    return humanizeDuration(usage.last_duration_ms)
  elseif column.key == "disk" then
    if usage.disk_bytes > 0 then
      return formatBytes(usage.disk_bytes)
    end

    return "-"
  elseif column.key == "memory" then
    if usage.peak_wasm_memory_bytes > 0 then
      return formatBytes(usage.peak_wasm_memory_bytes)
    end

    return "-"
  end

  return ""
end

--- @private
function TaskManagerView:buildCell(text, width, height, face, align, on_tap)
  local cell = InputContainer:new {
    dimen = Geom:new { w = width, h = height },
    show_parent = self,
    TextBoxWidget:new {
      text = text,
      face = face,
      width = width,
      height = height,
      alignment = align,
      height_overflow_show_ellipsis = true,
    },
  }
  if on_tap then
    -- The tap range references `cell.dimen`: InputContainer:paintTo keeps it
    -- in sync with the widget's absolute position on screen.
    cell.ges_events = {
      TapTap = {
        GestureRange:new {
          ges = "tap",
          range = cell.dimen,
        },
      },
    }
    cell.onTapTap = on_tap
  end

  return cell
end

--- @private
function TaskManagerView:buildHeaderRow()
  local cells = {}
  for _, column in ipairs(self:visibleColumns()) do
    local label = column.label
    if column.key == self.sort_column then
      label = label .. (self.sort_ascending and SORT_ASC or SORT_DESC)
    end
    local key = column.key
    table.insert(cells, self:buildCell(
      label,
      self.column_widths[column.key],
      self.header_height,
      self.header_face,
      column.align,
      function()
        self:onHeaderTap(key)
      end
    ))
  end

  return HorizontalGroup:new {
    align = "top",
    ---@diagnostic disable-next-line: deprecated
    unpack(cells),
  }
end

--- @private
function TaskManagerView:onHeaderTap(column_key)
  if column_key == self.sort_column then
    self.sort_ascending = not self.sort_ascending
  else
    self.sort_column = column_key
    self.sort_ascending = true
  end
  G_reader_settings:saveSetting(KEY_SORT_COLUMN, self.sort_column)
  G_reader_settings:saveSetting(KEY_SORT_ASCENDING, self.sort_ascending)

  self:rebuildTable()
  UIManager:setDirty(self, "ui")

  return true
end

--- @private
function TaskManagerView:onRowTap(item)
  if not item.usage then
    return true
  end

  local usage = item.usage
  local parts = {
    item.source_information.name,
    "",
    _("Calls") .. ": " .. usage.invokes,
    _("Last call") .. ": " .. humanizeDuration(usage.last_duration_ms),
    _("Total time") .. ": " .. humanizeDuration(usage.total_duration_ms),
    _("Disk usage") .. ": " .. formatBytes(usage.disk_bytes),
  }
  if usage.peak_wasm_memory_bytes > 0 then
    table.insert(parts, _("Peak wasm memory") .. ": " .. formatBytes(usage.peak_wasm_memory_bytes))
  end
  if usage.last_error then
    table.insert(parts, "")
    table.insert(parts, _("Last error") .. ": " .. usage.last_error)
  end

  UIManager:show(InfoMessage:new {
    text = table.concat(parts, "\n"),
  })

  return true
end

--- @private
function TaskManagerView:buildRow(item)
  local cells = {}
  for _, column in ipairs(self:visibleColumns()) do
    table.insert(cells, self:buildCell(
      self:cellText(item, column),
      self.column_widths[column.key],
      self.row_height,
      self.row_face,
      column.align,
      nil
    ))
  end

  local row = HorizontalGroup:new {
    align = "top",
    ---@diagnostic disable-next-line: deprecated
    unpack(cells),
  }

  local wrapper = InputContainer:new {
    dimen = Geom:new { w = self.content_width, h = self.row_height },
    show_parent = self,
    row,
  }
  wrapper.ges_events = {
    TapTap = {
      GestureRange:new {
        ges = "tap",
        range = wrapper.dimen,
      },
    },
  }
  wrapper.onTapTap = function()
    return self:onRowTap(item)
  end

  return wrapper
end

--- @private
function TaskManagerView:rebuildTable()
  local rows = VerticalGroup:new {
    align = "left",
  }

  local items = self:getSortedItems()
  if #items == 0 then
    table.insert(rows, self:buildCell(
      _("No installed sources found"),
      self.content_width,
      self.row_height,
      self.row_face,
      "left",
      nil
    ))
  else
    for _, item in ipairs(items) do
      table.insert(rows, self:buildRow(item))
    end
  end

  self.scrollable[1] = rows
  self.scrollable:reset()

  -- The header stays fixed above the scrollable, Windows task manager
  -- style; only the rows scroll away.
  self.table_group[1] = self:buildHeaderRow()
  self.table_group[2] = VerticalSpan:new { width = Size.span.vertical_small }
  self.table_group[3] = LineWidget:new {
    dimen = Geom:new { w = self.content_width, h = 1 },
  }
  self.table_group[4] = self.scrollable
end

--- @private
function TaskManagerView:onToggleColumnVisibility(column_key)
  if self.hidden_columns[column_key] then
    self.hidden_columns[column_key] = nil
  else
    if self:countVisibleColumns() <= 1 then
      return
    end
    self.hidden_columns[column_key] = true
    if self.sort_column == column_key then
      self.sort_column = "name"
    end
  end

  self:saveColumnPrefs()
  self:computeColumnWidths()
  self:rebuildTable()
  UIManager:setDirty(self, "ui")
end

--- @private
function TaskManagerView:onMoveColumn(column_key, direction)
  for i, key in ipairs(self.column_order) do
    if key == column_key then
      local j = i + direction
      if j < 1 or j > #self.column_order then
        return
      end
      self.column_order[i], self.column_order[j] = self.column_order[j], self.column_order[i]
      break
    end
  end

  self:saveColumnPrefs()
  self:computeColumnWidths()
  self:rebuildTable()
  UIManager:setDirty(self, "ui")
end

--- @private
function TaskManagerView:onToggleAutoRefresh()
  self.auto_refresh = not self.auto_refresh
  G_reader_settings:saveSetting(KEY_AUTO_REFRESH, self.auto_refresh)
  if self.auto_refresh then
    self:scheduleRefresh()
  else
    self:unscheduleRefresh()
  end
end

--- @private
function TaskManagerView:onSetRefreshInterval(interval)
  self.refresh_interval_s = interval
  G_reader_settings:saveSetting(KEY_REFRESH_INTERVAL, interval)
  self:unscheduleRefresh()
  self:scheduleRefresh()
end

--- @private
function TaskManagerView:onResetColumns()
  self.column_order = {}
  for _, column in ipairs(COLUMNS) do
    table.insert(self.column_order, column.key)
  end
  self.hidden_columns = {}
  G_reader_settings:delSetting(KEY_COLUMN_ORDER)
  G_reader_settings:delSetting(KEY_HIDDEN_COLUMNS)
  self:computeColumnWidths()
  self:rebuildTable()
  UIManager:setDirty(self, "ui")
end

--- Settings screen of the task manager: column visibility/order, auto
--- refresh, refresh interval and column reset. A dedicated view instead of
--- a popup menu, following the rakuyomi view conventions.
--- @class TaskManagerSettingsView: { [any]: any }
--- @field parent TaskManagerView
local TaskManagerSettingsView = InputContainer:extend {
  name = "task_manager_settings_view",
  covers_fullscreen = true,
  title = _("Task manager settings"),

  parent = nil,

  key_events = {
    Exit = { { "Back" } },
  },
}

function TaskManagerSettingsView:init()
  self.dimen = Geom:new {
    x = 0,
    y = 0,
    w = Screen:getWidth(),
    h = Screen:getHeight(),
  }

  local border_size = Size.border.window
  local padding = Size.padding.large
  self.inner_dimen = Geom:new {
    w = self.dimen.w - 2 * border_size,
    h = self.dimen.h - 2 * border_size,
  }
  self.content_width = self.inner_dimen.w - 2 * padding
  self.row_face = Font:getFace("ffont", 20)
  self.section_face = Font:getFace("tfont", 20)
  self.row_height = lineHeightPx(self.row_face) + 2 * Size.padding.small

  local title_bar = TitleBar:new {
    width = self.dimen.w,
    title = self.title,
    fullscreen = true,
    with_bottom_line = true,
    bottom_line_color = Blitbuffer.COLOR_DARK_GRAY,
    bottom_line_h_padding = padding,
    left_icon = "chevron.left",
    left_icon_tap_callback = function()
      self:onClose()
    end,
    close_callback = function()
      self:onClose()
    end,
  }
  self.title_bar = title_bar

  self.scrollable = ScrollableContainer:new {
    dimen = Geom:new {
      w = self.dimen.w,
      h = self.dimen.h - title_bar.dimen.h,
    },
    ScrollThroughPages = true,
  }
  self:rebuildRows()

  self[1] = FrameContainer:new {
    show_parent = self,
    width = self.dimen.w,
    height = self.dimen.h,
    padding = 0,
    margin = 0,
    bordersize = border_size,
    focusable = true,
    background = Blitbuffer.COLOR_WHITE,
    VerticalGroup:new {
      align = "left",
      title_bar,
      HorizontalGroup:new {
        HorizontalSpan:new { width = padding },
        self.scrollable,
      },
    },
  }

  UIManager:setDirty(self, "ui")
end

--- @private
function TaskManagerSettingsView:onClose()
  UIManager:close(self)
end

--- @private
function TaskManagerSettingsView:onCloseWidget()
  return true
end

--- @private
function TaskManagerSettingsView:onExit()
  self:onClose()
end

--- @private
function TaskManagerSettingsView:sectionLabel(text, top_gap)
  return VerticalGroup:new {
    align = "left",
    VerticalSpan:new { width = top_gap or 0 },
    TextBoxWidget:new {
      text = text,
      face = self.section_face,
      width = self.content_width,
      height = self.row_height,
      alignment = "left",
      height_overflow_show_ellipsis = true,
    },
  }
end

--- @private
function TaskManagerSettingsView:buildTapCell(text, width, align, callback)
  local row_pad = Size.padding.small
  local cell = InputContainer:new {
    dimen = Geom:new { w = width, h = self.row_height },
    show_parent = self,
    FrameContainer:new {
      padding = row_pad,
      bordersize = 0,
      TextBoxWidget:new {
        text = text,
        face = self.row_face,
        width = width - 2 * row_pad,
        height = self.row_height - 2 * row_pad,
        alignment = align,
        height_overflow_show_ellipsis = true,
      },
    },
  }
  if callback then
    cell.ges_events = {
      TapTap = {
        GestureRange:new {
          ges = "tap",
          range = cell.dimen,
        },
      },
    }
    cell.onTapTap = function()
      callback()
      return true
    end
  end

  return cell
end

--- @private
function TaskManagerSettingsView:buildRow(text, callback)
  return self:buildTapCell(text, self.content_width, "left", callback)
end

--- Row with "move left"/"move right" buttons around the column label.
--- @private
--- Opens KOReader's SortWidget for reordering the columns. The checkmark
--- on each item toggles the column visibility, the buttons on the footer
--- move the marked column around; Cancel restores the previous order.
--- @private
function TaskManagerSettingsView:onColumnOrder()
  ---@type any
  local parent = self.parent
  local item_table = {}
  for _, key in ipairs(parent.column_order) do
    local key_local = key
    local item = {
      key = key,
      text = columnByKey(key).label,
      dim = parent.hidden_columns[key] and true or nil,
      checked_func = function()
        return not parent.hidden_columns[key_local]
      end,
    }
    item.callback = function()
      parent:onToggleColumnVisibility(key_local)
      item.dim = parent.hidden_columns[key_local] and true or nil
    end
    table.insert(item_table, item)
  end
  local sort_widget = SortWidget:new {
    title = _("Column order"),
    item_table = item_table,
    callback = function()
      local new_order = {}
      for _, item in ipairs(item_table) do
        table.insert(new_order, item.key)
      end
      parent.column_order = new_order
      parent:saveColumnPrefs()
      parent:computeColumnWidths()
      parent:rebuildTable()
      UIManager:setDirty(parent, "ui")
    end,
  }
  UIManager:show(sort_widget)
end

--- @private
function TaskManagerSettingsView:rebuildRows()
  ---@type any
  local parent = self.parent
  local rows = VerticalGroup:new {
    align = "left",
  }
  local section_gap = Size.padding.small

  table.insert(rows, self:buildRow(_("Column order & visibility"), function()
    self:onColumnOrder()
  end))

  table.insert(rows, self:sectionLabel(_("Auto refresh"), section_gap))
  table.insert(rows, self:buildRow(
    (parent.auto_refresh and "\u{2713} " or "   ") .. _("Auto refresh"),
    function()
      parent:onToggleAutoRefresh()
      self:rebuildRows()
      UIManager:setDirty(self, "ui")
    end
  ))

  table.insert(rows, self:sectionLabel(_("Refresh interval"), section_gap))
  for __, interval in ipairs(REFRESH_INTERVALS) do
    local interval_local = interval
    table.insert(rows, self:buildRow(
      (parent.refresh_interval_s == interval and "\u{2713} " or "   ") .. string.format(_("%d seconds"), interval),
      function()
        parent:onSetRefreshInterval(interval_local)
        self:rebuildRows()
        UIManager:setDirty(self, "ui")
      end
    ))
  end

  table.insert(rows, self:buildRow(_("Reset columns"), function()
    parent:onResetColumns()
    self:rebuildRows()
    UIManager:setDirty(self, "ui")
  end))

  self.scrollable[1] = rows
  self.scrollable:reset()
end

--- @private
function TaskManagerView:scheduleRefresh()
  if not self.auto_refresh or self.refresh_timer then
    return
  end
  local function on_refresh()
    self.refresh_timer = nil
    self:onRefreshTick()
  end
  -- UIManager:scheduleIn expects seconds and does not return a handle, so
  -- keep the callback itself and unschedule with the same reference.
  self.refresh_timer = on_refresh
  UIManager:scheduleIn(self.refresh_interval_s, on_refresh)
end

--- @private
function TaskManagerView:unscheduleRefresh()
  if self.refresh_timer then
    UIManager:unschedule(self.refresh_timer)
    self.refresh_timer = nil
  end
end

--- @private
function TaskManagerView:onRefreshTick()
  if not self.auto_refresh then
    return
  end
  local usage_response = Backend.getSourceUsages()
  if usage_response.type ~= 'ERROR' then
    self.usages = usage_response.body
    self:rebuildTable()
    UIManager:setDirty(self, "ui")
  end
  self:scheduleRefresh()
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
    }
    UIManager:show(ui)

    Testing:emitEvent("task_manager_view_shown")
  end)
end

function TaskManagerView:showSettings()
  UIManager:show(TaskManagerSettingsView:new { parent = self })
end

return TaskManagerView
