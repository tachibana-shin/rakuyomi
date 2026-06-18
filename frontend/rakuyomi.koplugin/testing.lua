local C = require("ffi").C
local ffi = require("ffi")
require("ffi/posix_h")
local serpent = require("ffi/serpent")
local rapidjson = require("rapidjson")

local UIManager = require("ui/uimanager")
local logger = require("logger")

local NullTesting = {
  init = function() end,
  dumpVisibleUI = function() end,
  emitEvent = function(_, _) end
}

local Testing = {}

local function describeCurrentUI()
  local visible_windows = {}
  for i = #UIManager._window_stack, 0, -1 do
    local window = UIManager._window_stack[i]

    visible_windows[#visible_windows + 1] = window

    if window.widget.covers_fullscreen then
      break
    end
  end

  print("Got " .. #visible_windows .. " visible windows")

  local ignored_keys = {
    key_events = true,
    ges_events = true,
    _xshaping = true,
    face = true,
    koptinterface = true,
    deinflector = true,
    -- This technically helps the AI but is technically not UI
    -- and it takes like a shitload of context space
    item_table = true,
    -- This contains some cdata, which includes hashes. Those break
    -- some caching.
    ftsize = true,
  }

  local keyignore = {}
  local metatable = {}
  metatable.__index = function(_, key)
    if ignored_keys[key] then
      return true
    end

    if string.sub(key, 1, 1) == "_" then
      return true
    end

    return nil
  end

  setmetatable(keyignore, metatable)

  return serpent.block(visible_windows, {
    maxlevel = 15,
    indent = "  ",
    nocode = true,
    comment = false,
    keyignore = keyignore,
  })
end

function Testing:init()
  if self.initialized then
    return
  end

  self:setupIPC()
  self:hookOntoKeyPresses()
  self:periodicallyReadIPC()

  self.initialized = true
  logger.info("Testing hooks initialized!")
end

function Testing:setupIPC()
  local AF_UNIX = 1
  local SOCK_STREAM = 1

  ffi.cdef [[
    struct sockaddr_un {
      unsigned short sun_family;
      char sun_path[108];
    };
    int connect(int sockfd, const struct sockaddr *addr, unsigned int addrlen);
  ]]

  self.socket_fd = C.socket(AF_UNIX, SOCK_STREAM, 0)
  if self.socket_fd < 0 then
    local errno = ffi.errno()
    local err_msg = ffi.string(C.strerror(errno))
    logger.warn("Socket creation error: ", err_msg, " (errno: ", errno, ")")
    return
  end

  local addr = ffi.new("struct sockaddr_un")
  addr.sun_family = AF_UNIX
  local socket_path = "/tmp/rakuyomi_testing_ipc.sock"
  ffi.copy(addr.sun_path, socket_path)

  local addr_size = ffi.sizeof("struct sockaddr_un")
  if C.connect(self.socket_fd, ffi.cast("struct sockaddr *", addr), addr_size) < 0 then
    logger.warn("Failed to connect to socket at " .. socket_path)
    C.close(self.socket_fd)
    self.socket_fd = nil
    return
  end

  logger.info("Connected to IPC socket at " .. socket_path)
end

function Testing:periodicallyReadIPC()
  UIManager:scheduleIn(0.1, function()
    local data = self:_readNonBlockingFromIPC()
    if data then
      local success, decoded = pcall(rapidjson.decode, data)
      if success and decoded then
        if type(decoded) ~= "table" or type(decoded.type) ~= "string" then
          logger.warn("Invalid IPC message format")
          return self:periodicallyReadIPC()
        end

        -- Command handler mapping
        local commands = {
          dump_ui = function() self:dumpVisibleUI() end,
          install_source = function()
            local Backend = require("Backend")
            local source_id = decoded.params.source_id

            local response = Backend.installSource(source_id)
            if response.type == 'ERROR' then
              logger.warn("Failed to install source:", response.message)

              return
            end

            self:emitEvent("source_installed", {
              source_id = source_id,
            })
          end,
          ping = function() self:emitEvent("pong", { timestamp = os.time() }) end
        }

        local handler = commands[decoded.type]
        if handler then
          local ok, err = pcall(handler)
          if not ok then
            logger.warn("Error handling command", decoded.type, err)
          end
        else
          logger.warn("Unknown command type:", decoded.type)
        end
      else
        logger.warn("Failed to decode IPC message:", data)
      end
    end
    self:periodicallyReadIPC()
  end)
end

---@return string|nil
function Testing:_readNonBlockingFromIPC()
  if not self.socket_fd then
    return nil
  end

  -- Set up poll
  local pfd = ffi.new("struct pollfd")
  pfd.fd = self.socket_fd
  pfd.events = C.POLLIN

  -- Poll with zero timeout for non-blocking behavior
  local ret = C.poll(pfd, 1, 0)

  if ret < 0 then
    -- Poll error
    local errno = ffi.errno()
    local err_msg = ffi.string(C.strerror(errno))
    logger.warn("Poll error: ", err_msg, " (errno: ", errno, ")")
    C.close(self.socket_fd)
    self.socket_fd = nil
    return nil
  elseif ret == 0 then
    -- No data available
    return nil
  end

  -- Data is available, read it
  local buffer = ffi.new("char[?]", 1024)
  local bytes_read = C.read(self.socket_fd, buffer, 1024)

  if bytes_read <= 0 then
    -- Connection closed or error
    local errno = ffi.errno()
    if bytes_read < 0 then
      local err_msg = ffi.string(C.strerror(errno))
      logger.warn("Socket read error: ", err_msg, " (errno: ", errno, ")")
    else
      logger.info("IPC connection closed by peer")
    end
    C.close(self.socket_fd)
    self.socket_fd = nil
    return nil
  end

  return ffi.string(buffer, bytes_read)
end

function Testing:dumpVisibleUI()
  logger.info("Dumping visible UI")

  local ui_contents = describeCurrentUI()

  self:emitEvent('ui_contents', {
    contents = ui_contents
  })
end

--- @param name string
--- @param params table|nil
function Testing:emitEvent(name, params)
  if not self.socket_fd then
    logger.warn("No socket connection available. Cannot emit event.")
    return
  end

  local json_message = {
    type = name,
    params = params,
  }
  local message = rapidjson.encode(json_message)

  C.write(self.socket_fd, message .. '\n', #message + 1)
end

---@private
function Testing:hookOntoKeyPresses()
  local oldSendEvent = UIManager.sendEvent
  UIManager.sendEvent = function(newSelf, event)
    if event.handler == "onKeyPress" then
      if self:onKeyPress(event.args[1]) then
        return
      end
    end

    oldSendEvent(newSelf, event)
  end
end

---@private
function Testing:onKeyPress(key)
  if key.Shift and key.F8 then
    self:dumpVisibleUI()

    return true
  elseif key.Shift and key.F9 then
    local LibraryView = require("LibraryView")

    LibraryView:fetchAndShow()

    return true
  end
end

return os.getenv('RAKUYOMI_IS_TESTING') == '1' and Testing or NullTesting
