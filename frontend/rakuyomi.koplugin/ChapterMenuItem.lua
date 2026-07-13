local Blitbuffer = require("ffi/blitbuffer")
local Button = require("ui/widget/button")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local LeftContainer = require("ui/widget/container/leftcontainer")
local OverlapGroup = require("ui/widget/overlapgroup")
local RightContainer = require("ui/widget/container/rightcontainer")
local Size = require("ui/size")
local TextWidget = require("ui/widget/textwidget")
local Gesture = require("ui/gesturerange")

--- Single-line chapter item for ChapterListPopup.
--- Extends Button, adds post_text (after main text) and mandatory (right-aligned).
--- Tap calls menu:onMenuSelect(entry), hold calls menu:onMenuHold(entry).
--- @class ChapterMenuItem
--- @field padding any
--- @field font_size number?
--- @field infont_size number?
--- @field text string?
--- @field mandatory string?
--- @field post_text string?
--- @field mandatory_func fun():string?
--- @field mandatory_dim_func fun():boolean?
--- @field dim any
--- @field show_parent any
--- @field is_current boolean?
--- @field text_font_face string
--- @field mandatory_dim any
--- @field new any
local ChapterMenuItem = Button:extend {
  name = "chapter_menu_item",
  text_font_face = "smallinfofont",
  text_font_bold = false,
  bordersize = 0,
  padding = 0,
  margin = 0,
  background = Blitbuffer.COLOR_WHITE,
  single_line = true,
}

function ChapterMenuItem:init()
  if not self.padding_h then
    self.padding_h = self.padding
  end
  if not self.padding_v then
    self.padding_v = self.padding
  end

  local content_width = self.dimen.w
  local font_size = self.font_size or 14
  local infont_size = self.infont_size or (font_size - 4)

  local face = Font:getFace(self.text_font_face, font_size)
  local info_face = Font:getFace(self.text_font_face, infont_size)
  local text_fgcolor = self.dim and Blitbuffer.COLOR_DARK_GRAY or Blitbuffer.COLOR_BLACK

  local text = (self.text or ""):gsub("\n", " ")

  local mandatory = self.mandatory_func and self.mandatory_func() or self.mandatory
  local mandatory_dim = self.mandatory_dim_func and self.mandatory_dim_func() or self.mandatory_dim

  local text_mandatory_padding = 0
  if mandatory then
    text_mandatory_padding = Size.span.horizontal_default
  end
  local mandatory_widget = TextWidget:new {
    text = mandatory or "",
    face = info_face,
    bold = false,
    fgcolor = mandatory_dim and Blitbuffer.COLOR_DARK_GRAY or nil,
  }
  local mandatory_w = mandatory_widget:getWidth()

  local available_width = content_width - text_mandatory_padding - mandatory_w

  local post_text_widget
  if self.post_text then
    post_text_widget = TextWidget:new {
      text = self.post_text,
      face = Font:getFace(self.text_font_face, infont_size),
      max_width = math.floor(available_width / 2),
      fgcolor = text_fgcolor,
    }
    available_width = available_width - post_text_widget:getWidth() - Size.padding.large * 2
  end

  local item_name = TextWidget:new {
    text = text,
    face = face,
    bold = false,
    fgcolor = text_fgcolor,
  }
  if item_name:getWidth() > available_width then
    item_name:setMaxWidth(available_width)
  end

  local text_container = LeftContainer:new {
    dimen = Geom:new { w = content_width, h = self.dimen.h },
    HorizontalGroup:new {
      item_name,
      post_text_widget and HorizontalSpan:new { width = Size.padding.large },
      post_text_widget,
    },
  }

  local mandatory_container = RightContainer:new {
    dimen = Geom:new { w = content_width, h = self.dimen.h },
    mandatory_widget,
  }
  self.label_widget = OverlapGroup:new {
    dimen = Geom:new { w = content_width, h = self.dimen.h },
    fgcolor = Blitbuffer.COLOR_WHITE,
    text_container,
    mandatory_container,
  }
  self.frame = FrameContainer:new {
    bordersize = 0,
    padding = 0,
    padding_left = Size.padding.large,
    padding_right = Size.padding.large,
    padding_top = Size.padding.default,
    padding_bottom = Size.padding.default,
    background = Blitbuffer.COLOR_WHITE,
    dimen = Geom:new {
      x = 0, y = 0,
      w = content_width,
      h = self.dimen.h,
    },
    show_parent = self.show_parent,
    self.label_widget
  }

  self.dimen = self.frame:getSize()
  self[1] = self.frame

  if self.is_current then
    self.frame.invert = true
  end

  self.ges_events = {
    TapSelectButton = {
      Gesture:new {
        ges = "tap",
        range = self.dimen,
      },
    },
    HoldSelectButton = {
      Gesture:new {
        ges = "hold",
        range = self.dimen,
      },
    }
  }
end

return ChapterMenuItem
