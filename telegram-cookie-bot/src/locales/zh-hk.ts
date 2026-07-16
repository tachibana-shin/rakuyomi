import type { Locale } from "./en.ts"

const zhHk: Locale = {
  command_start: "啟動機械人",
  command_link: "連結裝置",
  command_devices: "已連結裝置列表",
  command_cookies: "檢視已儲存 Cookie",
  command_clearcookies: "清除已儲存 Cookie",
  command_status: "機械人狀態",
  command_help: "說明",
  command_language: "切換語言",
  command_github: "原始碼",
  command_donate: "支持開發",
  command_app: "打開mini app查看cookie",
  app_prompt: "打開mini app瀏覽已儲存的cookie。",

  welcome: "歡迎使用 Rakuyomi Cookie Sync Bot！\n\n" +
    "此機械人可將 Android 裝置（Kiwi Browser）上的 " +
    "Cloudflare Cookie 同步至執行 KOReader（Rakuyomi）的 Kindle。\n\n" +
    "開始使用:\n" +
    "1. 打開 KOReader → Rakuyomi → Cookie Sync\n" +
    "2. 點擊「Pair Device」並輸入機械人 URL\n" +
    "3. 使用以下指令發送配對碼:\n" +
    "   /link 配對碼 裝置名稱\n\n" +
    "發送 Cookie:\n" +
    "1. 在 Kiwi Browser 安裝 Get cookies.txt LOCALLY 擴充功能\n" +
    "   https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n" +
    "2. 打開擴充功能並點擊 Export\n" +
    "3. 打開下載的檔案並複製 JSON 陣列\n" +
    "4. 將 JSON 陣列直接貼到此聊天\n" +
    "   可選：在前面加上裝置名稱以指定裝置\n\n" +
    "指令:\n" +
    "/link [配對碼] [名稱] — 連結裝置\n" +
    "/devices — 已連結裝置列表\n" +
    "/cookies [名稱] — 檢視 Cookie\n" +
    "/clearcookies [名稱] [域名] — 清除 Cookie\n" +
    "/status — 機械人狀態\n" +
    "/help — 說明\n" +
    "/language — 切換語言",

  link_usage: "用法: /link [配對碼] [裝置名稱]\n\n" +
    "範例: /link A8F27K9X kindle_bedroom",

  link_invalid_code: "配對碼無效或已過期。" +
    "請在 KOReader → Rakuyomi → Cookie Sync 中產生新配對碼。",

  link_no_chat_id: "無法確定聊天 ID。",

  link_success: (name: string) => `裝置 [${name}] 已成功連結！`,

  devices_none: "尚未連結任何裝置。請使用 /link 配對碼 名稱 來連結裝置。",

  devices_list: (lines: string) => `已連結裝置:\n${lines}`,

  status_online: (chatId: string, pending: number) =>
    "Rakuyomi Cookie Sync Bot\n\n" +
    `聊天 ID: ${chatId}\n` +
    `待處理配對: ${pending}\n` +
    "狀態: 在線",

  cookie_received: (domains: string, device: string) =>
    `已收到以下域名的 Cookie: ${domains}\n` +
    `目標裝置: ${device}`,

  cookie_syntax:
    "從 Get cookies.txt LOCALLY 擴充功能發送 Cookie，請將 JSON 陣列直接貼到聊天:\n" +
    "[{...}]\n\n" +
    "為特定裝置指定 Cookie，在前面加上裝置名稱:\n" +
    "裝置名稱 [{...}]",

  cookie_invalid_json: "JSON 格式無效。" +
    "請貼上 Get cookies.txt LOCALLY 的有效 JSON 陣列。",

  cookie_invalid_format: "Cookie 格式無效。" +
    "每個條目至少需要 name、value 和 domain 欄位。\n\n" +
    "範例:\n" +
    '[{"name":"cf_clearance","value":"abc","domain":".example.com"}]',

  cookie_extension_url:
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc",

  cookies_none: "尚未儲存任何 Cookie。",
  cookies_header: (device: string) => `裝置 [${device}] 的 Cookie:`,
  cookies_list: (domains: string) => `域名:\n${domains}`,
  cookies_device_list: (lines: string) => `已儲存 Cookie:\n${lines}`,
  cookies_view_in_webapp: "📋 在Web App中查看",
  cookies_too_large:
    "Cookie數據太大，無法在此顯示。打開Web App查看所有Cookie。",

  clearcookies_all_done: "已清除所有 Cookie。",
  clearcookies_device_done: (device: string) =>
    `已清除裝置 [${device}] 的所有 Cookie。`,
  clearcookies_domain_done: (domain: string, device: string) =>
    `已清除裝置 [${device}] 上域名 ${domain} 的 Cookie。`,
  clearcookies_none: "找不到要清除的 Cookie。",
  clearcookies_usage: "用法:\n/clearcookies — 清除所有 Cookie\n" +
    "/clearcookies 裝置名稱 — 清除裝置的 Cookie\n" +
    "/clearcookies 裝置名稱 域名 — 清除指定域名的 Cookie",

  help: "Rakuyomi Cookie Sync Bot — 說明\n\n" +
    "此機械人作為 Android 瀏覽器與 " +
    "KOReader（Rakuyomi）之間的中介。\n\n" +
    "流程:\n" +
    "1. 在 Kiwi Browser 安裝 Get cookies.txt LOCALLY 擴充功能\n" +
    "2. 打開擴充功能並點擊 Export\n" +
    "3. 打開下載的檔案並複製 JSON 陣列\n" +
    "4. 貼到此聊天（一般文字訊息）\n" +
    "5. 使用 /link 配對碼 名稱 配對裝置\n" +
    "6. Rakuyomi 從機械人獲取 Cookie\n\n" +
    "Cookie 語法:\n" +
    '[{"name":"...","value":"...","domain":".example.com"}]\n' +
    "前面加上裝置名稱: kindle_bedroom [{...}]\n\n" +
    "Get cookies.txt LOCALLY 擴充功能:\n" +
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n\n" +
    "指令:\n" +
    "/link [配對碼] [名稱] — 連結裝置\n" +
    "/devices — 已連結裝置\n" +
    "/cookies [名稱] — 檢視 Cookie\n" +
    "/app — 打開mini app查看cookie\n" +
    "/clearcookies [名稱] [域名] — 清除 Cookie\n" +
    "/status — 狀態\n" +
    "/language — 切換語言\n" +
    "/github — 原始碼\n" +
    "/donate — 支持開發",

  github: "Rakuyomi 是開源軟件！\n\n" +
    "GitHub: https://github.com/tachibana-shin/rakuyomi\n" +
    "報告問題: https://github.com/tachibana-shin/rakuyomi/issues",

  donate: "如果您覺得此項目有用，請考慮支持開發者:\n\n" +
    "Ko-fi: https://ko-fi.com/tachib_shin\n" +
    "Momo: https://me.momo.vn/tachibshin",

  language_prompt: "請選擇您的語言:",

  language_set: (lang: string) => `語言已切換為 ${lang}。`,

  unlink_usage: "<b>Usage:</b> /unlink DEVICE_NAME",
  unlink_not_found: (device: string) =>
    `No data found for <code>${device}</code>. It may already be removed.`,
  unlink_done: (device: string) =>
    `<b>Unlinked</b> <code>${device}</code> — cookies and pairing info removed.`,
  unknown_command: "未知指令。請輸入 /help 查看可用指令。",

  cookie_needs_update: (device: string, url: string) =>
    `⚠️ <b>需要更新 Cookie</b>\n\n` +
    `裝置 <code>${device}</code> 在 <code>${url}</code> 同步 Cookie 後仍然遇到 403 錯誤。\n\n` +
    `請通過 Telegram 發送該裝置的新 Cookie。`,
}

export default zhHk
