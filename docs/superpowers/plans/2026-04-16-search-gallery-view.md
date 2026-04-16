# Search Gallery View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add cover and grid view modes to the manga search results screen, toggleable via a title bar icon and a plugin settings entry, persisted in `G_reader_settings`.

**Architecture:** `MangaSearchResults` is switched from `Menu:extend` to `MenuCustom:extend` to gain grid-column layout support. A `search_view_mode` field (read from `G_reader_settings` on init) drives which `MenuItem` class is used in `updateItems()`. A left title bar icon cycles through the three modes on tap. A new `is_local` entry in `Settings.lua` exposes the same key in the settings menu.

**Tech Stack:** LuaJIT, KOReader widget API (`Menu`, `MenuCustom`, `MenuItemCover`, `MenuItemGrid`), `G_reader_settings` for local persistence.

---

## File Map

| File | Change |
|------|--------|
| `frontend/rakuyomi.koplugin/MangaSearchResults.lua` | Extend `MenuCustom`, add view mode field, title bar toggle, update `updateItems` and `generateItemTableFromSearchResults` |
| `frontend/rakuyomi.koplugin/Settings.lua` | Add `rakuyomi_search_view_mode` entry under a new Search divider |

---

### Task 1: Add search_view_mode to Settings

**Files:**
- Modify: `frontend/rakuyomi.koplugin/Settings.lua:36-104`

- [ ] **Step 1: Add the Search divider and search_view_mode entry**

In `Settings.lua`, insert the following two entries immediately after the closing brace of the `rakuyomi_grid_rows` block (after line 104) and before the `Reader` divider:

```lua
  {
    nil,
    { type = 'divider', title = _("Search") }
  },
  {
    'rakuyomi_search_view_mode',
    {
      type = 'enum',
      title = _("Search view mode"),
      options = {
        { label = _("Base"),  value = "base" },
        { label = _("Cover"), value = "cover" },
        { label = _("Grid"),  value = "grid" },
      },
      is_local = true,
      default = "base",
    }
  },
```

The `is_local = true` flag causes `Settings:init()` to read/write this key via `G_reader_settings` automatically — no additional handler code needed.

- [ ] **Step 2: Verify the settings screen renders without error**

Launch KOReader with `./tools/dev-macos.sh`, open the plugin settings, and confirm the new "Search" section appears with a "Search view mode" entry defaulting to "Base". Changing it should not crash.

- [ ] **Step 3: Commit**

```bash
git add frontend/rakuyomi.koplugin/Settings.lua
git commit -m "feat: add search view mode setting"
```

---

### Task 2: Implement view mode in MangaSearchResults

**Files:**
- Modify: `frontend/rakuyomi.koplugin/MangaSearchResults.lua`

- [ ] **Step 1: Add the new requires and switch to MenuCustom**

Replace the existing require block and class declaration at the top of `MangaSearchResults.lua`. The file currently starts with:

```lua
local Menu = require("widgets/Menu")
```

and the class is declared as:

```lua
local MangaSearchResults = Menu:extend {
```

Update to:

```lua
local Menu = require("widgets/Menu")
local MenuCustom = require("patch/MenuCustom")
local MenuItemCover = require("patch/MenuItemCover")
local MenuItemGrid = require("patch/MenuItemGrid")
```

and change the class declaration to:

```lua
local MangaSearchResults = MenuCustom:extend {
  name = "manga_search_results",
  is_enable_shortcut = false,
  is_popout = false,
  title = _("Search results..."),
  with_context_menu = true,

  results = nil,
  on_return_callback = nil,
}
```

- [ ] **Step 2: Update init() to read the view mode and wire the title bar icon**

Replace the existing `MangaSearchResults:init()` with:

```lua
function MangaSearchResults:init()
  self.results = self.results or {}
  self.search_view_mode = G_reader_settings:readSetting("rakuyomi_search_view_mode", "base")

  self.title_bar_left_icon = "column.two"
  self.onLeftButtonTap = function()
    self:cycleViewMode()
  end

  self.width = Screen:getWidth()
  self.height = Screen:getHeight()
  local page = self.page
  Menu.init(self)
  self.page = page

  self.paths = { 0 }
  self.on_return_callback = nil
end
```

- [ ] **Step 3: Add cycleViewMode()**

Add this method after `init()`:

```lua
function MangaSearchResults:cycleViewMode()
  local modes = { "base", "cover", "grid" }
  local next_mode = "base"
  for i, mode in ipairs(modes) do
    if mode == self.search_view_mode then
      next_mode = modes[(i % #modes) + 1]
      break
    end
  end
  self.search_view_mode = next_mode
  G_reader_settings:saveSetting("rakuyomi_search_view_mode", next_mode)
  self:updateItems()
end
```

- [ ] **Step 4: Add _recalculateDimen()**

Add this method after `cycleViewMode()`:

```lua
function MangaSearchResults:_recalculateDimen(flag)
  if self.search_view_mode ~= "base" then
    MenuCustom._recalculateDimen(self, flag)
  else
    Menu._recalculateDimen(self, flag)
  end
end
```

- [ ] **Step 5: Replace updateItems()**

Replace the existing `MangaSearchResults:updateItems()` with:

```lua
function MangaSearchResults:updateItems()
  self.item_table = self:generateItemTableFromSearchResults(self.results)

  local mode = self.search_view_mode
  if mode == "grid" then
    self.grid_columns = G_reader_settings:readSetting("rakuyomi_grid_columns") or 3
    MenuCustom.updateItems(self, MenuItemGrid)
  elseif mode == "cover" then
    self.grid_columns = nil
    MenuCustom.updateItems(self, MenuItemCover)
  else
    self.grid_columns = nil
    Menu.updateItems(self)
  end
end
```

- [ ] **Step 6: Update generateItemTableFromSearchResults() to include manga_cover**

Replace the existing `MangaSearchResults:generateItemTableFromSearchResults()` with:

```lua
function MangaSearchResults:generateItemTableFromSearchResults(results)
  local item_table = {}
  local is_cover = self.search_view_mode == "cover"

  for _, manga in ipairs(results) do
    local mandatory = (manga.last_read and calcLastReadText(manga.last_read) .. " " or "")

    if manga.unread_chapters_count ~= nil and manga.unread_chapters_count > 0 then
      mandatory = mandatory .. Icons.FA_BELL .. manga.unread_chapters_count
    end

    if manga.in_library then
      mandatory = mandatory .. Icons.COD_LIBRARY
    end

    table.insert(item_table, {
      manga = manga,
      text = manga.title,
      post_text = is_cover and mandatory or manga.source.name,
      manga_cover = self.search_view_mode ~= "base" and manga.manga_cover or nil,
      mandatory = not is_cover and mandatory or nil,
    })
  end

  return item_table
end
```

- [ ] **Step 7: Manual test — list view (base)**

Launch with `./tools/dev-macos.sh`. Search for any manga. Confirm results display as a list (current behavior unchanged). The title bar should show the `column.two` icon on the left.

- [ ] **Step 8: Manual test — cover view**

Tap the `column.two` icon once. Confirm the results switch to cover art view with source name displayed below each title.

- [ ] **Step 9: Manual test — grid view**

Tap the icon again. Confirm the results switch to grid view using the same column count as the library grid setting.

- [ ] **Step 10: Manual test — persistence**

While in grid mode, close the search results and open a new search. Confirm the view mode is still grid. Restart KOReader and confirm it persists across sessions.

- [ ] **Step 11: Manual test — settings menu**

Open plugin settings. Change "Search view mode" to a different value. Open search results and confirm the new mode is applied.

- [ ] **Step 12: Commit**

```bash
git add frontend/rakuyomi.koplugin/MangaSearchResults.lua
git commit -m "feat: add gallery view to search results"
```
