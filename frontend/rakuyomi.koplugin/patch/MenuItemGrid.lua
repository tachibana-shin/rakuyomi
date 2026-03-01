local GestureRange = require("ui/gesturerange")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local Size = require("ui/size")
local TextWidget = require("ui/widget/textwidget")
local MenuItemRaw = require("MenuItem")
local Device = require("device")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local Blitbuffer = require("ffi/blitbuffer")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")

local MenuItemCover = require("patch/MenuItemCover")

local Screen = Device.screen

local MenuItemGrid = MenuItemRaw:extend {}

function MenuItemGrid:init()
  self.content_width = self.dimen.w - 2 * Size.padding.fullscreen
  self.content_height = self.dimen.h - 2 * Size.padding.fullscreen

  self.ges_events = {
    TapSelect = {
      GestureRange:new {
        ges = "tap",
        range = self.dimen,
      },
    },
    HoldSelect = {
      GestureRange:new {
        ges = self.handle_hold_on_hold_release and "hold_release" or "hold",
        range = self.dimen,
      },
    },
  }

  local text_height = Screen:scaleBySize(44)
  local img_width = self.dimen.w - 6
  local img_height = self.dimen.h - text_height - 12 - 6 -- padding y = 3

  -- Main text (Title)
  self.face = Font:getFace(self.font, self.font_size)

  local title_widget = TextWidget:new {
    text = self.text,
    face = self.face,
    max_width = self.dimen.w - 6,
    padding = 0,
    bold = self.bold,
    fgcolor = self.dim and Blitbuffer.COLOR_DARK_GRAY or nil,
  }

  -- Unread count / Mandatory info
  local mandatory = self.mandatory_func and self.mandatory_func() or self.mandatory
  local mandatory_widget
  if mandatory and mandatory ~= "" then
    mandatory_widget = TextWidget:new {
      text = mandatory,
      face = Font:getFace(self.infont, self.infont_size),
      bold = self.bold,
      fgcolor = Blitbuffer.COLOR_BLACK,
    }
    -- Wrap mandatory in a small frame for better visibility over covers
    mandatory_widget = FrameContainer:new {
      padding = 0,
      bordersize = 0,
      background = Blitbuffer.COLOR_WHITE,
      color = Blitbuffer.TRANSPARENT,
      mandatory_widget
    }
  end

  local cover_widget = MenuItemCover.genCover(self, img_width, img_height)

  local main_content = FrameContainer:new {
    padding = 0,
    bordersize = 0,
    VerticalGroup:new {
      VerticalGroup:new {
        align = "center",
        cover_widget,
      },
      title_widget,
    }
  }

  local final_content
  if mandatory_widget then
    final_content = VerticalGroup:new {
      main_content,
      mandatory_widget
    }
  else
    final_content = main_content
  end

  self._underline_container = FrameContainer:new {
    padding = 0,
    bordersize = 0,
    HorizontalGroup:new {
      HorizontalSpan:new { width = 3 },
      VerticalGroup:new {
        VerticalSpan:new { width = 3 },
        final_content,
        VerticalSpan:new { width = 3 },
      },
      HorizontalSpan:new { width = 3 },
    }
  }

  self[1] = FrameContainer:new {
    width = self.dimen.w,
    height = self.dimen.h,
    padding = 0,
    margin = 0, -- remove margin to ensure full 1/3 width
    color = Blitbuffer.TRANSPARENT,
    bordersize = 0,
    self._underline_container,
  }
end

return MenuItemGrid
