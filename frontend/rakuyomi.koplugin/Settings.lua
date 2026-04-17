local Blitbuffer = require("ffi/blitbuffer")
local FocusManager = require("ui/widget/focusmanager")
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

-- REFACT This is duplicated from `SourceSettings` (pretty much all of it actually)
local Settings = FocusManager:extend {
  settings = {},
  on_return_callback = nil,
  paths = { 0 }
}


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
        { label = _("Order added ascending (Default)"),  value = 'ascending' },
        { label = _("Order added descending"),           value = 'descending' },
        { label = _("Title manga ascending"),            value = 'title_asc' },
        { label = _("Title manga descending"),           value = 'title_desc' },
        { label = _("Count unread chapters ascending"),  value = 'unread_asc' },
        { label = _("Count unread chapters descending"), value = 'unread_desc' },
        { label = _("Last read ascending"),              value = 'last_read_asc' },
        { label = _("Last read descending"),             value = 'last_read_desc' },
        { label = _("Source ascending"),                 value = 'source_asc' },
        { label = _("Source descending"),                value = 'source_desc' },
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
    nil,
    { type = 'divider', title = _("Search") }
  },
  {
    'rakuyomi_search_view_mode',
    {
      type = 'enum',
      title = _("Search view mode"),
      options = {
        { label = _("Base"),  value = "base" },
        { label = _("Cover"), value = "cover" },
        { label = _("Grid"),  value = "grid" },
      },
      is_local = true,
      default = "base",
    }
  },
  {
    nil,
    { type = 'divider', title = _("Reader") }
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
      unit = 'MB'
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

  for _, tuple in ipairs(Settings.setting_value_definitions) do
    local key = tuple[1]
    local definition = tuple[2]
    if definition.type == 'divider' then
      table.insert(vertical_group, TextWidget:new {
        text = definition.title,
        face = Font:getFace("cfont"),
        bold = true,
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
      if key == 'storage_path' and value == nil then
        value = Paths.getHomeDirectory() .. '/downloads'
      end

      table.insert(vertical_group, SettingItem:new {
        show_parent = self,
        width = self.item_width,
        label = definition.title,
        value_definition = definition,
        value = value,
        on_value_changed_callback = function(new_value)
          self:updateSetting(key, new_value)
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

function Settings:fetchAndShow(on_return_callback)
  local response = Backend.getSettings()
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)
  end

  local ui = Settings:new {
    settings = response.body,
    on_return_callback = on_return_callback
  }
  ui.on_return_callback = on_return_callback
  UIManager:show(ui)
end

return Settings
