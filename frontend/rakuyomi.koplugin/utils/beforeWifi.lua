local NetworkMgr = require("ui/network/manager")

local function beforeWifi(callback)
  if NetworkMgr:isOnline() then
    callback()

    return
  end

  if not NetworkMgr:isOnline() then
    NetworkMgr:beforeWifiAction(callback)
  end
  NetworkMgr:beforeWifiAction()

  callback()
end

return beforeWifi
