-- Run from the repository root: luajit frontend/tests/chapter_languages.lua
-- Exercise ChapterListing with lightweight KOReader widget/backend substitutes.
-- luacheck: globals G_defaults

package.path = "frontend/rakuyomi.koplugin/?.lua;" .. package.path

local widget = {}
function widget:new(value) return value end
function widget:extend(value) return value end

local dependencies = {
  "ui/bidi", "ui/widget/buttondialog", "ui/widget/infomessage",
  "ui/widget/inputdialog", "ui/uimanager", "ui/widget/confirmbox",
  "logger", "ffi/util", "ui/widget/iconbutton", "ui/widget/horizontalgroup",
  "ui/widget/verticalgroup", "ui/widget/verticalspan", "ui/widget/button",
  "datastorage", "luasettings", "jobs/DownloadChapter", "jobs/DownloadUnreadChapters",
  "DownloadUnreadChaptersJobDialog", "ErrorDialog", "MangaReader", "MangaInfoWidget",
  "CheckboxDialog", "testing", "utils/calcLastReadText", "utils/isBeforeChapter",
  "utils/findLastRead", "utils/getChapterDisplayName", "TrackingMenu",
  "chapters/findNextChapter", "chapters/findPreviousChapter",
}
for _, name in ipairs(dependencies) do
  package.loaded[name] = widget
end
package.loaded["widgets/Menu"] = widget
package.loaded["gettext+"] = function(text) return text end
package.loaded["ffi/sha2"] = { md5 = function(text) return text end }
package.loaded["device"] = { screen = { scaleBySize = function(_, size) return size end } }
package.loaded["ui/font"] = { getFace = function() return {} end }
package.loaded["Icons"] = { LANG = "Languages" }
package.loaded["ui/trapper"] = { wrap = function(_, action) action() end }
package.loaded["ui/network/manager"] = { isConnected = function() return true end }
G_defaults = { readSetting = function() return 16 end }

local refreshed = {
  { id = "english", lang = "en" },
  { id = "brazilian", lang = "pt-br" },
}
local refresh_result = { type = "SUCCESS" }
local cancelled = false
local reads = 0
package.loaded["Backend"] = {
  createCancelId = function() return "test" end,
  refreshChapters = function() return refresh_result end,
  listCachedChapters = function()
    reads = reads + 1
    return { type = "SUCCESS", body = refreshed }
  end,
}
package.loaded["LoadingDialog"] = {
  showAndRun = function(_, _, action) return action(), cancelled end,
}
local errors = 0
package.loaded["ErrorDialog"] = { show = function() errors = errors + 1 end }
package.loaded["ui/uimanager"] = { setDirty = function() end }

local ChapterListing = require("ChapterListing")
local function listing()
  return setmetatable({
    manga = { id = "zk12", source = { id = "multi.mangafire" } },
    raw_chapters = { refreshed[1] },
    chapters = { refreshed[1] },
    langs = {},
    langs_selected = {},
    title_bar = { left_icon_size_ratio = 1 },
    hashMangaId = function() return "test" end,
    readSettings = function()
      return { readSetting = function() return {} end }
    end,
    extractAvailableScanlators = function() end,
    loadSavedScanlatorPreference = function() end,
    updateItems = function() end,
  }, { __index = ChapterListing })
end

-- Refresh must replace an already populated list and discover the new language.
local view = listing()
view:refreshChapters()
assert(reads == 1, "successful refresh must read the new cached chapters")
assert(#view.chapters == 2, "new Brazilian Portuguese chapters must appear")
assert(#view.langs == 2, "refresh must rebuild available language choices")
assert(#view.title_bar.left_button == 3, "the language selector must appear")

-- Choosing one language must not remove the way to change it again.
view.langs_selected = { "pt-br" }
view:patchTitleBar(1)
assert(#view.title_bar.left_button == 3, "keep selector when only one language is selected")
view.langs = { { id = "pt-br", name = "pt-br" } }
view:patchTitleBar(1)
assert(#view.title_bar.left_button == 2, "single-language lists need no selector")

-- Failed or cancelled refreshes preserve the currently usable list.
view = listing()
local original = view.raw_chapters
refresh_result = { type = "ERROR", message = "offline" }
view:refreshChapters()
assert(errors == 1 and view.raw_chapters == original and reads == 1)
refresh_result = { type = "SUCCESS" }
cancelled = true
view:refreshChapters()
assert(view.raw_chapters == original and reads == 1)

-- A successful empty result must clear the old English chapters too.
cancelled = false
refreshed = {}
view:refreshChapters()
assert(reads == 2 and #view.raw_chapters == 0 and #view.chapters == 0)

print("Chapter language regressions: passed")
