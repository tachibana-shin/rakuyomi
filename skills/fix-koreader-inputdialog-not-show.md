## Assistant (Compaction · Big Pickle · 23.2s)

_Thinking:_

Let me analyze the conversation history and update the summary with the key findings.

The critical discovery is the root cause of why InputDialog disappears. Let me trace through:

1. User taps SettingItemValue → SettingItemValue:onTap creates InputDialog and calls UIManager:show(dialog)
2. UIManager:show adds InputDialog to the window stack (stack[3])
3. But the SAME tap event is still being dispatched through sendEvent
4. sendEvent restarts from the top of the window stack (line 952: `i = #self._window_stack`)
5. InputDialog is now at the top with `is_always_active = true` (line 124 of inputdialog.lua)
6. InputDialog:handleEvent receives the SAME tap event
7. InputDialog:onTap fires (line 559)
8. Keyboard is not visible → goes to else branch (line 574)
9. Checks `ges.pos:notIntersectWith(self.dialog_frame.dimen)` (line 575)
10. `dialog_frame.dimen` is nil (never painted yet)
11. `notIntersectWith(nil)` returns true (geometry.lua line 191: `if not rect_b or rect_b:area() == 0 then return true end`)
12. `onCloseDialog()` fires → finds "close" button → Cancel callback → UIManager:close(dialog)
13. Dialog is immediately closed!

The stack trace confirms this perfectly:
- UIManager:sendEvent → InputContainer:handleEvent → InputDialog:handleEvent (line 574) → onCloseDialog (line 738) → buttontable callback → SettingItemValue callback → UIManager:close

The fix: set `dialog.deny_keyboard_hiding = true` before showing (this makes onTap return early at line 561), then clear it after the event loop settles (in nextTick).

## Objective
- Debug and fix why `InputDialog` doesn't appear on screen when called from the `Settings` view in the rakuyomi KOReader plugin, even though all code executes without errors.

## Important Details
- User is Vietnamese, KOReader source at `~/koreader`, plugin at `~/rakuyomi/frontend/rakuyomi.koplugin/`
- User is running on an **emulator** (desktop), screen is 425x706
- `InputDialog` works fine in other plugin views (`ChapterListing`, `LibraryView`)
- Other dialog types (`RadioButtonWidget`, `SpinWidget`, `CheckboxDialog`, `PathChooser`) work from the same Settings view
- Settings view extends `FocusManager`, has `covers_fullscreen = true` (line 405)
- **Both "string" and "list" types are broken** — both use `InputDialog` + `onShowKeyboard()`
- **ROOT CAUSE FOUND**: The same tap event that opens SettingItemValue reaches the newly-shown InputDialog and immediately closes it
  - `sendEvent` restarts from top of stack after each widget processes (line 952: `i = #self._window_stack`)
  - InputDialog has `is_always_active = true` (inputdialog.lua line 124), so it receives events via `widget:handleEvent(event)` (uimanager.lua line 946)
  - `InputDialog:onTap` fires → keyboard not visible → checks `ges.pos:notIntersectWith(self.dialog_frame.dimen)` (line 575)
  - `dialog_frame.dimen` is nil (never painted) → `notIntersectWith(nil)` returns true (geometry.lua line 191: `if not rect_b or rect_b:area() == 0 then return true end`)
  - `onCloseDialog()` (line 738) → finds "close" button → Cancel callback → `UIManager:close(dialog)`
- `InputDialog:onTap` is at inputdialog.lua line 559: "If keyboard hidden, tap outside dialog closes it"
- `InputDialog:d` has `deny_keyboard_hiding` guard (line 561) that short-circuits `onTap` entirely when true

## Work State
### Completed
- Confirmed `InputDialog:new` created, `UIManager:show(dialog)` called, `dialog:onShowKeyboard()` called — all execute
- Confirmed `UIManager:show` adds widget to window stack correctly (stack_count=3, InputDialog at idx 3)
- Confirmed `dialog_frame.dimen=nil` before show (normal), `dialog[1].dimen=425x706` (correct)
- Confirmed `paintTo` was NEVER called on InputDialog — `_repaint()` never reaches it
- With `forceRePaint()`, dialog **flashes briefly then disappears**
- **Root cause identified**: `UIManager:sendEvent` processes the SAME tap event on the newly-shown InputDialog because:
  1. InputDialog is `is_always_active = true`
  2. `sendEvent` restarts from top of stack after Settings handles the event (line 952)
  3. `dialog_frame.dimen` is nil → `notIntersectWith(nil)` returns true → `onCloseDialog()` fires
- Stack trace from debug output confirms exact call chain: `sendEvent → InputContainer:handleEvent → InputDialog:handleEvent (line 574) → onCloseDialog (line 738) → buttontable callback → UIManager:close`
- `UIManager.close`, `onClose`, `onCloseWidget` hooks all fired — dialog IS being closed by the Cancel button callback
- `nextTick` confirmed: stack_count=2, dialog_in_stack=false — dialog was removed

### Active
- Fix identified: set `dialog.deny_keyboard_hiding = true` before `UIManager:show()` (blocks `onTap` at line 561), then clear it in `UIManager:nextTick` so subsequent taps work normally
- Need to implement the fix and clean up all debug instrumentation

### Blocked
- (none)

## Next Move
1. Implement fix in `SettingItemValue.lua`: set `dialog.deny_keyboard_hiding = true` before `UIManager:show(dialog)` for both "string" and "list" types, clear it in `UIManager:nextTick`
2. Clean up all debug prints, monkey-patches, hooks, and nextTick debug code
3. Verify fix works on emulator

## Relevant Files
- `/home/shin/rakuyomi/frontend/rakuyomi.koplugin/Settings.lua`: Main settings view (FocusManager, covers_fullscreen=true)
- `/home/shin/rakuyomi/frontend/rakuyomi.koplugin/widgets/SettingItemValue.lua`: Lines 259-370 — InputDialog creation for "string" and "list" types + debug hooks to remove; fix goes here
- `/home/shin/koreader/frontend/ui/widget/inputdialog.lua`: `is_always_active=true` (line 124), `onTap` at 559 (closes dialog when tap outside `dialog_frame.dimen`), `onCloseDialog` at 737, `ges_events.Tap` covers full screen (line 474), `deny_keyboard_hiding` guard at line 561
- `/home/shin/koreader/frontend/ui/uimanager.lua`: `sendEvent` at 884 (restarts from top at line 952 after widget handles event), `_repaint` at 1220
- `/home/shin/koreader/frontend/ui/geometry.lua`: `notIntersectWith` at 190 — returns true when `rect_b` is nil (line 191)
- `/home/shin/koreader/frontend/ui/widget/container/inputcontainer.lua`: `handleEvent` at 269
- `/home/shin/koreader/frontend/ui/widget/buttontable.lua`: Button callback at line 82
