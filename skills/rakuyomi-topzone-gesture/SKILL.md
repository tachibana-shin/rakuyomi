---
name: rakuyomi-topzone-gesture
description: Use when adding or modifying top-zone gesture handling (tap/swipe to open KOReader native top bar) in RakuYomi plugin views. Covers TopZoneHandler utility, FocusManagerWithTopZone wrapper, and Menu-based auto-apply pattern.
---

# RakuYomi Top-Zone Gesture Pattern

This skill documents how top-zone gesture handling works in the RakuYomi KOReader plugin. Use this when adding the behavior to new views or modifying existing gesture handling.

## Architecture

Top-zone gestures (tap and south swipe in the top 15% of the screen) open KOReader's native top bar via `FileManager.instance.menu:onShowMenu()`.

The implementation uses KOReader's `registerTouchZones` API, which is checked BEFORE `ges_events` in `InputContainer:onGesture()`. This means top-zone gestures intercept events before any `onSwipe` or `TapSelect` handlers.

### Files

| File | Purpose |
|------|---------|
| `widgets/TopZoneHandler.lua` | Utility module with `enableTopZoneHandler(self)` method |
| `widgets/FocusManagerWithTopZone.lua` | Wrapper extending `FocusManager`, calls `enableTopZoneHandler` in `_init` |
| `widgets/Menu.lua` | Calls `enableTopZoneHandler(self)` after `BaseMenu.init(self)` |

### View Categories

**Menu-based views** (extend `Menu` or `MenuCustom`):
- LibraryView, ChapterListing, MangaSearchResults, NotificationView, InstalledSourcesListing, AvailableSourcesListing
- Get top-zone behavior automatically from `widgets/Menu.lua` init
- No changes needed when creating new Menu-based views

**FocusManager-based views** (extend `FocusManager`):
- Settings, SourceSettings, CookieSyncView, MangaInfoWidget
- Must extend `FocusManagerWithTopZone` instead of `FocusManager` directly

## How to Add Top-Zone Behavior to a New View

### If the view extends Menu or MenuCustom

Nothing to do — behavior is inherited from `widgets/Menu.lua`.

### If the view extends FocusManager

Change the import in the view file:

```lua
-- Before:
local FocusManager = require("ui/widget/focusmanager")

-- After:
local FocusManager = require("widgets/FocusManagerWithTopZone")
```

The variable name `FocusManager` can stay the same to minimize diff.

## How TopZoneHandler Works

`TopZoneHandler:enableTopZoneHandler(self)` registers two touch zones via `self:registerTouchZones()`:

1. **`rakuyomi_top_tap`** — matches taps in top 15% of screen
2. **`rakuyomi_top_swipe`** — matches south swipes starting in top 15% of screen

Both call `FileManager.instance.menu:onShowMenu()` to open KOReader's native top bar.

The `registerTouchZones` API uses ratio-based zones:
```lua
screen_zone = {
    ratio_x = 0,
    ratio_y = 0,
    ratio_w = 1,
    ratio_h = top_h / screen_h,  -- ~0.15
}
```

## Interaction with onSwipe

Since `registerTouchZones` is checked before `ges_events`:
- South swipe from top zone → touch zone intercepts → opens top bar
- South swipe from below top zone → `ges_events.Swipe` → `onSwipe` handler

Views with existing `onSwipe` (like LibraryView, ChapterListing) do NOT need top-zone checks in their `onSwipe` — the touch zone handles it.

## Creating New FocusManager-Based Views

```lua
local FocusManager = require("widgets/FocusManagerWithTopZone")

local MyNewView = FocusManager:extend {}

function MyNewView:init()
  -- FocusManagerWithTopZone:_init() is called automatically during new()
  -- Top-zone touch zones are registered before init() runs
  ...
end
```

## Key KOReader APIs

- `InputContainer:registerTouchZones(zones)` — registers gesture zones checked before `ges_events`
- `InputContainer:onGesture(ev)` — checks touch zones first, then `ges_events`
- `FileManager.instance.menu:onShowMenu()` — opens KOReader's native top bar
- `GestureRange:new{ ges, range, direction }` — gesture matching with position/direction filters
