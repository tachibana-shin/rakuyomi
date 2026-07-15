local TopZoneHandler = {}

local function showFileManagerMenu()
  local FileManager = require("apps/filemanager/filemanager")
  if FileManager.instance and FileManager.instance.menu then
    FileManager.instance.menu:onShowMenu()
    return true
  end
  return false
end

function TopZoneHandler:enableTopZoneHandler()
  self:registerTouchZones({
    {
      id = "rakuyomi_top_tap",
      ges = "tap",
      screen_zone = {
        ratio_x = 0,
        ratio_y = 0,
        ratio_w = 1,
        ratio_h = 0.15,
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
        ratio_h = 0.15,
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
