local logger = require('logger')
local Device = require('device')
local ffi = require('ffi')
local C = ffi.C
local ffiutil = require('ffi/util')
local bit = require('bit')
local Paths = require('Paths')
local util = require('frontend/util')
---@diagnostic disable-next-line: different-requires
local platformUtil = require('platform/util')
local must = platformUtil.must
local must0 = platformUtil.must0
local SubprocessOutputCapturer = platformUtil.SubprocessOutputCapturer

local SERVER_COMMAND_WORKING_DIRECTORY = os.getenv('RAKUYOMI_SERVER_WORKING_DIRECTORY')
local SERVER_COMMAND_OVERRIDE = os.getenv('RAKUYOMI_SERVER_COMMAND_OVERRIDE')

local SOCKET_PATH = '/tmp/rakuyomi.sock'

ffi.cdef([[
  char *getcwd(char *buf, size_t size);
  extern char **environ;

  typedef struct { uint64_t __pad[16]; } posix_spawn_file_actions_t;
  int posix_spawn(int *pid, const char *path, const posix_spawn_file_actions_t *file_actions, const void *attrp, const char *const argv[], char *const envp[]);
  int posix_spawn_file_actions_init(posix_spawn_file_actions_t *actions);
  int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *actions, int fd, int newfd);
  int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *actions, int fd);
  int posix_spawn_file_actions_addopen(posix_spawn_file_actions_t *actions, int fd, const char *path, int oflag, int mode);
  int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *actions);
]])

pcall(ffi.cdef, [[
  struct sockaddr_un {
    unsigned short sun_family;
    char sun_path[108];
  };
  int socket(int domain, int type, int protocol);
  int connect(int sockfd, const void *addr, unsigned int addrlen);
  int fcntl(int fd, int cmd, ...);
  struct timeval {
    long tv_sec;
    long tv_usec;
  };
  int setsockopt(int sockfd, int level, int optname, const void *optval, unsigned int optlen);
  int getsockopt(int sockfd, int level, int optname, void *optval, unsigned int *optlen);
]])

local AF_UNIX = 1
local SOCK_STREAM = 1
local EINTR = 4
local EAGAIN = 11
local EINPROGRESS = 115
local READ_BUF_SIZE = 4096
local F_SETFL = 4
local O_NONBLOCK = 0x4
local F_GETFL = 3
local SOL_SOCKET = 1
local SO_SNDTIMEO = 21
local SO_RCVTIMEO = 20

local t_sockaddr = ffi.typeof("struct sockaddr_un")
local t_readbuf = ffi.typeof("char[?]")
local t_charptr = ffi.typeof("const char *")

--- Write all bytes to a file descriptor, handling partial writes.
---@param fd number
---@param data string
---@param len number
---@param timeout_secs number
---@return boolean ok
---@return string|nil err
local function write_all(fd, data, len, timeout_secs)
  local ptr = ffi.cast(t_charptr, data)
  local total = 0
  local timeout_ms = math.floor(timeout_secs * 1000)
  local pfd = ffi.new("struct pollfd")
  pfd.fd = fd
  pfd.events = 4 -- POLLOUT

  while total < len do
    -- Wait for socket to be writable with timeout
    local ret = C.poll(pfd, 1, timeout_ms)
    if ret < 0 then
      if ffi.errno() ~= EINTR then
        return false, ffi.string(C.strerror(ffi.errno()))
      end
    elseif ret == 0 then
      return false, "write timed out"
    else
      local n = C.write(fd, ptr + total, len - total)
      if n > 0 then
        total = total + n
      elseif n < 0 then
        local err = ffi.errno()
        if err ~= EINTR and err ~= EAGAIN then
          return false, ffi.string(C.strerror(err))
        end
      else
        return false, "write returned 0"
      end
    end
  end
  return true, nil
end

--- Read from fd until EOF (server closes connection via Connection: close).
--- Uses a pre-allocated buffer to minimize GC pressure during the read loop.
---@param fd number
---@param timeout_secs number
---@return string|nil data
---@return string|nil err
local function read_until_eof(fd, timeout_secs)
  local timeout_ms = math.floor(timeout_secs * 1000)
  local chunks = {}
  local buf = t_readbuf(READ_BUF_SIZE)
  local pfd = ffi.new("struct pollfd")
  pfd.fd = fd
  pfd.events = 1 -- POLLIN

  while true do
    local ret = C.poll(pfd, 1, timeout_ms)
    if ret < 0 then
      if ffi.errno() ~= EINTR then
        return nil, ffi.string(C.strerror(ffi.errno()))
      end
    elseif ret == 0 then
      return nil, "read timed out"
    else
      local n = C.read(fd, buf, READ_BUF_SIZE)
      if n > 0 then
        chunks[#chunks + 1] = ffi.string(buf, n)
      elseif n == 0 then
        break -- EOF
      else
        if ffi.errno() ~= EINTR then
          return nil, ffi.string(C.strerror(ffi.errno()))
        end
      end
    end
  end

  return table.concat(chunks), nil
end

--- Extract status code and body from a raw HTTP response string.
---@param raw string
---@return number|nil status
---@return string|nil body
local function parse_http_response(raw)
  local sep = string.find(raw, "\r\n\r\n", 1, true)
  if not sep then
    return nil, nil
  end

  local status_line_end = string.find(raw, "\r\n", 1, true)
  local status_line = string.sub(raw, 1, status_line_end - 1)
  local status = tonumber(string.match(status_line, "HTTP/%d%.%d (%d+)"))
  local body = string.sub(raw, sep + 4)

  return status, body
end

--- Sanitize HTTP header value by removing CR/LF characters
---@param value string
---@return string
local function sanitize_http_value(value)
  -- Remove \r and \n to prevent header injection
  return string.gsub(string.gsub(value, "\r", ""), "\n", "")
end

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
  local method = request.method or "GET"
  local path = request.path
  local body = request.body or ""
  local headers = request.headers or {}
  local timeout = request.timeout_seconds or 60

  -- Validate path is present
  if not path or path == "" then
    return { type = 'ERROR', message = "request path is required" }
  end

  -- Sanitize path to prevent header injection
  path = sanitize_http_value(path)

  local fd = C.socket(AF_UNIX, SOCK_STREAM, 0)
  if fd < 0 then
    return { type = 'ERROR', message = "socket(): " .. ffi.string(C.strerror(ffi.errno())) }
  end

  -- Set socket timeouts for send and receive operations
  local tv = ffi.new("struct timeval")
  tv.tv_sec = timeout
  tv.tv_usec = 0
  C.setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, tv, ffi.sizeof(tv))
  C.setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, tv, ffi.sizeof(tv))

  local addr = ffi.new(t_sockaddr)
  addr.sun_family = AF_UNIX
  ffi.copy(addr.sun_path, SOCKET_PATH)

  -- Make socket non-blocking for connect timeout
  local flags = C.fcntl(fd, F_GETFL, 0)
  if flags >= 0 then
    C.fcntl(fd, F_SETFL, bit.bor(flags, O_NONBLOCK))
  end

  local connect_result = C.connect(fd, ffi.cast("struct sockaddr *", addr), ffi.sizeof(t_sockaddr))
  if connect_result < 0 then
    local err = ffi.errno()
    if err ~= EINPROGRESS then
      local err_msg = ffi.string(C.strerror(err))
      C.close(fd)
      return { type = 'ERROR', message = "connect(): " .. err_msg }
    end

    -- Wait for connect to complete with timeout
    local pfd = ffi.new("struct pollfd")
    pfd.fd = fd
    pfd.events = 4 -- POLLOUT
    local ret = C.poll(pfd, 1, math.floor(timeout * 1000))
    if ret <= 0 then
      C.close(fd)
      return { type = 'ERROR', message = ret == 0 and "connect timed out" or "connect poll failed" }
    end

    -- Check if connect succeeded
    local so_error = ffi.new("int[1]")
    local so_error_len = ffi.new("unsigned int[1]", ffi.sizeof("int"))
    if C.getsockopt(fd, SOL_SOCKET, 4, so_error, so_error_len) == 0 and so_error[0] ~= 0 then
      local err_msg = ffi.string(C.strerror(so_error[0]))
      C.close(fd)
      return { type = 'ERROR', message = "connect(): " .. err_msg }
    end
  end

  -- Restore blocking mode for simpler write/read logic
  if flags >= 0 then
    C.fcntl(fd, F_SETFL, flags)
  end

  -- Build raw HTTP/1.1 request
  local req = method .. " " .. path .. " HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n"

  -- Check if Content-Length header already exists (to avoid duplication)
  local has_content_length = false
  for k, _ in pairs(headers) do
    if string.lower(k) == "content-length" then
      has_content_length = true
      break
    end
  end

  -- Only add Content-Length if not already present and body exists
  if #body > 0 and not has_content_length then
    req = req .. "Content-Length: " .. #body .. "\r\n"
  end

  -- Add other headers with sanitization
  for k, v in pairs(headers) do
    req = req .. sanitize_http_value(k) .. ": " .. sanitize_http_value(tostring(v)) .. "\r\n"
  end
  req = req .. "\r\n" .. body

  local wok, werr = write_all(fd, req, #req, timeout)
  if not wok then
    C.close(fd)
    return { type = 'ERROR', message = "write: " .. werr }
  end

  local raw, rerr = read_until_eof(fd, timeout)
  C.close(fd)

  if not raw or raw == "" then
    return { type = 'ERROR', message = rerr or "empty response from server" }
  end

  local status, resp_body = parse_http_response(raw)
  if not resp_body then
    return { type = 'ERROR', message = "malformed HTTP response" }
  end

  return {
    type = 'RESPONSE',
    status = status or 0,
    body = resp_body,
  }
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

-- O_WRONLY | O_CREAT | O_TRUNC for /dev/null redirection
local O_WRONLY = 1
local O_CREAT = 64
local O_TRUNC = 512

function GenericUnixPlatform:startServer()
  if Device:isKobo() then
    os.execute("ifconfig lo 127.0.0.1")
  end

  local disable_logging = G_reader_settings:isTrue("rakuyomi_disable_logging")

  local capturer
  if not disable_logging then
    capturer = SubprocessOutputCapturer:new()
  end

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
  must0("posix_spawn_file_actions_init", C.posix_spawn_file_actions_init(actions))

  if disable_logging then
    -- OS-level redirection: stdout and stderr go to /dev/null.
    -- No pipes created, no SubprocessOutputCapturer, zero overhead.
    must0("addopen stdout", C.posix_spawn_file_actions_addopen(actions, 1, "/dev/null", bit.bor(O_WRONLY, O_CREAT, O_TRUNC), 0))
    must0("addopen stderr", C.posix_spawn_file_actions_addopen(actions, 2, "/dev/null", bit.bor(O_WRONLY, O_CREAT, O_TRUNC), 0))
  else
    must0("posix_spawn_file_actions_adddup2", C.posix_spawn_file_actions_adddup2(actions, capturer.stdout_pipe[1], 1))
    must0("posix_spawn_file_actions_adddup2", C.posix_spawn_file_actions_adddup2(actions, capturer.stderr_pipe[1], 2))

    must0("posix_spawn_file_actions_addclose", C.posix_spawn_file_actions_addclose(actions, capturer.stdout_pipe[0]))
    must0("posix_spawn_file_actions_addclose", C.posix_spawn_file_actions_addclose(actions, capturer.stderr_pipe[0]))

    must0("posix_spawn_file_actions_addclose", C.posix_spawn_file_actions_addclose(actions, capturer.stdout_pipe[1]))
    must0("posix_spawn_file_actions_addclose", C.posix_spawn_file_actions_addclose(actions, capturer.stderr_pipe[1]))
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


  must0("posix_spawn", spawn_res)
  local pid = pid_ptr[0]

  if capturer then
    capturer:setupParentProcess()
  end

  return UnixServer:new(pid, capturer)
end

return GenericUnixPlatform
