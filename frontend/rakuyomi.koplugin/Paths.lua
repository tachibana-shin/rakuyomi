local DataStorage = require("datastorage")
local ffiutil = require("ffi/util")

local Paths = {}

--- @return string -- The directory in which Rakuyomi's home folder is located.
function Paths.getHomeDirectory()
  return DataStorage:getDataDir() .. "/rakuyomi"
end

--- @return string -- The directory in which the plugin is located.
function Paths.getPluginDirectory()
  local callerSource = debug.getinfo(1, "S").source
  if callerSource:find("^@") then
    local directory, _ = callerSource:gsub("^@(.*)/[^/]*", "%1")

    return ffiutil.realpath(directory)
  end

  error("could not find the plugin's directory")
end

return Paths
