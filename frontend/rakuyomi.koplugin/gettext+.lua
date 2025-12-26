local logger = require("logger")
local buffer = require("string.buffer")
local ffi = require("ffi")
local C = ffi.C

require "table.new"
require "ffi/posix_h"

local GetText = {
  context = {},
  translation = {},
  current_lang = "C",
  dirname = require("datastorage"):getFullDataDir() .. "/plugins/rakuyomi.koplugin/l10n",
  textdomain = "koreader",
  plural_default = "n != 1",
}

local GetText_mt = {
  __index = {}
}
GetText.wrapUntranslated_nowrap = function(text) return text end
GetText.wrapUntranslated = GetText.wrapUntranslated_nowrap

function GetText_mt.__call(gettext, msgid)
  return gettext.translation[msgid] and gettext.translation[msgid][0] or gettext.translation[msgid] or
  gettext.wrapUntranslated(msgid)
end

--- Converts C logical operators to Lua.
local function logicalCtoLua(logical_str)
  logical_str = logical_str:gsub("&&", "and")
  logical_str = logical_str:gsub("!=", "~=")
  logical_str = logical_str:gsub("||", "or")
  return logical_str
end

--- Default getPlural function.
local function getDefaultPlural(n)
  if n ~= 1 then
    return 1
  else
    return 0
  end
end

--- Generates a proper Lua function out of logical gettext math tests.
local function getPluralFunc(pl_tests, nplurals, plural_default)
  -- the return function() stuff is a bit of loadstring trickery
  local plural_func_str = "return function(n) if "

  if #pl_tests > 1 then
    for i = 1, #pl_tests do
      local pl_test = pl_tests[i]
      pl_test = logicalCtoLua(pl_test)

      if i > 1 and tonumber(pl_test) == nil then
        pl_test = " elseif " .. pl_test
      end
      if tonumber(pl_test) ~= nil then
        -- no condition, just a number
        pl_test = " else return " .. pl_test
      end
      pl_test = pl_test:gsub("?", " then return")

      -- append to plural function
      plural_func_str = plural_func_str .. pl_test
    end
    plural_func_str = plural_func_str .. " end end"
  else
    local pl_test = pl_tests[1]
    -- Ensure JIT compiled function if we're dealing with one of the many simpler languages.
    -- After all, loadstring won't be.
    -- Potential workaround: write to file and use require.
    if pl_test == plural_default then
      return getDefaultPlural
    end
    -- language with no plural forms
    if tonumber(pl_test) ~= nil then
      plural_func_str = "return function(n) return " .. pl_test .. " end"
    else
      pl_test = logicalCtoLua(pl_test)
      plural_func_str = "return function(n) if " .. pl_test .. " then return 1 else return 0 end end"
    end
  end
  logger.dbg("gettext: plural function", plural_func_str)
  return loadstring(plural_func_str)()
end

local function addTranslation(msgctxt, msgid, msgstr, n)
  assert(not msgctxt or msgctxt ~= "")
  assert(msgid and msgid ~= "")
  assert(msgstr)
  if msgstr == "" then
    return
  end
  if msgctxt then
    if not GetText.context[msgctxt] then
      GetText.context[msgctxt] = {}
    end
    if n then
      if not GetText.context[msgctxt][msgid] then
        GetText.context[msgctxt][msgid] = {}
      end
      GetText.context[msgctxt][msgid][n] = msgstr
    else
      GetText.context[msgctxt][msgid] = msgstr
    end
  else
    if n then
      if not GetText.translation[msgid] then
        GetText.translation[msgid] = {}
      end
      GetText.translation[msgid][n] = msgstr
    else
      GetText.translation[msgid] = msgstr
    end
  end
end

function GetText_mt.__index.changeLang(new_lang)
  if new_lang == "vi_VN" then
    new_lang = "vi"
  end
  GetText.context = {}
  GetText.translation = {}
  GetText.current_lang = "C"

  -- the "C" locale disables localization altogether
  -- can be various things such as `en_US` or `en_US:en`
  if new_lang == "C" or new_lang == nil or new_lang == ""
      or new_lang:match("^en_US") == "en_US" then
    return
  end

  -- strip encoding suffix in locale like "zh_CN.utf8"
  new_lang = new_lang:sub(1, new_lang:find(".%."))

  local mo = GetText.dirname .. "/" .. new_lang .. "/" .. GetText.textdomain .. ".mo"
  print(mo)
  if not GetText.loadMO(mo) then
    return false
  end

  GetText.current_lang = new_lang
  return true
end

local function parse_headers(headers)
  local plural_forms = headers:match("Plural%-Forms: (.*)")
  local nplurals = plural_forms:match("nplurals=([0-9]+);") or 2
  local plurals = plural_forms:match("plural=%((.*)%);")

  -- Hardcoded workaround for Hebrew which has 4 plural forms.
  if plurals == "n == 1) ? 0 : ((n == 2) ? 1 : ((n > 10 && n % 10 == 0) ? 2 : 3)" then
    plurals = "n == 1 ? 0 : (n == 2) ? 1 : (n > 10 && n % 10 == 0) ? 2 : 3"
  end
  -- Hardcoded workaround for Latvian.
  if plurals == "n % 10 == 0 || n % 100 >= 11 && n % 100 <= 19) ? 0 : ((n % 10 == 1 && n % 100 != 11) ? 1 : 2" then
    plurals = "n % 10 == 0 || n % 100 >= 11 && n % 100 <= 19 ? 0 : (n % 10 == 1 && n % 100 != 11) ? 1 : 2"
  end
  -- Hardcoded workaround for Romanian which has 3 plural forms.
  if plurals == "n == 1) ? 0 : ((n == 0 || n != 1 && n % 100 >= 1 && n % 100 <= 19) ? 1 : 2" then
    plurals = "n == 1 ? 0 : (n == 0 || n != 1 && n % 100 >= 1 && n % 100 <= 19) ? 1 : 2"
  end

  if not plurals then
    -- Some languages (e.g., Arabic) may not use parentheses.
    -- However, the following more inclusive match is more likely
    -- to accidentally include junk and seldom relevant.
    -- We might also be dealing with a language without plurals.
    -- That would look like `plural=0`.
    plurals = plural_forms:match("plural=(.*);")
  end

  if plurals:find("[^n!=%%<>&:%(%)|?0-9 ]") then
    -- we don't trust this input, go with default instead
    plurals = GetText.plural_default
  end

  local pl_tests = {}
  for pl_test in plurals:gmatch("[^:]+") do
    table.insert(pl_tests, pl_test)
  end

  GetText.getPlural = getPluralFunc(pl_tests, nplurals, GetText.plural_default)
  if not GetText.getPlural then
    GetText.getPlural = getDefaultPlural
  end
end

local MO_MAGIC = 0x950412de

function GetText_mt.__index.loadMO(file)
  local fd = C.open(file, C.O_RDONLY)
  if fd < 0 then
    logger.dbg(string.format("cannot open translation file: %s", file))
    return false
  end
  local strerror = function()
    return ffi.string(C.strerror(ffi.errno()))
  end
  local seek_and_read = function(off, ptr, len)
    local ret
    ret = C.lseek(fd, off, C.SEEK_SET)
    if ret ~= off then
      logger.err(string.format("loading translation file failed: %s [%s]", file, ret < 0 and strerror() or "lseek"))
      return false
    end
    ret = C.read(fd, ptr, len)
    if ret ~= len then
      logger.err(string.format("loading translation file failed: %s [%s]", file), ret < 0 and strerror() or "short read")
      return false
    end
    return true
  end
  local mo_hdr = ffi.new("struct mo_header")
  if not seek_and_read(0, mo_hdr, ffi.sizeof(mo_hdr)) then
    C.close(fd)
    return false
  end
  if mo_hdr.magic ~= MO_MAGIC then
    logger.err(string.format("bad translation file: %s [magic]", file))
    C.close(fd)
    return false
  end
  if mo_hdr.revision_major ~= 0 then
    logger.err(string.format("bad translation file: %s [revision]", file))
    C.close(fd)
    return false
  end
  local table_buf = buffer:new()
  local table_size = mo_hdr.nb_strings * ffi.sizeof("struct mo_string_table")
  local table_ptr = table_buf:reserve(table_size)
  local read_strings_count
  local read_strings = function(check_for_context)
    local m_str_tbl = ffi.cast("struct mo_string_table *", table_ptr)
    local str_buf = buffer:new()
    read_strings_count = -1
    return function()
      read_strings_count = read_strings_count + 1
      if read_strings_count >= mo_hdr.nb_strings then
        return
      end
      local str_len = m_str_tbl[read_strings_count].length
      local str_off = m_str_tbl[read_strings_count].offset
      local str_ptr = str_buf:reserve(str_len)
      if not seek_and_read(str_off, str_ptr, str_len) then
        return
      end
      local ctx
      local pos = 0
      if check_for_context then
        -- 4: ␄ (End of Transmission).
        local p = C.memchr(str_ptr, 4, str_len)
        if p ~= nil then
          local l = ffi.cast("ssize_t", p) - ffi.cast("ssize_t", str_ptr)
          ctx = ffi.string(str_ptr, l)
          pos = l + 1
        end
      end
      local l = C.strnlen(str_ptr + pos, str_len - pos)
      if l + pos < str_len then
        -- Plurals!
        local strings = { ffi.string(str_ptr + pos, l) }
        pos = pos + l + 1
        while pos < str_len do
          l = C.strnlen(str_ptr + pos, str_len - pos)
          table.insert(strings, ffi.string(str_ptr + pos, l))
          pos = pos + l + 1
        end
        return read_strings_count + 1, strings, ctx
      else
        return read_strings_count + 1, ffi.string(str_ptr + pos, str_len - pos), ctx
      end
    end
  end
  -- Read original strings.
  if not seek_and_read(mo_hdr.original_strings_table_offset, table_ptr, table_size) then
    C.close(fd)
    return false
  end
  local original_context = {}
---@diagnostic disable-next-line: undefined-field
  local original_strings = table.new(mo_hdr.nb_strings, 0)
  for n, s, ctx in read_strings(true) do
    if ctx then
      original_context[n] = ctx
    end
    original_strings[n] = s
  end
  if read_strings_count ~= mo_hdr.nb_strings then
    C.close(fd)
    return false
  end
  -- Read translated strings.
  if not seek_and_read(mo_hdr.translated_strings_table_offset, table_ptr, table_size) then
    C.close(fd)
    return false
  end
  for n, ts in read_strings() do
    local ctx = original_context[n]
    local os = original_strings[n]
    if type(os) == "table" then
      if type(ts) == "table" then
        for pn, pts in ipairs(ts) do
          addTranslation(ctx, os[1], pts, pn - 1)
        end
      else
        addTranslation(ctx, os[1], ts, 0)
      end
    elseif type(ts) == "table" then
      logger.warn(string.format("bad translation file: %s [singular / plurals mismatch]", file))
    else
      if n == 1 and #os == 0 then
        parse_headers(ts)
      else
        addTranslation(ctx, os, ts)
      end
    end
  end
  local ok = read_strings_count == mo_hdr.nb_strings
  C.close(fd)
  return ok
end

GetText_mt.__index.getPlural = getDefaultPlural

function GetText_mt.__index.ngettext(msgid, msgid_plural, n)
  local plural = GetText.getPlural(n)

  if plural == 0 then
    return GetText.translation[msgid] and GetText.translation[msgid][plural] or GetText.wrapUntranslated(msgid)
  else
    return GetText.translation[msgid] and GetText.translation[msgid][plural] or GetText.wrapUntranslated(msgid_plural)
  end
end

function GetText_mt.__index.npgettext(msgctxt, msgid, msgid_plural, n)
  local plural = GetText.getPlural(n)

  if plural == 0 then
    return GetText.context[msgctxt] and GetText.context[msgctxt][msgid] and GetText.context[msgctxt][msgid][plural] or
    GetText.wrapUntranslated(msgid)
  else
    return GetText.context[msgctxt] and GetText.context[msgctxt][msgid] and GetText.context[msgctxt][msgid][plural] or
    GetText.wrapUntranslated(msgid_plural)
  end
end

function GetText_mt.__index.pgettext(msgctxt, msgid)
  return GetText.context[msgctxt] and GetText.context[msgctxt][msgid] or GetText.wrapUntranslated(msgid)
end

setmetatable(GetText, GetText_mt)

GetText.changeLang(require("gettext").current_lang)

return GetText
