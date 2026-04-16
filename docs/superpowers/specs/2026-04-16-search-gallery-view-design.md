# Search Gallery View

## Summary

Add gallery (cover and grid) view modes to the manga search results screen, mirroring the existing library view modes. The preference is stored separately from the library setting and persists across sessions.

## Goals

- Users can view search results in list, cover, or grid mode
- The active mode is toggled via an icon in the search results title bar
- The same setting is also accessible from the plugin settings menu
- Defaults to list (`base`) so existing users see no change

## Non-Goals

- Migrating `library_view_mode` from backend settings to `G_reader_settings` (future cleanup)
- Changing the grid column/row counts (reuses the existing `rakuyomi_grid_columns` / `rakuyomi_grid_rows` settings)

## Storage

`G_reader_settings` key: `rakuyomi_search_view_mode`  
Values: `"base"` | `"cover"` | `"grid"`  
Default: `"base"`

No backend/Rust changes required.

## Components

### MangaSearchResults.lua

- Extend `MenuCustom` instead of `Menu` to gain grid column layout support
- Add `search_view_mode` field, read from `G_reader_settings` on init
- Add a title bar left icon that cycles `base → cover → grid → base` on tap:
  - Saves the new mode to `G_reader_settings` immediately
  - Re-renders the list with the new mode applied
- In `updateItems()`:
  - `"cover"` → use `MenuItemCover`, `grid_columns = nil`
  - `"grid"` → use `MenuItemGrid`, `grid_columns` read from `rakuyomi_grid_columns`
  - `"base"` → use default `MenuItem`, `grid_columns = nil`

### Settings.lua

- Add a `search_view_mode` entry with labels Base / Cover / Grid
- Reads and writes directly to `G_reader_settings` (not the backend settings API)
- Positioned alongside the existing `library_view_mode` entry
