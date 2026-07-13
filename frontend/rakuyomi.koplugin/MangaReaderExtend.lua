local FrameContainer = require("ui/widget/container/framecontainer")
local IconButton = require("ui/widget/iconbutton")
local Button = require("ui/widget/button")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local VerticalGroup = require("ui/widget/verticalgroup")
local LineWidget = require("ui/widget/linewidget")
local Geom = require("ui/geometry")
local Blitbuffer = require("ffi/blitbuffer")
local Device = require("device")
local Size = require("ui/size")
local getChapterDisplayName = require("utils/getChapterDisplayName")

local DGENERIC_ICON_SIZE = G_defaults:readSetting("DGENERIC_ICON_SIZE")
local Screen = Device.screen

--- @class MangaReaderExtend
--- @field new any
--- @field chapter Chapter
--- @field position "top"|"bottom" Where to render the toolbar.
--- @field on_prev_callback fun(): nil
--- @field on_next_callback fun(): nil
--- @field on_chapter_name_callback fun(): nil
local MangaReaderExtend = FrameContainer:extend {
  bordersize = 0,
  padding = 0,
}

--- @class MangaReaderExtendOpenOptions OpenOptions
--- @field chapter Chapter
--- @field position "top"|"bottom"
--- @field on_prev_callback fun(): nil
--- @field on_next_callback fun(): nil
--- @field on_chapter_name_callback fun(): nil
--- @param opts MangaReaderExtendOpenOptions
function MangaReaderExtend:create(opts)
  return MangaReaderExtend:new {
    chapter = opts.chapter,
    position = opts.position,
    on_prev_callback = opts.on_prev_callback,
    on_next_callback = opts.on_next_callback,
    on_chapter_name_callback = opts.on_chapter_name_callback
  }
end

function MangaReaderExtend:init()
  local icon_size = Screen:scaleBySize(DGENERIC_ICON_SIZE)
  local bar_height = icon_size + 2 * Size.padding.default
  local button_padding = Screen:scaleBySize(5)
  local is_top = self.position == "top"

  local prev_button = IconButton:new {
    icon = "chevron.first",
    width = icon_size,
    height = icon_size,
    padding = button_padding,
    callback = self.on_prev_callback,
    show_parent = self,
  }

  local next_button = IconButton:new {
    icon = "chevron.last",
    width = icon_size,
    height = icon_size,
    padding = button_padding,
    callback = self.on_next_callback,
    show_parent = self,
  }

  local chapter_name = getChapterDisplayName(self.chapter)
  local available_width = Screen:getWidth()
  local prev_width = prev_button:getSize().w
  local next_width = next_button:getSize().w
  local min_spacing = Screen:scaleBySize(4)
  local label_max_width = available_width - prev_width - next_width - min_spacing * 2

  local chapter_label = Button:new {
    text = chapter_name,
    max_width = label_max_width,
    avoid_text_truncation = false,
    bordersize = 0,
    padding_h = Screen:scaleBySize(10),
    padding_v = 0,
    text_font_size = 18,
    text_font_bold = false,
    enabled = true,
    callback = self.on_chapter_name_callback,
  }

  local label_width = chapter_label:getSize().w
  local spacing_width = math.max(min_spacing, math.floor((available_width - prev_width - next_width - label_width) / 2))

  local separator = LineWidget:new {
    background = Blitbuffer.COLOR_GRAY_1,
    dimen = Geom:new {
      w = available_width,
      h = Size.line.thick,
    },
  }

  local toolbar = HorizontalGroup:new {
    align = "center",
    prev_button,
    HorizontalSpan:new { width = spacing_width },
    chapter_label,
    HorizontalSpan:new { width = spacing_width },
    next_button,
  }

  self.dimen = Geom:new { x = 0, y = 0, w = available_width, h = bar_height + Size.line.thick }

  if is_top then
    table.insert(self, VerticalGroup:new { toolbar, separator })
  else
    table.insert(self, VerticalGroup:new { separator, toolbar })
  end
end

return MangaReaderExtend
