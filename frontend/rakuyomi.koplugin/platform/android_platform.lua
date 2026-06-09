local logger = require("logger")
local ltn12 = require("ltn12")
local socket_http = require("socket.http")
local ffiutil = require("ffi/util")

local PORT = 8787
local HOST = "127.0.0.1"
local BASE_URL = "http://" .. HOST .. ":" .. PORT

--- @class AndroidServer: Server
--- @field logBuffer string[]
local AndroidServer = {}

function AndroidServer:new(error_logs)
  return setmetatable({ logBuffer = error_logs or {} }, { __index = self })
end

function AndroidServer:request(request)
  local url = BASE_URL .. (request.path or "/")
  local method = request.method or "GET"
  local headers = {}

  if request.headers then
    for k, v in pairs(request.headers) do
      headers[k] = v
    end
  end

  local response = {}
  local request_table = {
    url = url,
    method = method,
    headers = headers,
    sink = ltn12.sink.table(response),
    timeout = request.timeout_seconds or 60,
  }

  if request.body then
    request_table.source = ltn12.source.string(request.body)
  end
  local ok, status = socket_http.request(request_table)

  if not ok then
    return {
      type = "ERROR",
      message = tostring(status or "connection refused"),
    }
  end

  return {
    type = "RESPONSE",
    status = status,
    body = table.concat(response),
  }
end

function AndroidServer:getLogBuffer()
  return self.logBuffer
end

function AndroidServer:stop()
  local android = require("android")
  android.openLink("rakuyomi_bridge://stop")
end

--- @class AndroidPlatform: Platform
local AndroidPlatform = {}
local function launch_android_service()
  local android = require("android")
  android.openLink("rakuyomi_bridge://start")

  return true
end

function AndroidPlatform:startServer()
  local temp_server = AndroidServer:new({ "Checking server status…" })

  local success, resp = pcall(function()
    return temp_server:request({
      path = "/health-check",
      timeout_seconds = 2,
    })
  end)

  local is_running = success and resp and resp.type == "RESPONSE"

  if not is_running then
    logger.info("Rakuyomi Bridge server not responding. Attempting to wake up service...")

    local launched = launch_android_service()

    if launched then
      ffiutil.sleep(1)

      success, resp = pcall(function()
        return temp_server:request({
          path = "/health-check",
          timeout_seconds = 3,
        })
      end)
      is_running = success and resp and resp.type == "RESPONSE"
    end
  end

  if not is_running then
    local error_lines = {
      "Could not connect to Rakuyomi Bridge server.",
      "Expected at " .. BASE_URL,
      "",
      "Please make sure the companion app is running:",
      "1. Open Rakuyomi Bridge app.",
      "2. Tap 'Start Server' manually.",
      "3. Disallow Battery Optimization for the app (Xiaomi/Huawei).",
      "",
      "Download Link:",
      "https://github.com/tachibana-shin/rakuyomi_bridge/releases",
    }
    return AndroidServer:new(error_lines)
  end

  logger.info("Successfully connected to Rakuyomi Bridge server at", BASE_URL)
  return AndroidServer:new()
end

return AndroidPlatform
