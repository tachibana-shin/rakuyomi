local Blitbuffer = require("ffi/blitbuffer")
local FocusManager = require("widgets/FocusManagerWithTopZone")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local OverlapGroup = require("ui/widget/overlapgroup")
local Screen = require("device").screen
local Size = require("ui/size")
local TitleBar = require("ui/widget/titlebar")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")
local InfoMessage = require("ui/widget/infomessage")
local _ = require("gettext+")
local Paths = require("Paths")
local Device = require("device")
local Font = require("ui/font")
local TextWidget = require("ui/widget/textwidget")
local ScrollableContainer = require("ui/widget/container/scrollablecontainer")
local MovableContainer = require("ui/widget/container/movablecontainer")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local SettingItem = require('widgets/SettingItem')
local formatBytes = require("utils/formatBytes")

local ffi = require("ffi")

ffi.cdef [[
  struct sysinfo {
      long uptime;
      unsigned long loads[3];
      unsigned long totalram;
      unsigned long freeram;
      unsigned long sharedram;
      unsigned long bufferram;
      unsigned long totalswap;
      unsigned long freeswap;
      unsigned short procs;
      unsigned short pad;
      unsigned long totalhigh;
      unsigned long freehigh;
      unsigned int mem_unit;
      char _f[20-2*sizeof(long)-sizeof(int)];
  };
  int sysinfo(struct sysinfo *info);
]]

local function get_ram_via_ffi()
  local info = ffi.new("struct sysinfo")
  if ffi.C.sysinfo(info) == 0 then
    local mem_unit = info.mem_unit > 0 and info.mem_unit or 1
    local total_bytes = tonumber(info.totalram) * mem_unit
    local free_bytes = tonumber(info.freeram) * mem_unit

    return {
      total_mb = math.floor(total_bytes / 1024 / 1024),
      free_mb = math.floor(free_bytes / 1024 / 1024)
    }
  end
  return nil
end

local function validate_proxy_url(value)
  if value == nil or value == "" then
    return true
  end
  local lower_value = value:lower()
  return lower_value:match("^https?://") ~= nil or lower_value:match("^socks5://") ~= nil
end

-- REFACT This is duplicated from `SourceSettings` (pretty much all of it actually)
local Settings = FocusManager:extend {
  settings = {},
  on_return_callback = nil,
  storage_total_text = '',
  paths = { 0 }
}

local ram_info = get_ram_via_ffi()

--- @type [string, ValueDefinition][]
Settings.setting_value_definitions = {
  {
    nil,
    { type = 'divider', title = _("Library") }
  },
  {
    'library_view_mode',
    {
      type = 'enum',
      title = _("Library view mode"),
      options = {
        { label = _("Base"),  value = "base" },
        { label = _("Cover"), value = "cover" },
        { label = _("Grid"),  value = "grid" },
      },
      default = "cover",
    }
  },
  {
    'library_sorting_mode',
    {
      type = 'enum',
      title = _("Library sorting mode"),
      options = {
        { label = _("Date added (oldest first)"),  value = 'ascending' },
        { label = _("Date added (newest first)"),  value = 'descending' },
        { label = _("Title (A-Z)"),                value = 'title_asc' },
        { label = _("Title (Z-A)"),                value = 'title_desc' },
        { label = _("Unread count (fewest first)"), value = 'unread_asc' },
        { label = _("Unread count (most first)"),  value = 'unread_desc' },
        { label = _("Last read (oldest first)"),   value = 'last_read_asc' },
        { label = _("Last read (newest first)"),   value = 'last_read_desc' },
        { label = _("Source (A-Z)"),               value = 'source_asc' },
        { label = _("Source (Z-A)"),               value = 'source_desc' },
      }
    }
  },
  {
    'rakuyomi_items_per_page',
    {
      type = 'integer',
      title = _("Items per page (0 = auto)"),
      min_value = 0,
      max_value = 100,
      is_local = true,
      default = 0
    }
  },
  {
    'rakuyomi_grid_columns',
    {
      type = 'integer',
      title = _("Grid columns"),
      min_value = 2,
      max_value = 6,
      is_local = true,
      default = 3
    }
  },
  {
    'rakuyomi_grid_rows',
    {
      type = 'integer',
      title = _("Grid rows"),
      min_value = 0,
      max_value = 6,
      is_local = true,
      default = 0
    }
  },
  {
    'rakuyomi_grid_show_title',
    {
      type = 'boolean',
      title = _("Show title in grid mode"),
      default = true,
      is_local = true,
    }
  },
  {
    'rakuyomi_grid_show_metadata',
    {
      type = 'boolean',
      title = _("Show metadata in grid mode"),
      default = true,
      is_local = true,
    }
  },
  {
    'rakuyomi_hide_read_manga',
    {
      type = 'boolean',
      title = _("Hide fully read manga"),
      default = false,
      is_local = true,
    }
  },
  {
    'rakuyomi_skip_resume_confirm',
    {
      type = 'boolean',
      title = _("Skip resume reading confirmation"),
      default = false,
      is_local = true,
    }
  },
  {
    nil,
    { type = 'divider', title = _("Search") }
  },
  {
    'search_view_mode',
    {
      type = 'enum',
      title = _("Search view mode"),
      options = {
        { label = _("Base"),  value = "base" },
        { label = _("Cover"), value = "cover" },
        { label = _("Grid"),  value = "grid" },
      },
      default = "base",
    }
  },
  {
    nil,
    { type = 'divider', title = _("Reader") }
  },
  {
    'rakuyomi_reading_direction',
    {
      type = 'enum',
      title = _("Reading direction"),
      options = {
        { label = _("Left to right"), value = "ltr" },
        { label = _("Right to left"), value = "rtl" },
      },
      default = "ltr",
      is_local = true,
    }
  },
  {
    'rakuyomi_page_turn_style',
    {
      type = 'enum',
      title = _("Page turn style"),
      options = {
        { label = _("Paginated"),         value = "paginated" },
        { label = _("Continuous scroll"), value = "scroll" },
      },
      default = "paginated",
      is_local = true,
    }
  },
  {
    'chapter_sorting_mode',
    {
      type = 'enum',
      title = _('Chapter sorting mode'),
      options = {
        { label = _("By chapter ascending"),  value = 'chapter_ascending' },
        { label = _("By chapter descending"), value = 'chapter_descending' },
      }
    }
  },
  {
    'chapter_title_format',
    {
      type = 'enum',
      title = _("Chapter title in statistics (ComicInfo)"),
      options = {
        { label = _("Chapter title only"),   value = 'title' },
        { label = _("Series + chapter title"), value = 'series_title' },
        { label = _("Series + chapter number"), value = 'series_chapter_number' },
      },
      default = 'title',
    }
  },
  {
    'preload_chapters',
    {
      type = 'integer',
      title = _("Preload chapters on reader open"),
      min_value = 0,
      max_value = 10,
      unit = 'chapters',
      default = 0
    }
  },
  {
    'optimize_image',
    {
      type = 'boolean',
      title = _("Optimize page images (experimental)"),
      default = false,
    }
  },
  {
    'concurrent_requests_pages',
    {
      type = 'integer',
      title = _("Concurrent page requests"),
      min_value = 1,
      max_value = 20,
      unit = 'pages',
      default = Device.isKindle() and 4 or 5
    }
  },
  {
    'rakuyomi_never_rtl',
    {
      type = 'boolean',
      title = _('Never turn on RTL, even for Japanese manga'),
      default = false,
      is_local = true,
    }
  },
  {
    'rakuyomi_auto_viewer_mode',
    {
      type = 'boolean',
      title = _('Automatically set the viewer to manga mode'),
      default = true,
      is_local = true,
    }
  },
  {
    'rakuyomi_global_viewer',
    {
      type = 'enum',
      title = _('Global viewer override'),
      options = {
        { label = _("Off"),      value = '' },
        { label = _("Default"),  value = 'DefaultViewer' },
        { label = _("RTL"),      value = 'Rtl' },
        { label = _("LTR"),      value = 'Ltr' },
        { label = _("Vertical"), value = 'Vertical' },
        { label = _("Scroll"),   value = 'Scroll' },
      },
      default = '',
      is_local = true,
    }
  },
  {
    'rakuyomi_hide_btn_prev',
    {
      type = 'boolean',
      title = _('Show button previous chapter in toolbar reader'),
      default = true,
      is_local = true,
    }
  },
  {
    'rakuyomi_show_btn_next',
    {
      type = 'boolean',
      title = _('Show button next chapter in toolbar reader'),
      default = true,
      is_local = true,
    }
  },
  {
    'rakuyomi_reader_extend',
    {
      type = 'enum',
      title = _('Display the expanded rakuyomi bottom toolbar in the reader'),
      options = {
        { label = _('Off'),               value = 'off' },
        { label = _('On bottom toolbar'), value = 'bottom' },
        { label = _('On top toolbar'),    value = 'top' },
      },
      default = 'bottom',
      is_local = true,
    }
  },
  {
    nil,
    { type = 'divider', title = _('Recommended reader settings') }
  },
  {
    'rakuyomi_page_margin',
    {
      type = 'boolean',
      title = _('Turn off all margins'),
      default = false,
      is_local = true,
    }
  },
  {
    'rakuyomi_trim_page',
    {
      type = 'boolean',
      title = _('Automatically crop excess edges from photos (a little slow)'),
      default = false,
      is_local = true,
    }
  },
  {
    'rakuyomi_zoom_mode_type',
    {
      type = 'boolean',
      title = _('Zoom to full screen'),
      default = false,
      is_local = true,
    }
  },
  {
    'rakuyomi_zoom_mode_genus',
    {
      type = 'boolean',
      title = _('Zoom to fit image size'),
      default = false,
      is_local = true,
    }
  },
  {
    nil,
    { type = 'divider', title = _("Storage") }
  },
  {
    'storage_path',
    {
      type = 'path',
      title = _("Chapter storage path"),
      path_type = 'directory',
      default = Paths.getHomeDirectory() .. '/downloads',
    }
  },
  {
    'storage_size_limit_mb',
    {
      type = 'integer',
      title = _('Storage size limit'),
      min_value = 1,
      max_value = 10240,
      unit = 'MB',
      default = 2000,
    }
  },
  {
    'storage_total_display',
    {
      type = 'label',
      title = _("Total downloaded"),
      text = '',
    }
  },
  {
    'delete_downloaded_on_remove',
    {
      type = 'boolean',
      title = _("Delete downloads when removing from library"),
      default = true,
    }
  },
  {
    'delete_downloaded_after_read',
    {
      type = 'boolean',
      title = _("Delete downloads after marking as read"),
      default = false,
    }
  },
  {
    'ram_storage_enabled',
    {
      type = 'boolean',
      title = _("Write chapters to RAM (protect eMMC)"),
      default = false,
    }
  },
  {
    'ram_storage_size_mb',
    {
      type = 'integer',
      title = _("RAM storage size. Your RAM is: " .. (ram_info and ram_info.total_mb or 0) .. " MB"),
      min_value = 8,
      max_value = ram_info and math.max(8, math.floor(ram_info.total_mb / 2)) or 32,
      unit = 'MB',
      default = 32,
    }
  },
  {
    nil,
    { type = 'divider', title = _("Network") }
  },
  {
    'proxy_url',
    {
      type = 'string',
      title = _("HTTP, HTTPS or SOCKS5 Proxy"),
      placeholder = 'http://user:pass@host:port',
      validate = validate_proxy_url,
      validate_error = _("Proxy URL must start with http://, https://, or socks5://"),
    }
  },
  {
    nil,
    { type = 'divider', title = _("Sync & Updates") }
  },
  {
    'api_sync',
    {
      type = 'string',
      title = _("WebDAV Sync"),
      placeholder = 'user:password@example.com/folder',
    }
  },
  {
    'enabled_cron_check_mangas_update',
    {
      type = 'boolean',
      title = _("Enabled cron check for manga updates"),
      -- default = true,
    }
  },
  {
    'source_skip_cron',
    {
      type = 'string',
      title = _("Source IDs skip check update"),
      placeholder = 'com.manga,com.manga2'
    }
  },
  {
    nil,
    { type = 'divider', title = _("System") }
  },
  {
    'allow_commaneer_filemanager',
    {
      type = 'boolean',
      title = _("Allow requisition of the back button"),
      is_local = true,
      default = true
    }
  },
  {
    nil,
    { type = 'divider', title = _("Server") }
  },
  {
    'rakuyomi_auto_kill_server_delay',
    {
      type = 'enum',
      title = _("Auto-stop server when leaving library view"),
      options = {
        { label = _("Disabled"),         value = "disabled" },
        { label = _("Immediate"),        value = "immediate" },
        { label = _("After 30 seconds"), value = "30" },
        { label = _("After 1 minute"),   value = "60" },
        { label = _("After 5 minutes"),  value = "300" },
        { label = _("After 10 minutes"), value = "600" },
      },
      is_local = true,
      default = "disabled",
    }
  },
  {
    'rakuyomi_show_download_progress',
    {
      type = 'boolean',
      title = _("Show chapter download progress"),
      is_local = true,
      default = true
    }
  },
  {
    nil,
    { type = 'divider', title = _("Logging") }
  },
  {
    'rakuyomi_disable_logging',
    {
      type = 'boolean',
      title = _("Disable logging"),
      default = false,
      is_local = true,
    }
  },
}


--- @private
function Settings:init()
  self.dimen = Geom:new {
    x = 0,
    y = 0,
    w = self.width or Screen:getWidth(),
    h = self.height or Screen:getHeight(),
  }

  if self.dimen.w == Screen:getWidth() and self.dimen.h == Screen:getHeight() then
    self.covers_fullscreen = true -- hint for UIManager:_repaint()
  end

  local border_size = Size.border.window
  local padding = Size.padding.large

  self.inner_dimen = Geom:new {
    w = self.dimen.w - 2 * border_size,
    h = self.dimen.h - 2 * border_size,
  }

  self.item_width = self.inner_dimen.w - 2 * padding

  local vertical_group = VerticalGroup:new {
    align = "left",
  }

  for __, tuple in ipairs(Settings.setting_value_definitions) do
    local key = tuple[1]
    local definition = tuple[2]
    if definition.type == 'divider' then
      table.insert(vertical_group, TextWidget:new {
        text = definition.title,
        face = Font:getFace("cfont"),
        bold = true,
      })
    elseif definition.type == 'label' then
      local text = definition.text
      if key == 'storage_total_display' then
        text = self.storage_total_text ~= '' and self.storage_total_text or _("Unknown")
      end

      table.insert(vertical_group, SettingItem:new {
        show_parent = self,
        width = self.item_width,
        label = definition.title,
        value_definition = {
          type = 'label',
          title = definition.title,
          text = text,
        },
        value = nil,
        on_value_changed_callback = function() end,
      })
    elseif definition.is_local then
      table.insert(vertical_group, SettingItem:new {
        show_parent = self,
        width = self.item_width,
        label = definition.title,
        value_definition = definition,
        value = G_reader_settings:readSetting(key, definition.default),
        on_value_changed_callback = function(new_value)
          G_reader_settings:saveSetting(key, new_value)
        end
      })
    else
      -- FIXME shouldn't the backend return the default value when unset?
      local value = self.settings[key]
      if value == nil and definition.default ~= nil then
        value = definition.default
        self.settings[key] = value
      elseif value == nil and definition.type == 'boolean' then
        value = false
        self.settings[key] = value
      end
      if key == 'storage_path' and value == nil then
        value = Paths.getHomeDirectory() .. '/downloads'
        self.settings[key] = value
      end

      table.insert(vertical_group, SettingItem:new {
        show_parent = self,
        width = self.item_width,
        label = definition.title,
        value_definition = definition,
        value = value,
        on_value_changed_callback = function(new_value)
          return self:updateSetting(key, new_value)
        end
      })
    end
  end

  self.title_bar = TitleBar:new {
    title = _("Settings"),
    fullscreen = true,
    width = self.dimen.w,
    with_bottom_line = true,
    bottom_line_color = Blitbuffer.COLOR_DARK_GRAY,
    bottom_line_h_padding = padding,
    left_icon = "chevron.left",
    left_icon_tap_callback = function()
      self:onReturn()
    end,
    close_callback = function()
      self:onClose()
    end,
  }

  local scrollable = ScrollableContainer:new {
    dimen = Geom:new {
      w = self.dimen.w,
      h = self.dimen.h - self.title_bar.dimen.h,
    },
    vertical_group,
  }
  local content = OverlapGroup:new {
    allow_mirroring = false,
    dimen = self.inner_dimen:copy(),
    VerticalGroup:new {
      align = "left",
      self.title_bar,
      HorizontalGroup:new {
        HorizontalSpan:new { width = padding },
        scrollable
      }
    }
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
    content
  }

  self.movable = MovableContainer:new {
    self[1],
    unmovable = self.unmovable,
  }
  scrollable.show_parent = self


  UIManager:setDirty(self, "ui")
end

--- @private
function Settings:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

--- @private
function Settings:onReturn()
  self:onClose()
end

--- @private
function Settings:updateSetting(key, value)
  -- fallback control ram_storage_enabled, ram_storage_size_mb
  if key == 'ram_storage_enabled' or key == 'ram_storage_size_mb' then
    local enabled = key == 'ram_storage_enabled' and value or self.settings.ram_storage_enabled
    local ram_storage_size_mb = key == 'ram_storage_size_mb' and value or self.settings.ram_storage_size_mb

    local response = Backend.mountFS({
      enabled = enabled,
      ram_storage_size_mb = ram_storage_size_mb,
    })

    if response.type == 'ERROR' then
      if key == 'ram_storage_enabled' then
        self.settings.ram_storage_enabled = false
      end
      ErrorDialog:show(response.message)
      return false
    end

    self.settings[key] = value

    return
  end

  -- Test proxy before saving
  if key == 'proxy_url' and value ~= nil and value ~= '' then
    local test_response = Backend.testProxy(value)
    if test_response.type == 'ERROR' then
      ErrorDialog:show(test_response.message or _("Failed to test proxy"))
      return false
    end
  end

  self.settings[key] = value

  local response = Backend.setSettings(self.settings)
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)
    return
  end

  if key == "enabled_cron_check_mangas_update" or key == "source_skip_cron" then
    UIManager:show(InfoMessage:new {
      text = "You'll need to restart the app for this change to take effect"
    })
  end
end

--- @param on_return_callback function|nil Called when the settings screen is closed.
--- @return boolean # true when the settings screen was shown
function Settings:fetchAndShow(on_return_callback)
  local response = Backend.getSettings()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)
    return false
  end

  local storage_total_text = ''
  local stats_ok, stats_result = pcall(function()
    local stats_response = Backend.getStorageStats()
    if stats_response.type == 'SUCCESS' and stats_response.body then
      return formatBytes(stats_response.body.total_bytes)
    end
    return ''
  end)
  if stats_ok and stats_result then
    storage_total_text = stats_result
  end

  local ok, ui = pcall(function()
    return Settings:new {
      settings = response.body,
      storage_total_text = storage_total_text,
      on_return_callback = on_return_callback,
    }
  end)

  if not ok then
    ErrorDialog:show(tostring(ui))
    return false
  end

  ui.on_return_callback = on_return_callback
  UIManager:show(ui)

  return true
end

return Settings
