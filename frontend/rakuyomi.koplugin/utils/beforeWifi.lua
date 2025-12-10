local NetworkMgr = require("ui/network/manager")

local function beforeWifi(callback)
  if NetworkMgr:isConnected() then
    callback()

    return
  end

  if not NetworkMgr:isConnected() then
    NetworkMgr:beforeWifiAction(callback)
  end
  NetworkMgr:beforeWifiAction()

  callback()
end

return beforeWifi
