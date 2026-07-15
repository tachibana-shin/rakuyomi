local Screen = require("device").screen

local TopZoneHandler = {}

function TopZoneHandler:enableTopZoneHandler()
  local screen_h = Screen:getHeight()
  local top_h = math.floor(screen_h * 0.15)

  self:registerTouchZones({
    {
      id = "rakuyomi_top_tap",
      ges = "tap",
      screen_zone = {
        ratio_x = 0,
        ratio_y = 0,
        ratio_w = 1,
        ratio_h = top_h / screen_h,
      },
      handler = function(_ges)
        local FileManager = require("apps/filemanager/filemanager")
        if FileManager.instance and FileManager.instance.menu then
          FileManager.instance.menu:onShowMenu()
        end
        return true
      end,
    },
    {
      id = "rakuyomi_top_swipe",
      ges = "swipe",
      screen_zone = {
        ratio_x = 0,
        ratio_y = 0,
        ratio_w = 1,
        ratio_h = top_h / screen_h,
      },
      handler = function(ges)
        if ges.direction ~= "south" then
          return false
        end
        local FileManager = require("apps/filemanager/filemanager")
        if FileManager.instance and FileManager.instance.menu then
          FileManager.instance.menu:onShowMenu()
        end
        return true
      end,
    },
  })
end

return TopZoneHandler
