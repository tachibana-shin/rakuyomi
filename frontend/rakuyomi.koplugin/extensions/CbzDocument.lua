local PdfDocument = require('document/pdfdocument')
local logger = require("logger")
local rapidjson = require("rapidjson")
local Paths = require('Paths')

-- Environment variable for overriding the command
local CBZ_METADATA_READER_COMMAND_OVERRIDE = os.getenv('RAKUYOMI_CBZ_METADATA_READER_COMMAND_OVERRIDE')
local CBZ_METADATA_READER_COMMAND_WORKING_DIRECTORY = os.getenv('RAKUYOMI_CBZ_METADATA_READER_WORKING_DIRECTORY')

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
  local json_content = nil

  -- Determine the command to run
  local command_path
  if CBZ_METADATA_READER_COMMAND_OVERRIDE then
    command_path = CBZ_METADATA_READER_COMMAND_OVERRIDE
    logger.dbg("CbzDocument: Using overridden command:", command_path)
  else
    command_path = Paths.getPluginDirectory() .. "/cbz_metadata_reader"
    logger.dbg("CbzDocument: Using default command path:", command_path)
  end

  -- Construct the command. Ensure the file path is properly quoted for the shell.
  local command = string.format("%s %q", command_path, file_path)
  logger.dbg("CbzDocument: Executing command:", command)

  if CBZ_METADATA_READER_COMMAND_WORKING_DIRECTORY then
    command = string.format("cd %q && %s", CBZ_METADATA_READER_COMMAND_WORKING_DIRECTORY, command)
  end

  -- Use io.popen to run the command and capture its standard output.
  local handle = io.popen(command, 'r')

  if not handle then
    logger.warn("CbzDocument: Failed to execute command:", command)
    return nil
  end

  -- Read all output from the command (should be the JSON string or '{}').
  json_content = handle:read("*a")
  local status, exit_code_or_signal, exit_code = handle:close() -- Check exit status

  -- Check status and output
  if not status or (exit_code and exit_code ~= 0) then
    logger.warn("CbzDocument: Command exited with non-zero status:", command, "Exit code:", exit_code, "Output:",
      json_content)

    return nil
  end

  -- Check if the output is empty or just '{}', which indicates no valid metadata found.
  if not json_content or json_content == "" or json_content == "{}" then
    logger.dbg("CbzDocument: Rust binary returned no valid JSON metadata for", file_path)
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

  -- The Rust binary now returns the flat structure directly.
  local metadata = parsed_data

  -- Helper functions to safely get string/number values
  -- Define helpers *before* they are used
  local function getString(key)
    local value = metadata[key]
    if type(value) == "string" then
      local trimmed = value:match("^%s*(.-)%s*$")
      return trimmed ~= "" and trimmed or nil
    end
    return nil
  end
  local function getNumber(key)
    local value = metadata[key]
    if type(value) == "number" then
      return value
    elseif type(value) == "string" then
      return tonumber(value) -- Attempt conversion if it's a string
    end
    return nil
  end

  local info

  -- Map fields from the simplified JSON to self.info
  info = {} -- Initialize local info table

  info.title = getString("title")
  info.series = getString("series")
  info.publisher = getString("publisher")
  info.notes = getString("notes")       -- Map 'notes' from JSON
  info.language = getString("language")
  info.keywords = getString("keywords") -- Map 'keywords' from JSON
  info.author = getString("authors")    -- Map 'authors' from JSON
  info.series_index = getNumber("series_index")

  local rating = getNumber("rating")
  if rating and rating >= 0 then
    info.rating = rating
  end

  local pub_year = getNumber("publication_year")
  if pub_year then
    info.publication_year = pub_year
  end

  return info
end

function CbzDocument:register(registry)
  registry:addProvider("cbz", "application/vnd.comicbook+zip", self, 110)
end

return CbzDocument
