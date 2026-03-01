local GestureRange = require("ui/gesturerange")
local RightContainer = require("ui/widget/container/rightcontainer")
local HorizontalGroup = require("ui/widget/horizontalgroup")
local HorizontalSpan = require("ui/widget/horizontalspan")
local VerticalGroup = require("ui/widget/verticalgroup")
local VerticalSpan = require("ui/widget/verticalspan")
local Size = require("ui/size")
local TextBoxWidget = require("ui/widget/textboxwidget")
local TextWidget = require("ui/widget/textwidget")
local logger = require("logger")
local MenuItemRaw = require("MenuItem")
local Device = require("device")
local Font = require("ui/font")
local FrameContainer = require("ui/widget/container/framecontainer")
local Geom = require("ui/geometry")
local Blitbuffer = require("ffi/blitbuffer")
local LeftContainer = require("ui/widget/container/leftcontainer")
local UnderlineContainer = require("ui/widget/container/underlinecontainer")
local OverlapGroup = require("ui/widget/overlapgroup")
local Math = require("optmath")
local ImageWidget = require("ui/widget/imagewidget")
local CenterContainer = require("ui/widget/container/centercontainer")

local Screen = Device.screen

local MenuItemCover = MenuItemRaw:extend {}

function MenuItemCover:init()
  self.content_width = self.dimen.w - 2 * Size.padding.fullscreen

  local shortcut_icon_dimen
  if self.shortcut then
    local icon_width = self.entry.shortcut_icon_width or math.floor(self.dimen.h * 4 / 5)
    shortcut_icon_dimen = Geom:new {
      x = 0,
      y = 0,
      w = icon_width,
      h = icon_width,
    }
    self.content_width = self.content_width - shortcut_icon_dimen.w - Size.span.horizontal_default
  end

  -- we need this table per-instance, so we declare it here
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

  local max_item_height = self.dimen.h - 2 * self.linesize

  --- @patch
  local img_width, img_height
  if Screen:getScreenMode() == "landscape" then
    img_height = math.min(Screen:scaleBySize(184), max_item_height) - 4
  else
    img_height = math.min(Screen:scaleBySize(184 * 1.5), max_item_height) - 4
  end
  img_width = 132 / 184 * img_height
  --- @end patch

  self.content_width = self.dimen.w - 2 * Size.padding.fullscreen

  -- We want to show at least one line, so cap the provided font sizes
  local max_font_size = TextBoxWidget:getFontSizeToFitHeight(max_item_height, 1)
  if self.font_size > max_font_size then
    self.font_size = max_font_size
  end
  if self.infont_size > max_font_size then
    self.infont_size = max_font_size
  end
  if not self.single_line and not self.multilines_forced
      and not self.multilines_show_more_text and not self.items_max_lines then
    -- For non single line menus (File browser, Bookmarks), if the
    -- user provided font size is large and would not allow showing
    -- more than one line in our item height, just switch to single
    -- line mode. This allows, when truncating, to take the full
    -- width and cut inside a word to add the ellipsis - while in
    -- multilines modes, with TextBoxWidget, words are wrapped to
    -- follow line breaking rules, and the ellipsis might be placed
    -- way earlier than the full width.
    local min_font_size_2_lines = TextBoxWidget:getFontSizeToFitHeight(max_item_height, 2)
    if self.font_size > min_font_size_2_lines then
      self.single_line = true
    end
  end

  -- State button and indentation for tree expand/collapse (for TOC)
  local state_button = self.entry.state or HorizontalSpan:new {}
  local state_indent = self.entry.indent or 0
  local state_width = state_indent + (self.state_w or 0)
  local state_container = LeftContainer:new {
    dimen = Geom:new { w = math.floor(self.content_width / 2), h = self.dimen.h },
    HorizontalGroup:new {
      HorizontalSpan:new {
        width = state_indent,
      },
      state_button,
    }
  }

  -- Font for main text (may have its size decreased to make text fit)
  self.face = Font:getFace(self.font, self.font_size)
  -- Font for "mandatory" on the right
  self.info_face = Font:getFace(self.infont, self.infont_size)
  -- Font for post_text if any: for now, this is only used with TOC, showing
  -- the chapter length: if feels best to use the face of the main text, but
  -- with the size of the mandatory font (which shows some number too).
  if self.post_text then
    self.post_text_face = Font:getFace(self.font, self.infont_size)
  end

  -- "mandatory" is the text on the right: file size, page number...
  -- Padding before mandatory
  local text_mandatory_padding = 0
  local text_ellipsis_mandatory_padding = 0
  local mandatory = self.mandatory_func and self.mandatory_func() or self.mandatory
  local mandatory_dim = self.mandatory_dim_func and self.mandatory_dim_func() or self.mandatory_dim
  if mandatory then
    text_mandatory_padding = Size.span.horizontal_default
    -- Smaller padding when ellipsis for better visual feeling
    text_ellipsis_mandatory_padding = Size.span.horizontal_small
  end
  local mandatory_widget = TextWidget:new {
    text = mandatory or "",
    face = self.info_face,
    bold = self.bold,
    fgcolor = mandatory_dim and Blitbuffer.COLOR_DARK_GRAY or nil,
  }
  local mandatory_w = mandatory_widget:getWidth()

  local available_width = self.content_width - state_width - text_mandatory_padding - mandatory_w - img_width - 12
  local text_fgcolor = self.dim and Blitbuffer.COLOR_DARK_GRAY or nil
  local text_bgcolor = self.text_bgcolor
  local item_name

  -- Whether we show text on a single or multiple lines, we don't want it shortened
  -- because of some \n that would push the following text on another line that would
  -- overflow and not be displayed, or show a tofu char when displayed by TextWidget:
  -- get rid of any \n (which could be found in highlighted text in bookmarks).
  local text = self.text:gsub("\n", " ")

  -- Wrap text with provided bidi_wrap_func (only provided by FileChooser,
  -- to correctly display filenames and directories)
  if self.bidi_wrap_func then
    text = self.bidi_wrap_func(text)
  end

  -- Note: support for post_text is currently implemented only when single_line=true
  local post_text_widget
  local dots_widget
  local dots_left_padding = Size.padding.small
  local dots_right_padding = Size.padding.small

  if self.single_line then
    -- Items only in single line
    if self.post_text then
      post_text_widget = TextWidget:new {
        text = self.post_text,
        face = self.post_text_face,
        bold = self.bold,
        fgcolor = text_fgcolor,
        padding = 0,
      }
    end
    -- No font size change: text will be truncated if it overflows
    item_name = TextWidget:new {
      text = text,
      face = self.face,
      bold = self.bold,
      truncate_left = self.truncate_left,
      fgcolor = text_fgcolor,
    }
    local w = item_name:getWidth()
    if w > available_width then
      local text_max_width_if_ellipsis = available_width
      -- We give it a little more room if truncated at the right for better visual
      -- feeling (which might make it no more truncated, but well...)
      if not self.truncate_left then
        text_max_width_if_ellipsis = text_max_width_if_ellipsis + text_mandatory_padding -
            text_ellipsis_mandatory_padding
      end
      item_name:setMaxWidth(text_max_width_if_ellipsis)
    else
      if self.with_dots then
        local dots_width = available_width + text_mandatory_padding - w - dots_left_padding - dots_right_padding
        if dots_width > 0 then
          local dots_text, dots_min_width = self:getDotsText(self.info_face)
          -- Don't show any dots if there would be less than 3
          if dots_width >= dots_min_width then
            dots_widget = TextWidget:new {
              text = dots_text,
              face = self.info_face, -- same as mandatory widget, to keep their baseline adjusted
              max_width = dots_width,
              truncate_with_ellipsis = false,
            }
          end
        end
      end
    end
    item_name = VerticalGroup:new {
      item_name,
      post_text_widget,
      align = "left",
    }
    if self.align_baselines then -- Align baselines of text and mandatory
      -- The container widgets would additionally center these widgets,
      -- so make sure they all get a height=self.dimen.h so they don't
      -- risk being shifted later and becoming misaligned
      local name_baseline = item_name[1]:getBaseline()
      local mdtr_baseline = mandatory_widget:getBaseline()
      local name_height = item_name[1]:getSize().h
      local mdtr_height = mandatory_widget:getSize().h
      -- Make all the TextWidgets be self.dimen.h
      item_name.forced_height = self.dimen.h
      mandatory_widget.forced_height = self.dimen.h
      if dots_widget then
        dots_widget.forced_height = self.dimen.h
      end
      -- And adjust their baselines for proper centering and alignment
      -- (We made sure the font sizes wouldn't exceed self.dimen.h, so we
      -- get only non-negative pad_top here, and we're moving them down.)
      local name_missing_pad_top = math.floor((self.dimen.h - name_height) / 2)
      local mdtr_missing_pad_top = math.floor((self.dimen.h - mdtr_height) / 2)
      name_baseline = name_baseline + name_missing_pad_top
      mdtr_baseline = mdtr_baseline + mdtr_missing_pad_top
      local baselines_diff = Math.round(name_baseline - mdtr_baseline)
      if baselines_diff > 0 then
        mdtr_baseline = mdtr_baseline + baselines_diff
      else
        name_baseline = name_baseline - baselines_diff
      end
      -- item_name[1].forced_baseline = name_baseline
      mandatory_widget.forced_baseline = mdtr_baseline
      if dots_widget then
        dots_widget.forced_baseline = mdtr_baseline
      end
    end
    if text_bgcolor then
      item_name = FrameContainer:new {
        width = math.max(item_name[1]:getWidth(), available_width), -- if the ellipsis doesn't fit
        background = text_bgcolor,
        bordersize = 0,
        padding = 0,
        margin = 0,
        item_name,
      }
    end
  elseif self.multilines_show_more_text then
    -- Multi-lines, with font size decrease if needed to show more of the text.
    -- It would be costly/slow with use_xtext if we were to try all
    -- font sizes from self.font_size to min_font_size.
    -- So, we try to optimize the search of the best font size.
    logger.dbg("multilines_show_more_text menu item font sizing start")
    local function make_item_name(font_size)
      if item_name then
        item_name:free()
      end
      logger.dbg("multilines_show_more_text trying font size", font_size)
      item_name = TextBoxWidget:new {
        text = text,
        face = Font:getFace(self.font, font_size),
        width = available_width,
        alignment = "left",
        bold = self.bold,
        fgcolor = text_fgcolor,
        bgcolor = text_bgcolor,
      }
      -- return true if we fit
      return item_name:getSize().h <= max_item_height
    end
    -- To keep item readable, do not decrease font size by more than 8 points
    -- relative to the specified font size, being not smaller than 12 absolute points.
    local min_font_size = math.max(12, self.font_size - 8)
    -- First, try with specified font size: short text might fit
    if not make_item_name(self.font_size) then
      -- It doesn't, try with min font size: very long text might not fit
      if not make_item_name(min_font_size) then
        -- Does not fit with min font size: keep widget with min_font_size, but
        -- impose a max height to show only the first lines up to where it fits
        item_name:free()
        item_name.height = max_item_height
        item_name.height_adjust = true
        item_name.height_overflow_show_ellipsis = true
        item_name:init()
      else
        -- Text fits with min font size: try to find some larger
        -- font size in between that make text fit, with some
        -- binary search to limit the number of checks.
        local bad_font_size = self.font_size
        local good_font_size = min_font_size
        local item_name_is_good = true
        while true do
          local test_font_size = math.floor((good_font_size + bad_font_size) / 2)
          if test_font_size == good_font_size then -- +1 would be bad_font_size
            if not item_name_is_good then
              make_item_name(good_font_size)
            end
            break
          end
          if make_item_name(test_font_size) then
            good_font_size = test_font_size
            item_name_is_good = true
          else
            bad_font_size = test_font_size
            item_name_is_good = false
          end
        end
      end
    end
  else
    -- Multi-lines, with fixed user provided font size
    item_name = TextBoxWidget:new {
      text = text,
      face = self.face,
      width = available_width,
      height = self.entry.height and (self.entry.height - 2 * Size.span.vertical_default - self.linesize) or max_item_height,
      height_adjust = true,
      height_overflow_show_ellipsis = true,
      alignment = "left",
      bold = self.bold,
      fgcolor = text_fgcolor,
      bgcolor = text_bgcolor,
    }
  end

  local text_container = LeftContainer:new {
    dimen = Geom:new { w = self.content_width, h = self.dimen.h },
    HorizontalGroup:new {
      HorizontalSpan:new {
        width = state_width,
      },
      --- @patch
      VerticalGroup:new {
        VerticalSpan:new { width = 2 },

        self:genCover(img_width, img_height),

        VerticalSpan:new { width = 2 },
      },
      HorizontalSpan:new {
        width = 12,
      },
      --- @end patch
      item_name,
    }
  }

  if dots_widget then
    mandatory_widget = HorizontalGroup:new {
      dots_widget,
      HorizontalSpan:new { width = dots_right_padding },
      mandatory_widget,
    }
  end
  local mandatory_container = RightContainer:new {
    dimen = Geom:new { w = self.content_width, h = self.dimen.h },
    mandatory_widget,
  }

  self._underline_container = UnderlineContainer:new {
    color = self.line_color,
    linesize = self.linesize,
    vertical_align = "center",
    padding = 0,
    dimen = Geom:new {
      x = 0, y = 0,
      w = self.content_width,
      h = self.dimen.h
    },
    HorizontalGroup:new {
      align = "center",
      OverlapGroup:new {
        dimen = Geom:new { w = self.content_width, h = self.dimen.h },
        state_container,
        text_container,
        mandatory_container,
      },
    }
  }
  local hgroup = HorizontalGroup:new {
    align = "center",
    HorizontalSpan:new { width = self.items_padding or Size.padding.fullscreen },
  }
  if self.shortcut then
    table.insert(hgroup, self.menu:getItemShortCutIcon(shortcut_icon_dimen, self.shortcut, self.shortcut_style))
    table.insert(hgroup, HorizontalSpan:new { width = Size.span.horizontal_default })
  end
  table.insert(hgroup, self._underline_container)
  table.insert(hgroup, HorizontalSpan:new { width = Size.padding.fullscreen })

  self[1] = FrameContainer:new {
    bordersize = 0,
    padding = 0,
    hgroup,
  }
end

local function starts_with(s, p)
  return s:sub(1, #p) == p
end

local function getCachedCoverSize(img_w, img_h, max_img_w, max_img_h)
  local scale_factor
  local width = math.floor(max_img_h * img_w / img_h + 0.5)
  if max_img_w >= width then
    max_img_w = width
    scale_factor = max_img_w / img_w
  else
    max_img_h = math.floor(max_img_w * img_h / img_w + 0.5)
    scale_factor = max_img_h / img_h
  end
  return max_img_w, max_img_h, scale_factor
end

--- the bishop!!
local scale_by_size = Screen:scaleBySize(1000000) * (1 / 1000000)
function MenuItemCover:genCover(wleft_width, wleft_height)
  local border_size = Size.border.thin

  local wleft
  if self.entry.manga_cover and starts_with(self.entry.manga_cover, "file://") then
    local wimage = ImageWidget:new {
      file = self.entry.manga_cover:gsub("^file://", ""),
      -- scale_factor = 0.5
    }
    wimage:_loadfile()
    local image_size = wimage:getSize() -- get final widget size
    local _, _, scale_factor = getCachedCoverSize(image_size.w, image_size.h, wleft_width, wleft_height)

    wimage = ImageWidget:new {
      file = self.entry.manga_cover:gsub("^file://", ""),
      scale_factor = scale_factor
    }

    wimage:_render()
    image_size = wimage:getSize()

    wleft = CenterContainer:new {
      dimen = Geom:new { w = wleft_width, h = wleft_height },
      FrameContainer:new {
        width = image_size.w + 2 * border_size,
        height = image_size.h + 2 * border_size,
        margin = 0,
        padding = 0,
        bordersize = border_size,
        dim = self.file_deleted,
        color = Blitbuffer.COLOR_GRAY_9,
        wimage,
      }
    }
    -- Let menu know it has some item with images
    self.menu._has_cover_images = true
    self._has_cover_image = true
  else
    local function _fontSize(nominal, max)
      -- The nominal font size is based on 64px ListMenuItem height.
      -- Keep ratio of font size to item height
      local font_size = math.floor(nominal * self.dimen.h * (1 / 64) / scale_by_size)
      -- But limit it to the provided max, to avoid huge font size when
      -- only 4-6 items per page
      if max and font_size >= max then
        return max
      end
      return font_size
    end

    local max_w = wleft_width - border_size * 2
    local max_h = wleft_height - border_size * 2

    local aspect = 2 / 3 -- width / height

    local w_by_height = max_h * aspect
    local h_by_width = max_w / aspect

    local fake_cover_w, fake_cover_h

    if w_by_height <= max_w then
      fake_cover_w = w_by_height
      fake_cover_h = max_h
    else
      fake_cover_w = max_w
      fake_cover_h = h_by_width
    end
    wleft = CenterContainer:new {
      dimen = Geom:new { w = wleft_width, h = wleft_height },
      FrameContainer:new {
        width = fake_cover_w + 2 * border_size,
        height = fake_cover_h + 2 * border_size,
        margin = 0,
        padding = 0,
        bordersize = border_size,
        dim = self.file_deleted,
        color = Blitbuffer.COLOR_GRAY_9,
        CenterContainer:new {
          dimen = Geom:new { w = fake_cover_w, h = fake_cover_h },
          TextWidget:new {
            text = "⛶", -- U+26F6 Square four corners
            face = Font:getFace("cfont", _fontSize(20)),
          },
        },
      },
    }
  end

  return wleft
end

return MenuItemCover
