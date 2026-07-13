local function shallow_clone(arr)
    return table.move(arr, 1, #arr, 1, {})
end

return shallow_clone
