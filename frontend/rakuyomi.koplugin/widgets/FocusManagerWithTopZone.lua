local FocusManager = require("ui/widget/focusmanager")
local TopZoneHandler = require("widgets/TopZoneHandler")

local FocusManagerWithTopZone = FocusManager:extend {}

function FocusManagerWithTopZone:_init()
  FocusManager._init(self)
  TopZoneHandler.enableTopZoneHandler(self)
end

return FocusManagerWithTopZone
