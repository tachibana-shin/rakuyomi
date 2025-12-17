local Device = require("device")
local Geom = require("ui/geometry")

local InfoMessage = require("ui/widget/infomessage")
local Font = require("ui/font")
local GestureRange = require("ui/gesturerange")
local Screen = require("device").screen

local Input = Device.input

local DialogEmpty = InfoMessage:extend {}
function DialogEmpty:init()
  if not self.face then
    self.face = Font:getFace(self.monospace_font and "infont" or "infofont")
  end

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
end

return DialogEmpty
