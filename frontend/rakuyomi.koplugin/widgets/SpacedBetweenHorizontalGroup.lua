local HorizontalGroup = require("ui/widget/horizontalgroup")
local Screen = require("device").screen

local SpacedBetweenHorizontalGroup = HorizontalGroup:extend {
  width = nil,
}

function SpacedBetweenHorizontalGroup:init()
  self.width = self.width or Screen:getWidth()
end

function SpacedBetweenHorizontalGroup:getSize()
  if not self._size then
    HorizontalGroup.getSize(self)

    -- Change offsets so that things are spaced between
    local available_width = self.width - self._size.w
    local spacer_width = available_width / (#self - 1)

    for offset_index = 2, #self._offsets do
      self._offsets[offset_index].x = self._offsets[offset_index].x + (offset_index - 1) * spacer_width
    end

    -- Make it occupy the entire space.
    self._size.w = self.width
  end

  return self._size
end

return SpacedBetweenHorizontalGroup
