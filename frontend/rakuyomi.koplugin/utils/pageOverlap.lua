--- Utilities for the configurable page overlap used by the continuous scroll
--- viewers (Scroll and Vertical).
local PageOverlap = {}

--- Maximum configurable overlap, in percent of the visible page height.
PageOverlap.MAX_PERCENT = 50

--- Converts an overlap percentage into a pixel offset for `ReaderPaging`.
--- Values outside the supported range are clamped, so a missing or broken
--- setting falls back to no overlap.
--- @param visible_height number|nil Height of the visible page area, in pixels.
--- @param percent number|nil Overlap, in percent of `visible_height`.
--- @return number
function PageOverlap.computePixels(visible_height, percent)
  local value = tonumber(percent) or 0
  value = math.max(0, math.min(PageOverlap.MAX_PERCENT, value))
  return math.floor((visible_height or 0) * value / 100)
end

return PageOverlap
