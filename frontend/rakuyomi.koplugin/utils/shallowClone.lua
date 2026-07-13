local table_move = table.move
if not table_move then
    table_move = function(a1, f, e, t, a2)
        a2 = a2 or a1
        if e >= f then
            for i = 0, e - f do
                a2[t + i] = a1[f + i]
            end
        end
        return a2
    end
end

local function shallow_clone(arr)
    return table_move(arr, 1, #arr, 1, {})
end

return shallow_clone
