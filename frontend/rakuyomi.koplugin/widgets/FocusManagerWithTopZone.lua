local FocusManager = require("ui/widget/focusmanager")
local TopZoneHandler = require("widgets/TopZoneHandler")

local FocusManagerWithTopZone = FocusManager:extend {}

function FocusManagerWithTopZone:new(o)
  local instance = FocusManager.new(self, o)
  -- FocusManager-based views (Settings, SourceSettings, ...) place tappable
  -- rows right below the title bar, so restrict the top zone to the title-bar
  -- strip instead of the default 15% of the screen.
  TopZoneHandler.enableTopZoneHandler(instance, 0.08)
  return instance
end

return FocusManagerWithTopZone
