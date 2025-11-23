local Trapper = require("ui/trapper")
local UIManager = require("ui/uimanager")

local getUrlContent = require("utils/urlContent")

--- @class ImageLoader
--- @field loading boolean
--- @field url_map table<string, boolean>
--- @field callback fun(content: string)
local ImageLoader = {
  loading = false,
  url_map = {},
}

function ImageLoader:new()
  local obj = setmetatable({}, self)
  return obj
end

--- @param url string
function ImageLoader:loadImage(url)
  if ImageLoader.loading then
    error("batch already in progress")
  end

  self.loading = true

  Trapper:wrap(function()
    local completed, success, content = Trapper:dismissableRunInSubprocess(function()
      return getUrlContent(url, 10, 30)
    end)

    --if not completed then
    --  logger.warn("Aborted")
    --end

    if completed and success then
      self.callback(content)
    end

    self.loading = false
  end)
end
