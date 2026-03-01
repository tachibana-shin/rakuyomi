local BasicJobDialog = require("BasicJobDialog")
local _ = require("gettext+")

--- @class DownloadUnreadChaptersJobDialog: BasicJobDialog
local DownloadUnreadChaptersJobDialog = BasicJobDialog:extend {
  title = _("Downloading chapters, this will take a while…"),
  success_message = _('Download complete!'),
  error_prefix = _("An error occurred while downloading chapters"),
}

--- @param data any
--- @return string|nil
function DownloadUnreadChaptersJobDialog:format_progress(data)
  if not data or data.type == 'INITIALIZING' then
    return nil
  end

  return data.downloaded .. ' / ' .. data.total
end

function DownloadUnreadChaptersJobDialog:onCancellationRequested()
  BasicJobDialog.onCancellationRequested(self)
  self.success_message = _("Download cancelled!")
end

return DownloadUnreadChaptersJobDialog
