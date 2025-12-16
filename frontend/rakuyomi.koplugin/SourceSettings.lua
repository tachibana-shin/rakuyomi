local Blitbuffer = require("ffi/blitbuffer")
local FocusManager = require("ui/widget/focusmanager")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local Geom = require("ui/geometry")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local LineWidget = require("ui/widget/linewidget")
local OverlapGroup = require("ui/widget/overlapgroup")
local Screen = require("device").screen
local Size = require("ui/size")
local TextWidget = require("ui/widget/textwidget")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TitleBar = require("ui/widget/titlebar")
local UIManager = require("ui/uimanager")
local VerticalGroup = require("ui/widget/verticalgroup")

local Backend = require("Backend")
local ErrorDialog = require("ErrorDialog")
local SettingItem = require("widgets/SettingItem")

local FOOTER_FONT_SIZE = 14

--- @param setting_definition SettingDefinition
--- @return ValueDefinition
local function mapSettingDefinitionToValueDefinition(setting_definition)
  if setting_definition.type == 'switch' then
    return {
      type = 'boolean'
    }
  elseif setting_definition.type == 'select' then
    local options = {}

    for index, value in ipairs(setting_definition.values) do
      local title = value

      if setting_definition.titles ~= nil then
        title = setting_definition.titles[index]
      end

      table.insert(options, { label = title, value = value })
    end

    return {
      type = 'enum',
      title = setting_definition.title,
      options = options,
    }
  elseif setting_definition.type == 'multi-select' then
    local options = {}

    print(setting_definition.values)

    for index, value in ipairs(setting_definition.values) do
      local title = value

      if setting_definition.titles ~= nil then
        title = setting_definition.titles[index]
      end

      table.insert(options, { label = title, value = value })
    end

    return {
      type = 'multi-enum',
      title = setting_definition.title,
      options = options,
    }
  elseif setting_definition.type == 'login' then
    return {
      type = 'string',
      title = setting_definition.title,
      placeholder = 'Not support login'
    }
  elseif setting_definition.type == 'button' then
   return {
      type = 'button',
      key = setting_definition.key,
      title = setting_definition.title,
      confirm_title = setting_definition.confirmTitle,
      confirm_message = setting_definition.confirmMessage
    }
  elseif setting_definition.type == 'editable-list' then
    return {
      type = 'list',
      title = setting_definition.title,
      placeholder = setting_definition.placeholder
    }
  elseif setting_definition.type == 'text' then
    return {
      type = 'string',
      title = setting_definition.title or setting_definition.placeholder,
      placeholder = setting_definition.placeholder
    }
  elseif setting_definition.type == 'link' then
    return {
      type = 'label',
      title = setting_definition.title,
      text = setting_definition.url,
    }
  else
    error("unexpected setting definition type: " .. setting_definition.type)
  end
end

local SourceSettings = FocusManager:extend {
  source_id = nil,
  setting_definitions = nil,
  stored_settings = nil,
  -- callback to be called when pressing the back button
  on_return_callback = nil,
  paths = { 0 }
}

--- @private
function SourceSettings:init()
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

  local should_add_separator = false

  for _, setting_definition in ipairs(self.setting_definitions) do
    -- TODO This assumes 1-level groups at maximum, which is probably true for all extensions but
    -- multiple nested groups are technically possible...
    local children = {}
    if setting_definition.type == "group" then
      if setting_definition.title ~= nil then
        vertical_group[#vertical_group + 1] = TextWidget:new {
          text = setting_definition.title,
          face = Font:getFace("cfont"),
          bold = true,
        }
      end

      children = setting_definition.items
    else
      children = { setting_definition }
    end

    if should_add_separator then
      local separator = LineWidget:new {
        background = Blitbuffer.COLOR_LIGHT_GRAY,
        dimen = Geom:new {
          w = self.item_width,
          h = Size.line.thick,
        },
        style = "solid",
      }

      vertical_group[#vertical_group + 1] = separator
    end

    for _, child in ipairs(children) do
      local setting_item = SettingItem:new {
        show_parent = self,
        width = self.item_width,
        -- REFACT `text` setting definitions usually have the `placeholder` field as a replacement for
        -- `title`, however this is a implementation detail of Aidoku's extensions and it shouldn't
        -- leak here
        label = child.title or child.placeholder,
        value_definition = mapSettingDefinitionToValueDefinition(child),
        value = self.stored_settings[child.key] or child.default,
        source_id = self.source_id,
        on_value_changed_callback = function(new_value)
          self:updateStoredSetting(child.key, new_value)
        end
      }

      vertical_group[#vertical_group + 1] = setting_item
    end

    if setting_definition.type == "group" and setting_definition.footer ~= nil then
      local footer = TextBoxWidget:new {
        text = setting_definition.footer,
        face = Font:getFace("cfont", FOOTER_FONT_SIZE),
        color = Blitbuffer.COLOR_LIGHT_GRAY,
        width = self.item_width,
      }

      vertical_group[#vertical_group + 1] = footer
    end

    -- If we're inside a group, add a separator before the next setting widget
    should_add_separator = setting_definition.type == "group"
  end

  self.title_bar = TitleBar:new {
    -- TODO add source name here
    title = "Source settings",
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

  local content = OverlapGroup:new {
    allow_mirroring = false,
    dimen = self.inner_dimen:copy(),
    VerticalGroup:new {
      align = "left",
      self.title_bar,
      HorizontalGroup:new {
        HorizontalSpan:new { width = padding },
        vertical_group
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

  UIManager:setDirty(self, "ui")
end

--- @private
function SourceSettings:onClose()
  UIManager:close(self)
  if self.on_return_callback then
    self.on_return_callback()
  end
end

--- @private
function SourceSettings:onReturn()
  self:onClose()
end

--- @private
function SourceSettings:updateStoredSetting(key, new_value)
  self.stored_settings[key] = new_value

  local response = Backend.setSourceStoredSettings(self.source_id, self.stored_settings)
  if response.type == 'ERROR' then
    ErrorDialog:show(response.message)
  end
end

--- @private
function SourceSettings:fetchAndShow(source_id, on_return_callback)
  local setting_definitions_response = Backend.getSourceSettingDefinitions(source_id)
  if setting_definitions_response.type == 'ERROR' then
    ErrorDialog:show(setting_definitions_response.message)
    return
  end

  local stored_settings_response = Backend.getSourceStoredSettings(source_id)
  if stored_settings_response.type == 'ERROR' then
    ErrorDialog:show(stored_settings_response.message)
    return
  end

  local setting_definitions = setting_definitions_response.body
  local stored_settings = stored_settings_response.body

  local ui = SourceSettings:new {
    source_id = source_id,
    setting_definitions = setting_definitions,
    stored_settings = stored_settings,
    on_return_callback = on_return_callback,
  }
  ui.on_return_callback = on_return_callback
  UIManager:show(ui)
end

return SourceSettings
