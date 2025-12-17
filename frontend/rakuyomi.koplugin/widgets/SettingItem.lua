local Font = require("ui/font")
local InputContainer = require("ui/widget/container/inputcontainer")
local Screen = require("device").screen
local TextBoxWidget = require("ui/widget/textboxwidget")
local UIManager = require("ui/uimanager")

local SettingItemValue = require("widgets/SettingItemValue")
local SpacedBetweenHorizontalGroup = require("widgets/SpacedBetweenHorizontalGroup")

local SETTING_ITEM_FONT_SIZE = 18

--- @class SettingItem: { [any]: any }
--- @field value_definition ValueDefinition
local SettingItem = InputContainer:extend {
  show_parent = nil,
  width = nil,
  label = nil,
  value_definition = nil,
  value = nil,
  on_value_changed_callback = nil,
  source_id = nil,
}

function SettingItem:init()
  self.show_parent = self.show_parent or self
  self.width = self.width or Screen:getWidth()
  self.label_widget = TextBoxWidget:new {
    -- REFACT `text` setting definitions usually have the `placeholder` field as a replacement for
    -- `title`, however this is a implementation detail of Aidoku's extensions and it shouldn't
    -- leak here
    text = self.label,
    face = Font:getFace("cfont", SETTING_ITEM_FONT_SIZE),
    width = self.width / 2,
  }

  self.value_widget = SettingItemValue:new {
    show_parent = self.show_parent,
    value_definition = self.value_definition,
    max_width = self.width / 2,
    value = self.value,
    source_id = self.source_id,
    on_value_changed_callback = function(new_value)
      self:onValueChanged(new_value)
    end,
  }

  self[1] = SpacedBetweenHorizontalGroup:new {
    width = self.width,
    self.label_widget,
    self.value_widget,
  }
end

--- @private
function SettingItem:onValueChanged(new_value)
  -- The SpacedBetweenHorizontalGroup layout is cached, so we clear it
  self[1]:resetLayout()

  UIManager:setDirty(self.show_parent, "ui")

  self.on_value_changed_callback(new_value)
end

return SettingItem
