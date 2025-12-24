local Device = require("device")
local Geom = require("ui/geometry")

local InfoMessage = require("ui/widget/infomessage")
local Font = require("ui/font")
local GestureRange = require("ui/gesturerange")
local Blitbuffer = require("ffi/blitbuffer")
local Size = require("ui/size")
local FrameContainer = require("ui/widget/container/framecontainer")
local CenterContainer = require("ui/widget/container/centercontainer")
local MovableContainer = require("ui/widget/container/movablecontainer")
local ScrollableContainer = require("ui/widget/container/scrollablecontainer")
local TextWidget = require("ui/widget/textwidget")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local VerticalGroup = require("ui/widget/verticalgroup")

local Screen = require("device").screen

local Input = Device.input

local DGENERIC_ICON_SIZE = G_defaults:readSetting("DGENERIC_ICON_SIZE")

--- @class CustomDialog
--- @field navbar_height number
--- @field title string
--- @field options any[]
--- @field padding number
--- @field generate fun(option: any, max_width: number, index: number): any
--- @field unmovable boolean|nil
--- @field key_events any
--- @field ges_events any
--- @field extend fun(any): CustomDialog
local CustomDialog = InfoMessage:extend {
  navbar_height = Screen:scaleBySize(1),
  title = nil,
  options = nil,
  padding = 16,
}

function CustomDialog:init(sel)
  if sel ~= nil then
    self = sel
  end

  if not self.face then
    self.face = Font:getFace("infofont")
  end

  local right_icon_size = Screen:scaleBySize(DGENERIC_ICON_SIZE * 0.6)
  local button_padding = Screen:scaleBySize(11)

  if Device:hasKeys() then
    self.key_events.AnyKeyPressed = { { Input.group.Any } }
  end
  if Device:isTouchDevice() then
    self.ges_events.TapClose = {
      GestureRange:new {
        ges = "tap",
        range = Geom:new {
          x = 0, y = 0,
          w = Screen:getWidth(),
          h = Screen:getHeight(),
        }
      }
    }
  end

  local navbar = HorizontalGroup:new {
    align = "center",
    TextWidget:new { text = self.title, face = self.face },
  }
  local body = VerticalGroup:new {
    align = "left"
  }
  local paddingx4 = self.padding * 4
  local max_height = 0
  local max_width_item = Screen:getWidth() - paddingx4 - ScrollableContainer:getScrollbarWidth()
  for index, option in ipairs(self.options) do
    local check = self.generate(option, max_width_item, index)

    check.parent = check
    max_height = max_height + check.dimen.h
    table.insert(body, check)
  end


  max_height = math.min(max_height, Screen:getHeight() - paddingx4)

  local scrollable = ScrollableContainer:new {
    dimen = Geom:new {
      w = Screen:getWidth() - paddingx4,
      h = max_height - self.navbar_height,
    },
    body,
  }
  local frame = FrameContainer:new {
    dimen = Geom:new {
      w = Screen:getWidth() - self.padding * 2,
      h = max_height + self.padding * 2,
    },
    padding = self.padding,
    background = Blitbuffer.COLOR_WHITE,
    radius = Size.radius.window,
    VerticalGroup:new {
      navbar,
      scrollable,
    }
  }

  self.movable = MovableContainer:new {
    frame,
    unmovable = self.unmovable,
  }
  scrollable.show_parent = self

  self[1] = CenterContainer:new {
    dimen = Screen:getSize(),
    self.movable,
  }
end

return CustomDialog
