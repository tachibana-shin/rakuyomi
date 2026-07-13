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

local SERVER_COMMAND_WORKING_DIRECTORY = os.getenv('RAKUYOMI_SERVER_WORKING_DIRECTORY')
local SERVER_COMMAND_OVERRIDE = os.getenv('RAKUYOMI_SERVER_COMMAND_OVERRIDE')
local REQUEST_COMMAND_WORKING_DIRECTORY = os.getenv('RAKUYOMI_UDS_HTTP_REQUEST_WORKING_DIRECTORY')
local REQUEST_COMMAND_OVERRIDE = os.getenv('RAKUYOMI_UDS_HTTP_REQUEST_COMMAND_OVERRIDE')

local SOCKET_PATH = '/tmp/rakuyomi.sock'

ffi.cdef([[
  char *getcwd(char *buf, size_t size);
  extern char **environ;

  typedef struct { char __pad[128]; } posix_spawn_file_actions_t;
  int posix_spawn(int *pid, const char *path, const posix_spawn_file_actions_t *file_actions, const void *attrp, const char *const argv[], char *const envp[]);
  int posix_spawn_file_actions_init(posix_spawn_file_actions_t *actions);
  int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *actions, int fd, int newfd);
  int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *actions, int fd);
  int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *actions);
]])

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


local t_int_array = ffi.typeof("int[1]")
local t_file_actions = ffi.typeof("posix_spawn_file_actions_t")

function GenericUnixPlatform:startServer()
  if Device:isKobo() then
    os.execute("ifconfig lo 127.0.0.1")
  end

  local capturer = SubprocessOutputCapturer:new()
  local binaryPath
  local argv

  if SERVER_COMMAND_OVERRIDE ~= nil then
    local serverCommand = util.splitToArray(SERVER_COMMAND_OVERRIDE, ' ')
    local args = {}
    util.arrayAppend(args, serverCommand)
    util.arrayAppend(args, { Paths.getHomeDirectory() })

    binaryPath = args[1]
    argv = ffi.new("const char *[?]", #args + 1)
    for i, arg in ipairs(args) do
      argv[i - 1] = arg
    end
    argv[#args] = nil
  else
    binaryPath = Paths.getPluginDirectory() .. "/server"
    argv = ffi.new("const char *[3]")
    argv[0] = binaryPath
    argv[1] = Paths.getHomeDirectory()
    argv[2] = nil
  end

  local actions = t_file_actions()
  must("posix_spawn_file_actions_init", C.posix_spawn_file_actions_init(actions))

  if capturer.stdout_pipe and capturer.stderr_pipe then
    must("posix_spawn_file_actions_adddup2", C.posix_spawn_file_actions_adddup2(actions, capturer.stdout_pipe[1], 1))
    must("posix_spawn_file_actions_adddup2", C.posix_spawn_file_actions_adddup2(actions, capturer.stderr_pipe[1], 2))

    must("posix_spawn_file_actions_addclose", C.posix_spawn_file_actions_addclose(actions, capturer.stdout_pipe[0]))
    must("posix_spawn_file_actions_addclose", C.posix_spawn_file_actions_addclose(actions, capturer.stderr_pipe[0]))

    must("posix_spawn_file_actions_addclose", C.posix_spawn_file_actions_addclose(actions, capturer.stdout_pipe[1]))
    must("posix_spawn_file_actions_addclose", C.posix_spawn_file_actions_addclose(actions, capturer.stderr_pipe[1]))
  end

  local old_dir = nil
  if SERVER_COMMAND_WORKING_DIRECTORY ~= nil then
    local buf = ffi.new("char[4096]")
    if C.getcwd(buf, 4096) ~= nil then
      old_dir = ffi.string(buf)
      C.chdir(SERVER_COMMAND_WORKING_DIRECTORY)
    end
  end

  local pid_ptr = t_int_array()
  local spawn_res = C.posix_spawn(pid_ptr, binaryPath, actions, nil, argv, C.environ)

  if old_dir ~= nil then
    C.chdir(old_dir)
  end

  C.posix_spawn_file_actions_destroy(actions)


  local pid = must("posix_spawn", spawn_res == 0 and pid_ptr[0] or -1)

  capturer:setupParentProcess()

  return UnixServer:new(pid, capturer)
end

return GenericUnixPlatform
