local TopZoneHandler = {}

local function showFileManagerMenu()
  local FileManager = require("apps/filemanager/filemanager")
  if FileManager.instance and FileManager.instance.menu then
    FileManager.instance.menu:onShowMenu()
    return true
  end
  return false
end

--- @param ratio_h number|nil Height of the tap/swipe zone as a fraction of the
--- screen. Defaults to 0.15. Views whose content reaches into the top of the
--- screen (e.g. Settings) should pass a smaller value so the zone does not
--- swallow taps meant for their topmost interactive elements.
function TopZoneHandler:enableTopZoneHandler(ratio_h)
  ratio_h = ratio_h or 0.15

  self:registerTouchZones({
    {
      id = "rakuyomi_top_tap",
      ges = "tap",
      screen_zone = {
        ratio_x = 0,
        ratio_y = 0,
        ratio_w = 1,
        ratio_h = ratio_h,
      },
      handler = function()
        return showFileManagerMenu()
      end,
    },
    {
      id = "rakuyomi_top_swipe",
      ges = "swipe",
      screen_zone = {
        ratio_x = 0,
        ratio_y = 0,
        ratio_w = 1,
        ratio_h = ratio_h,
      },
      handler = function(ges)
        if ges.direction ~= "south" then
          return false
        end
        return showFileManagerMenu()
      end,
    },
  })
end

return TopZoneHandler
