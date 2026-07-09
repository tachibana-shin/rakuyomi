local rapidjson = require("rapidjson")
local ChapterListing = require("ChapterListing")
local LibraryView = require("LibraryView")
local Backend = require("Backend")
local Trapper = require("ui/trapper")

local Shared = {}

--- @param filepath string
--- @return string | nil, string | nil
--- @private
function Shared:getZipComment(filepath)
  local f, err = io.open(filepath, "rb")
  if not f then return nil, err end

  local size = f:seek("end")

  -- EOCD max 65535 + 22 marker
  local read_size = math.min(size, 65535 + 22)

  f:seek("set", size - read_size)
  local data = f:read(read_size)
  f:close()

  if not data then return nil, "Cannot read file data" end

  -- Scan back to find EOCD signature: 0x06054b50 (byte string: "PK\005\006")
  for i = read_size - 21, 1, -1 do
    if data:sub(i, i + 3) == "\x50\x4b\x05\x06" then
      -- Read 2 bytes define comment length (at offset 20 and 21 from signature)
      local len_low = data:byte(i + 20)
      local len_high = data:byte(i + 21)
      local comment_len = len_low + (len_high * 256)

      -- Check integrity: EOCD position + comment length = file size
      if i + 21 + comment_len == read_size then
        if comment_len > 0 then
          return data:sub(i + 22, i + 21 + comment_len)
        else
          return ""           -- ZIP file valid but no comment
        end
      end
    end
  end

  return nil, "Not found EOCD"
end

---@param filepath string
---@return ChapterId | nil
function Shared:getOrigin(filepath)
  local comment = Shared:getZipComment(filepath)
  if not comment or comment == "" then return nil end

  -- if exits comment is json format {"chapter_id":"321","manga_id":"tom-lai-la-em-de-thuong-duoc-chua-4746","source_id":"vi.truyenqq"}
  local ok, data = pcall(function()
    return rapidjson.decode(comment)
  end)

  if not ok then return nil end
  if type(data) ~= "table"
      or type(data.chapter_id) ~= "string"
      or type(data.manga_id) ~= "string"
      or type(data.source_id) ~= "string" then
    return nil
  end

  return {
    chapter_id = data.chapter_id,
    manga_id = {
      manga_id = data.manga_id,
      source_id = data.source_id,
    },
  }
end

--- Opens the ChapterListing view for the manga owning a CBZ file.
--- When the user closes ChapterListing, returns to LibraryView.
--- Called from other plugins via the filepath of a Rakuyomi-downloaded CBZ.
--- @param filepath string Path to a CBZ file with Rakuyomi ZIP comment
--- @param hideTopClose boolean? If set, the top close button will be hidden.
--- @return boolean true if ChapterListing was opened, false if file has no origin metadata
function Shared:openChapterListingFromFile(filepath, hideTopClose)
  Backend.getBackend()

  if not Backend.getInitialized() then
    self:showErrorDialog()

    return false
  end

  local origin = Shared:getOrigin(filepath)
  if not origin then return false end

  ---@type Manga
  ---@description The fake data
  local manga = {
    id = origin.manga_id.manga_id,
    source = { id = origin.manga_id.source_id, name = "", version = 0 },
    title = "",
    in_library = false,
  }

  local focus_manga_id = origin.manga_id.manga_id
  local focus_manga_source_id = origin.manga_id.source_id

  Trapper:wrap(function()
    ChapterListing:fetchAndShow(manga, function()
      LibraryView:fetchAndShow(nil, nil, {
        hideTopClose = hideTopClose,
        focus_manga_id = focus_manga_id,
        focus_manga_source_id = focus_manga_source_id,
      })
    end, true, origin.chapter_id)
  end)

  return true
end

return Shared
