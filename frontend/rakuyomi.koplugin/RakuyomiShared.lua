local rapidjson = require("rapidjson")

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

return Shared
