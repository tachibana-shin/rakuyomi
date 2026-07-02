local logger = require('logger')
local Device = require('device')
local ffi = require('ffi')
local C = ffi.C
local ffiutil = require('ffi/util')
local Paths = require('Paths')
local util = require('frontend/util')
---@diagnostic disable-next-line: different-requires
local platformUtil = require('platform/util')
local must = platformUtil.must
local SubprocessOutputCapturer = platformUtil.SubprocessOutputCapturer
local rapidjson = require("rapidjson")
local execute_binary_fast = require("utils/executeBinaryFast")
local closeInheritedFDs = require("utils/closeInheritedFDs")

local SERVER_COMMAND_WORKING_DIRECTORY = os.getenv('RAKUYOMI_SERVER_WORKING_DIRECTORY')
local SERVER_COMMAND_OVERRIDE = os.getenv('RAKUYOMI_SERVER_COMMAND_OVERRIDE')
local REQUEST_COMMAND_WORKING_DIRECTORY = os.getenv('RAKUYOMI_UDS_HTTP_REQUEST_WORKING_DIRECTORY')
local REQUEST_COMMAND_OVERRIDE = os.getenv('RAKUYOMI_UDS_HTTP_REQUEST_COMMAND_OVERRIDE')

local SOCKET_PATH = '/tmp/rakuyomi.sock'

---@class UnixServer: Server
---@field private pid number
---@field private outputCapturer SubprocessOutputCapturer
---@field private logBuffer string[]
---@field private disable_logging boolean
local UnixServer = {}

function UnixServer:new(pid, outputCapturer)
  local disable_logging = G_reader_settings:isTrue("rakuyomi_disable_logging")

  local server = {
    pid = pid,
    outputCapturer = outputCapturer,
    logBuffer = {},
    maxLogLines = 100,
    disable_logging = disable_logging,
  }
  setmetatable(server, { __index = UnixServer })

  server:startLogCapture()

  return server
end

function UnixServer:getLogBuffer()
  self:flushLogBuffer()

  return self.logBuffer
end

function UnixServer:request(request)
  local requestWithDefaults = {
    socket_path = SOCKET_PATH,
    path = request.path,
    method = request.method or "GET",
    headers = request.headers or {},
    body = request.body or "",
    timeout_seconds = request.timeout_seconds or 60,
  }

  local requestJson = rapidjson.encode(requestWithDefaults)
  local udsHttpRequestCommand = REQUEST_COMMAND_OVERRIDE or (Paths.getPluginDirectory() .. "/uds_http_request")

  local responseJson, err = execute_binary_fast(udsHttpRequestCommand, requestJson, REQUEST_COMMAND_WORKING_DIRECTORY)

  if not responseJson or responseJson == "" then
    return { type = 'ERROR', message = err or "Rust binary returned empty output or crashed" }
  end

  local response, err2 = rapidjson.decode(responseJson)
  if err2 ~= nil then
    return { type = 'ERROR', message = err2 }
  end

  return response
end

function UnixServer:stop()
  local SIGTERM = 15

  logger.info("Terminating subprocess with PID " .. self.pid)
  must("kill", C.kill(self.pid, SIGTERM))
  local done = ffiutil.isSubProcessDone(self.pid, true)

  logger.info("Subprocess finished:", done)
end

function UnixServer:startLogCapture()
  if self.disable_logging then return end
  local onOutput = function(contents)
    self:handleLogOutput(contents)
  end

  self.outputCapturer:periodicallyPipeOutput(onOutput, onOutput)
end

function UnixServer:flushLogBuffer()
  if self.disable_logging then return end
  local onOutput = function(contents)
    self:handleLogOutput(contents)
  end

  self.outputCapturer:pipeOutput(onOutput, onOutput)
end

function UnixServer:handleLogOutput(contents)
  if self.disable_logging then return end
  local newLines = util.splitToArray(contents, '\n')
  for _, line in ipairs(newLines) do
    logger.info("Server output: " .. line)

    table.insert(self.logBuffer, line)
  end

  -- Keep only last 100 lines
  while #self.logBuffer > 100 do
    table.remove(self.logBuffer, 1)
  end
end

---@class GenericUnixPlatform: Platform
local GenericUnixPlatform = {}

function GenericUnixPlatform:startServer()
  -- setup loopback on Kobo devices (see #22)
  if Device:isKobo() then
    os.execute("ifconfig lo 127.0.0.1")
  end

  local serverCommand
  if SERVER_COMMAND_OVERRIDE ~= nil then
    serverCommand = util.splitToArray(SERVER_COMMAND_OVERRIDE, ' ')
  else
    serverCommand = { Paths.getPluginDirectory() .. "/server" }
  end

  local serverCommandWithArgs = {}
  util.arrayAppend(serverCommandWithArgs, serverCommand)
  util.arrayAppend(serverCommandWithArgs, { Paths.getHomeDirectory() })

  local capturer = SubprocessOutputCapturer:new()

  local pid = must("fork", C.fork())
  if pid == 0 then
    capturer:setupChildProcess()

    closeInheritedFDs(3)

    if SERVER_COMMAND_WORKING_DIRECTORY ~= nil then
      ffi.cdef([[
        int chdir(const char *) __attribute__((nothrow, leaf));
      ]])
      logger.info('changing directory to', SERVER_COMMAND_WORKING_DIRECTORY)
      C.chdir(SERVER_COMMAND_WORKING_DIRECTORY)
    end

    local exitCode = must(
      "execl",
      ---@diagnostic disable-next-line: deprecated
      C.execl(serverCommandWithArgs[1], unpack(serverCommandWithArgs, 1, #serverCommandWithArgs + 1))
    )

    logger.info("server exited with code " .. exitCode)
  end

  capturer:setupParentProcess()

  return UnixServer:new(pid, capturer)
end

return GenericUnixPlatform
