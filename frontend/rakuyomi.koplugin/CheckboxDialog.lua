local CheckButton = require("ui/widget/checkbutton")
local UIManager = require("ui/uimanager")
local CustomDialog = require("CustomDialog")
local hasValue = require("utils/hasValue")
---@diagnostic disable-next-line: different-requires
local util = require("util")

--- @class BaseOption
--- @field name string
--- @field id string

--- @class CheckboxDialog<T>: CustomDialog
--- @field current string[]
--- @field options BaseOption[]
--- @field update_callback fun(value: string[])
--- @field new fun(any): CheckboxDialog
--- @diagnostic disable-next-line: redundant-parameter
local CheckboxDialog = CustomDialog:extend {

}
function CheckboxDialog:init()
  self.generate = function(item, max_width)
    local option = util.tableDeepCopy(item)
    if option.label ~= nil then
      option.name = option.label
      option.id = option.value
    end

    return CheckButton:new {
      text = option.name,
      provider = option.id,
      checked = hasValue(self.current, option.id),
      width = max_width,
      callback = function()
        local checked = hasValue(self.current, option.id)

        if checked then
          for i = #self.current, 1, -1 do
            if self.current[i] == option.id then
              table.remove(self.current, i)
              break
            end
          end
        else
          table.insert(self.current, option.id)
        end

        self.update_callback(self.current)
        UIManager:setDirty(self, "ui")
      end
    }
  end

  CustomDialog:init(self)
end

return CheckboxDialog
