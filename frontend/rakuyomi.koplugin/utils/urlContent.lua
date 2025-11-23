local logger = require("logger")
local http = require("socket.http")
local ltn12 = require("ltn12")
local socket = require("socket")
local socketutil = require("socketutil")
local socket_url = require("socket.url")

local function getUrlContent(url, timeout, maxtime)
  local parsed = socket_url.parse(url)
  if parsed.scheme ~= "http" and parsed.scheme ~= "https" then
    return false, "Unsupported protocol"
  end
  if not timeout then timeout = 10 end

  local sink = {}
  socketutil:set_timeout(timeout, maxtime or 30)
  local request = {
    url    = url,
    method = "GET",
    sink   = maxtime and socketutil.table_sink(sink) or ltn12.sink.table(sink),
  }

  local code, headers, status = socket.skip(1, http.request(request))
  socketutil:reset_timeout()
  local content = table.concat(sink) -- empty or content accumulated till now
  -- logger.dbg("code:", code)
  -- logger.dbg("headers:", headers)
  -- logger.dbg("status:", status)
  -- logger.dbg("#content:", #content)

  if code == socketutil.TIMEOUT_CODE or
      code == socketutil.SSL_HANDSHAKE_CODE or
      code == socketutil.SINK_TIMEOUT_CODE
  then
    logger.warn("request interrupted:", code)
    return false, code
  end
  if headers == nil then
    logger.warn("No HTTP headers:", status or code or "network unreachable")
    return false, "Network or remote server unavailable"
  end
  if not code or code < 200 or code > 299 then -- all 200..299 HTTP codes are OK
    logger.warn("HTTP status not okay:", status or code or "network unreachable")
    logger.dbg("Response headers:", headers)
    return false, "Remote server error or unavailable"
  end
  if headers and headers["content-length"] then
    -- Check we really got the announced content size
    local content_length = tonumber(headers["content-length"])
    if #content ~= content_length then
      return false, "Incomplete content received"
    end
  end

  return true, content
end

return getUrlContent
