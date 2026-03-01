local Blitbuffer = require("ffi/blitbuffer")
local Device = require("device")
local InputContainer = require("ui/widget/container/inputcontainer")
local Size = require("ui/size")
local TextWidget = require("ui/widget/textwidget")
local UIManager = require("ui/uimanager")
local ffiUtil = require("ffi/util")
local Screen = Device.screen

--[[
Widget that displays an item for menu
--]]
local MenuItem = InputContainer:extend {
  font = "smallinfofont",
  infont = "infont",
  linesize = Size.line.medium,
  single_line = true,
  multilines_forced = false, -- set to true to always use TextBoxWidget
  multilines_show_more_text = false,
  -- Align text & mandatory baselines (only when single_line=true)
  align_baselines = false,
  -- Show a line of dots (also called tab or dot leaders) between text and mandatory
  with_dots = false,
}

local _dots_cached_info
function MenuItem:getDotsText(face)
  local screen_w = Screen:getWidth()
  if not _dots_cached_info or _dots_cached_info.screen_width ~= screen_w
      or _dots_cached_info.face ~= face then
    local unit = "."
    local tmp = TextWidget:new {
      text = unit,
      face = face,
    }
    local unit_w = tmp:getSize().w
    tmp:free()
    -- (We assume/expect no kerning will happen between consecutive units)
    local nb_units = math.ceil(screen_w / unit_w)
    local min_width = unit_w * 3 -- have it not shown if smaller than this
    local text = unit:rep(nb_units)
    _dots_cached_info = {
      text = text,
      min_width = min_width,
      screen_width = screen_w,
      face = face,
    }
  end
  return _dots_cached_info.text, _dots_cached_info.min_width
end

function MenuItem:onFocus()
  self._underline_container.color = Blitbuffer.COLOR_BLACK
  -- NOTE: Medium is really, really, really thin; so we'd ideally swap to something thicker...
  --       Unfortunately, this affects vertical text positioning,
  --       leading to an unsightly refresh of the item :/.
  --self._underline_container.linesize = Size.line.thick
  return true
end

function MenuItem:onUnfocus()
  self._underline_container.color = self.line_color
  -- See above for reasoning.
  --self._underline_container.linesize = self.linesize
  return true
end

function MenuItem:getGesPosition(ges)
  local dimen = self[1].dimen
  return {
    x = (ges.pos.x - dimen.x) / dimen.w,
    y = (ges.pos.y - dimen.y) / dimen.h,
  }
end

function MenuItem:onTapSelect(arg, ges)
  -- Abort if the menu hasn't been painted yet.
  if not self[1].dimen then return end

  local pos = self:getGesPosition(ges)
  if G_reader_settings:isFalse("flash_ui") then
    self.menu:onMenuSelect(self.entry, pos)
  else
    -- c.f., ui/widget/iconbutton for the canonical documentation about the flash_ui code flow

    -- Highlight
    --
    self[1].invert = true
    UIManager:widgetInvert(self[1], self[1].dimen.x, self[1].dimen.y)
    UIManager:setDirty(nil, "fast", self[1].dimen)

    UIManager:forceRePaint()
    UIManager:yieldToEPDC()

    -- Unhighlight
    --
    self[1].invert = false
    UIManager:widgetInvert(self[1], self[1].dimen.x, self[1].dimen.y)
    UIManager:setDirty(nil, "ui", self[1].dimen)

    -- Callback
    --
    self.menu:onMenuSelect(self.entry, pos)

    UIManager:forceRePaint()
  end
  return true
end

function MenuItem:onHoldSelect(arg, ges)
  if not self[1].dimen then return end

  local pos = self:getGesPosition(ges)
  if G_reader_settings:isFalse("flash_ui") then
    self.menu:onMenuHold(self.entry, pos)
  else
    -- c.f., ui/widget/iconbutton for the canonical documentation about the flash_ui code flow

    -- Highlight
    --
    self[1].invert = true
    UIManager:widgetInvert(self[1], self[1].dimen.x, self[1].dimen.y)
    UIManager:setDirty(nil, "fast", self[1].dimen)

    UIManager:forceRePaint()
    UIManager:yieldToEPDC()

    -- Unhighlight
    --
    self[1].invert = false
    UIManager:widgetInvert(self[1], self[1].dimen.x, self[1].dimen.y)
    UIManager:setDirty(nil, "ui", self[1].dimen)

    -- Callback
    --
    self.menu:onMenuHold(self.entry, pos)

    UIManager:forceRePaint()
  end
  return true
end

return MenuItem
