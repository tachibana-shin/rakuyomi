# Search Gallery View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add cover and grid view modes to the manga search results screen, toggleable via a title bar icon and a plugin settings entry, persisted in `G_reader_settings`.

**Architecture:** `MangaSearchResults` is switched from `Menu:extend` to `MenuCustom:extend` to gain grid-column layout support. A `search_view_mode` field (read from `G_reader_settings` on init) drives which `MenuItem` class is used in `updateItems()`. A left title bar icon cycles through the three modes on tap and emits a `search_view_mode_changed` IPC event for testability. A new `is_local` entry in `Settings.lua` exposes the same key in the settings menu.

**Tech Stack:** LuaJIT, KOReader widget API (`Menu`, `MenuCustom`, `MenuItemCover`, `MenuItemGrid`), `G_reader_settings` for local persistence. E2E tests use pytest + `KOReaderDriver` (pyautogui + LLM-based UI queries).

---

## File Map

| File | Change |
|------|--------|
| `frontend/rakuyomi.koplugin/MangaSearchResults.lua` | Extend `MenuCustom`, add view mode field, title bar toggle + event emit, update `updateItems` and `generateItemTableFromSearchResults` |
| `frontend/rakuyomi.koplugin/Settings.lua` | Add `rakuyomi_search_view_mode` entry under a new Search divider |
| `e2e-tests/tests/test_search_view_modes.py` | New e2e test: toggle cycles modes, UI reflects each mode, mode persists across searches |

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

Add this method after `init()`. It cycles the mode, persists to `G_reader_settings`, re-renders, and emits an IPC event so the e2e test can wait on it:

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
  Testing:emitEvent("search_view_mode_changed", { mode = next_mode })
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

- [ ] **Step 7: Manual smoke test**

Launch with `./tools/dev-macos.sh`. Search for any manga. Confirm:
- Default view is a text list with the `column.two` icon in the title bar
- Tapping the icon once switches to cover art view
- Tapping again switches to grid view
- Tapping again returns to list view

- [ ] **Step 8: Commit**

```bash
git add frontend/rakuyomi.koplugin/MangaSearchResults.lua
git commit -m "feat: add gallery view to search results"
```

---

### Task 3: Add e2e test for search view modes

**Files:**
- Create: `e2e-tests/tests/test_search_view_modes.py`

- [ ] **Step 1: Create the test file**

Create `e2e-tests/tests/test_search_view_modes.py` with the following content:

```python
import time
from typing import Literal

from pydantic import BaseModel

from . import queries
from .queries.locate_button import LocateButtonResponse
from .koreader_driver import KOReaderDriver


class SearchViewModeResponse(BaseModel):
    mode: Literal['base', 'cover', 'grid']


async def get_search_view_mode(driver: KOReaderDriver) -> str:
    response = await driver.query(
        "What is the current view mode of the search results? "
        "Reply with 'base' if items are shown as a plain text list with no images, "
        "'cover' if items show cover art on the left side next to text, "
        "or 'grid' if items are arranged in multiple columns each showing cover art.",
        SearchViewModeResponse,
    )
    return response.mode


async def open_search(driver: KOReaderDriver, query: str) -> None:
    menu_button = await queries.locate_button(driver, "menu")
    driver.click_element(menu_button)

    search_button = await queries.locate_button(driver, "Search")
    driver.click_element(search_button)
    time.sleep(1)

    driver.type(query)
    search_button = await queries.locate_button(driver, "Search")
    driver.click_element(search_button)

    await driver.wait_for_event('manga_search_results_shown')


async def test_search_view_modes(koreader_driver: KOReaderDriver):
    await koreader_driver.install_source('multi.batoto')
    await koreader_driver.open_library_view()

    # Open an initial search
    await open_search(koreader_driver, 'houseki no kuni')

    # Default should be base (list) view
    mode = await get_search_view_mode(koreader_driver)
    assert mode == 'base', f"Expected default view mode 'base', got '{mode}'"

    # Tap toggle → cover
    toggle = await koreader_driver.query(
        "Locate the view mode toggle icon button in the top left corner of the title bar",
        LocateButtonResponse,
    )
    koreader_driver.click_element(toggle)
    await koreader_driver.wait_for_event('search_view_mode_changed')

    mode = await get_search_view_mode(koreader_driver)
    assert mode == 'cover', f"Expected view mode 'cover', got '{mode}'"

    # Tap toggle → grid
    toggle = await koreader_driver.query(
        "Locate the view mode toggle icon button in the top left corner of the title bar",
        LocateButtonResponse,
    )
    koreader_driver.click_element(toggle)
    await koreader_driver.wait_for_event('search_view_mode_changed')

    mode = await get_search_view_mode(koreader_driver)
    assert mode == 'grid', f"Expected view mode 'grid', got '{mode}'"

    # Close search and reopen — mode should persist
    back_button = await queries.locate_button(koreader_driver, "Back")
    koreader_driver.click_element(back_button)

    await open_search(koreader_driver, 'houseki no kuni')

    mode = await get_search_view_mode(koreader_driver)
    assert mode == 'grid', f"Expected persisted view mode 'grid', got '{mode}'"
```

- [ ] **Step 2: Commit**

```bash
git add e2e-tests/tests/test_search_view_modes.py
git commit -m "test: add e2e test for search view mode cycling and persistence"
```
