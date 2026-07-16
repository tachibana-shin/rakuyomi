local PdfDocument = require('document/pdfdocument')
local logger = require("logger")
local rapidjson = require("rapidjson")
local Paths = require('Paths')
local execute_binary_fast = require("utils/executeBinaryFast")

-- Environment variable for overriding the command
local CBZ_METADATA_READER_COMMAND_OVERRIDE = os.getenv('RAKUYOMI_CBZ_METADATA_READER_COMMAND_OVERRIDE')
local CBZ_METADATA_READER_COMMAND_WORKING_DIRECTORY = os.getenv('RAKUYOMI_CBZ_METADATA_READER_WORKING_DIRECTORY')


local function getString(metadata, key)
  local value = metadata[key]
  if type(value) == "string" then
    local trimmed = value:match("^%s*(.-)%s*$")
    return trimmed ~= "" and trimmed or nil
  end
  return nil
end

local function getNumber(metadata, key)
  local value = metadata[key]
  if type(value) == "number" then
    return value
  elseif type(value) == "string" then
    return tonumber(value)
  end
  return nil
end

local CbzDocument = PdfDocument:extend {
  -- Inherit properties and methods from PdfDocument
}

function CbzDocument:getDocumentProps()
  local base_props = PdfDocument.getDocumentProps(self)

  local json_content = self:_getComicBookInfoJSONFromBinary()
  if not json_content then
    logger.warn("CbzDocument: No JSON content received from binary.")
    return base_props
  end

  local info = self:_parseMetadata(json_content)
  if not info then
    logger.warn("CbzDocument: Failed to parse JSON content.")
    return base_props
  end

  -- Merge the parsed metadata with the base properties
  for key, value in pairs(info) do
    base_props[key] = value
  end

  return base_props
end

--- Calls the external Rust binary to get simplified metadata JSON.
--- @private
--- @return string|nil The JSON string or nil if an error occurred.
function CbzDocument:_getComicBookInfoJSONFromBinary()
  local file_path = self.file

  -- Determine the command to run
  local command_path
  if CBZ_METADATA_READER_COMMAND_OVERRIDE then
    command_path = CBZ_METADATA_READER_COMMAND_OVERRIDE
    logger.dbg("CbzDocument: Using overridden command:", command_path)
  else
    command_path = Paths.getPluginDirectory() .. "/cbz_metadata_reader"
    logger.dbg("CbzDocument: Using default command path:", command_path)
  end

  logger.dbg("CbzDocument: Executing binary via FFI:", command_path, "with file:", file_path)

  local json_content, err = execute_binary_fast(command_path, file_path, CBZ_METADATA_READER_COMMAND_WORKING_DIRECTORY)

  if not json_content or json_content == "" or json_content == "{}" then
    if err then
      logger.warn("CbzDocument: Command execution failed:", err)
    else
      logger.dbg("CbzDocument: Rust binary returned no valid JSON metadata for", file_path)
    end
    return nil
  end

  logger.dbg("CbzDocument: Successfully received JSON from binary for", file_path)
  return json_content
end

--- Parses the simplified metadata JSON content from the Rust binary.
--- @private
--- @param json_content string The JSON content to parse.
--- @return table|nil The parsed metadata table or nil if parsing failed.
function CbzDocument:_parseMetadata(json_content)
  -- Use rapidjson for decoding
  if not rapidjson or not rapidjson.decode then
    logger.warn("CbzDocument: rapidjson library/decode function not available, cannot parse metadata JSON.")
    return
  end

  -- Use pcall for safety when decoding JSON
  local ok, parsed_data = pcall(rapidjson.decode, json_content)

  if not ok or type(parsed_data) ~= "table" then
    logger.warn("CbzDocument: Failed to parse JSON with rapidjson or result is not a table:", parsed_data) -- Error message in parsed_data on failure
    return nil
  end

  local metadata = parsed_data
  local info = {}

  info.title = getString(metadata, "title")
  info.series = getString(metadata, "series")
  info.publisher = getString(metadata, "publisher")
  info.notes = getString(metadata, "notes")
  info.language = getString(metadata, "language")
  info.keywords = getString(metadata, "keywords")
  info.author = getString(metadata, "authors")
  info.series_index = getNumber(metadata, "series_index")

  local rating = getNumber(metadata, "rating")
  if rating and rating >= 0 then
    info.rating = rating
  end

  local pub_year = getNumber(metadata, "publication_year")
  if pub_year then
    info.publication_year = pub_year
  end

  return info
end

function CbzDocument:register(registry)
  registry:addProvider("cbz", "application/vnd.comicbook+zip", self, 110)
end

return CbzDocument
