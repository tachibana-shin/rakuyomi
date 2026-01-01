local _ = require("gettext+")

local function calcLastReadText(ts)
    -- 現在のUNIXタイムを取得する
    local now = os.time()
    local diff = now - ts
    if diff < 0 then diff = 0 end

    if diff < 60 then
        return _("jnow")
    elseif diff < 3600 then
        return string.format(_("%dm"), math.floor(diff / 60))
    elseif diff < 86400 then
        return string.format(_("%dh"), math.floor(diff / 3600))
    elseif diff < 604800 then
        return string.format(_("%dd"), math.floor(diff / 86400))
    elseif diff < 2592000 then
        return string.format(_("%dw"), math.floor(diff / 604800))
    elseif diff < 31536000 then
        return string.format(_("%dM"), math.floor(diff / 2592000))
    else
        return string.format(_("%dy"), math.floor(diff / 31536000))
    end
end
return calcLastReadText