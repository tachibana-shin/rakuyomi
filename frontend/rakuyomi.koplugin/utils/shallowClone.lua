local function shallow_clone(arr)
    local clone = {}
    for i = 1, #arr do
        clone[i] = arr[i]
    end
    return clone
end

return shallow_clone
