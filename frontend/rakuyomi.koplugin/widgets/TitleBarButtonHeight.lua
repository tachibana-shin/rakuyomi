--- Height budget for a text Button added to a title bar.
--
--- ui/widget/button has no `padding_bottom` field at all (only `padding`,
--- `padding_h` and `padding_v`), so a text button's frame is
--- `2 * padding_v + height` and the frame is painted opaque white. Left
--- unconstrained it is taller than the IconButton siblings, whose padding area
--- is transparent, and it paints over the bottom line — at every screen size,
--- since both are derived from Screen:scaleBySize.
---
--- Two caps, both from values the caller already computes:
---   * the IconButton size, so the frame never grows taller than its siblings;
---   * the width KOReader reserves above the bottom line, minus our own vertical
---     padding, so the frame stops before the line.
---
--- The line sits in `title_bar[2]`, whose first child is the VerticalSpan
--- TitleBar inserts right before the LineWidget.
--- @param title_bar table the TitleBar being patched
--- @param icon_size number the IconButton height the frame must not exceed
--- @param padding number the button's vertical padding
--- @return number height to give the Button
local function titleBarTextButtonHeight(title_bar, icon_size, padding)
  local height = icon_size
  local filler_span
  if title_bar.with_bottom_line and title_bar[2] then
    filler_span = title_bar[2][1]
  end
  if filler_span and type(filler_span.width) == "number" then
    -- Hardcoded-number exception: the floor keeps a degenerate geometry from
    -- producing a non-positive height.
    height = math.max(1, math.min(icon_size, filler_span.width - 2 * padding))
  end
  return height
end

return titleBarTextButtonHeight
