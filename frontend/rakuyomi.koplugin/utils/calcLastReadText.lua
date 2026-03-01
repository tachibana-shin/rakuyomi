local _ = require("gettext+")

--- @param ts number
--- @param full_text boolean|nil
--- @return string
local function calcLastReadText(ts, full_text)
    -- 現在のUNIXタイムを取得する
    local now = os.time()
    local diff = now - ts
    if diff < 0 then diff = 0 end

    if diff < 60 then
        return full_text and _("just now") or _("jnow")
    elseif diff < 3600 then
        return string.format(full_text and ("%d " .. _("minutes")) or _("%dm"), math.floor(diff / 60))
    elseif diff < 86400 then
        return string.format(full_text and ("%d " .. _("hours")) or _("%dh"), math.floor(diff / 3600))
    elseif diff < 604800 then
        return string.format(full_text and ("%d " .. _("days")) or _("%dd"), math.floor(diff / 86400))
    elseif diff < 2592000 then
        return string.format(full_text and ("%d " .. _("weeks")) or _("%dw"), math.floor(diff / 604800))
    elseif diff < 31536000 then
        return string.format(full_text and ("%d " .. _("months")) or _("%dM"), math.floor(diff / 2592000))
    else
        return string.format(full_text and ("%d " .. _("years")) or _("%dy"), math.floor(diff / 31536000))
    end
end
return calcLastReadText
