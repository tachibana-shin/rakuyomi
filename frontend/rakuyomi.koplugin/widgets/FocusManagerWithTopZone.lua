local FocusManager = require("ui/widget/focusmanager")
local TopZoneHandler = require("widgets/TopZoneHandler")

local FocusManagerWithTopZone = FocusManager:extend {}

function FocusManagerWithTopZone:new(o)
  local instance = FocusManager.new(self, o)
  TopZoneHandler.enableTopZoneHandler(instance, 0.08)
  return instance
end

return FocusManagerWithTopZone
