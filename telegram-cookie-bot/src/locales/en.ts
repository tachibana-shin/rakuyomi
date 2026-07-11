export interface Locale {
  command_start: string
  command_link: string
  command_devices: string
  command_cookies: string
  command_clearcookies: string
  command_status: string
  command_help: string
  command_language: string
  command_github: string
  command_donate: string
  command_app: string
  app_prompt: string
  welcome: string
  link_usage: string
  link_invalid_code: string
  link_no_chat_id: string
  link_success: (name: string) => string
  devices_none: string
  devices_list: (lines: string) => string
  status_online: (chatId: string, pending: number) => string
  cookie_received: (domains: string, device: string) => string
  cookie_syntax: string
  cookie_invalid_json: string
  cookie_invalid_format: string
  cookie_extension_url: string
  cookies_none: string
  cookies_header: (device: string) => string
  cookies_list: (domains: string) => string
  cookies_device_list: (lines: string) => string
  cookies_view_in_webapp: string
  cookies_too_large: string
  clearcookies_all_done: string
  clearcookies_device_done: (device: string) => string
  clearcookies_domain_done: (domain: string, device: string) => string
  clearcookies_none: string
  clearcookies_usage: string
  help: string
  github: string
  donate: string
  language_prompt: string
  language_set: (lang: string) => string
  unlink_usage: string
  unlink_not_found: (device: string) => string
  unlink_done: (device: string) => string
  unknown_command: string
  cookie_needs_update: (device: string, url: string) => string
}

const en: Locale = {
  command_start: "Start the bot",
  command_link: "Link a device with pairing code",
  command_devices: "List linked devices",
  command_cookies: "List stored cookies per device",
  command_clearcookies: "Clear stored cookies",
  command_status: "Show bot status",
  command_help: "Show help",
  command_language: "Change language",
  command_github: "Source code & issues",
  command_donate: "Support development",
  command_app: "Open the mini app to view cookies",

  app_prompt: "Open the mini app to browse your stored cookies.",

  welcome: "<b>Rakuyomi Cookie Sync Bot</b>\n\n" +
    "This bot syncs Cloudflare cookies from your Android device " +
    "(Kiwi Browser) to your Kindle running KOReader (Rakuyomi).\n\n" +
    "<b>Getting Started</b>\n" +
    "1. Open <b>KOReader → Rakuyomi → Cookie Sync</b>\n" +
    "2. Tap <b>Pair Device</b> and enter this bot's URL\n" +
    "3. Send the pairing code to the bot:\n" +
    "   <code>/link CODE DEVICE_NAME</code>\n\n" +
    "<b>Sending Cookies</b>\n" +
    "1. Install <b>Get cookies.txt LOCALLY</b> in Kiwi Browser\n" +
    "   https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n" +
    "2. Open the extension and tap <b>Export</b>\n" +
    "3. Paste the JSON array directly into this chat\n" +
    "   Prefix with a device name: <code>DEVICE_NAME [{...}]</code>\n\n" +
    "<b>Commands</b>\n" +
    "/link [CODE] [NAME] — Link a device\n" +
    "/unlink [NAME] — Unlink a device\n" +
    "/devices — List linked devices\n" +
    "/cookies [DEVICE] — View stored cookies\n" +
    "/app — Open mini app\n" +
    "/clearcookies [DEVICE] [DOMAIN] — Clear cookies\n" +
    "/status — Show bot status\n" +
    "/help — Show this help\n" +
    "/language — Change language",

  link_usage:     "<b>Usage:</b> /link CODE DEVICE_NAME\n\n" +
    "Example: <code>/link A8F27K9X kindle_bedroom</code>",

  link_invalid_code: "Invalid or expired pairing code.\n" +
    "Generate a new code from <b>KOReader → Rakuyomi → Cookie Sync</b>.",

  link_no_chat_id: "Could not determine chat ID.",

  link_success: (name: string) =>
    `<b>Device linked successfully!</b>\n\n<code>${name}</code> is now connected to this chat.`,

  devices_none:
    "No devices linked yet.\nUse <code>/link CODE DEVICE_NAME</code> to link a device.",

  devices_list: (lines: string) => `<b>Linked devices:</b>\n${lines}`,

  status_online: (chatId: string, pending: number) =>
    "<b>Rakuyomi Cookie Sync Bot</b>\n\n" +
    `Chat ID: <code>${chatId}</code>\n` +
    `Pending pairings: <b>${pending}</b>\n` +
    "Status: <b>Online</b>",

  cookie_received: (domains: string, device: string) =>
    `<b>Cookie received</b>\n` +
    `Target device: <code>${device}</code>\n` +
    `Domains: <code>${domains}</code>`,

  cookie_syntax: "To send cookies from your browser extension, " +
    "paste the JSON array directly into the chat:\n" +
    "<code>[{...}]</code>\n\n" +
    "For a specific device, add its name as prefix:\n" +
    "<code>DEVICE_NAME [{...}]</code>",

  cookie_invalid_json: "Invalid JSON format.\n" +
    "Please paste a valid JSON array from <b>Get cookies.txt LOCALLY</b>.",

  cookie_invalid_format: "Invalid cookie format.\n" +
    "Each entry must have at least <code>name</code>, <code>value</code>, and <code>domain</code> fields.\n\n" +
    "Example:\n" +
    '<code>[{"name":"cf_clearance","value":"abc","domain":".example.com"}]</code>',

  cookie_extension_url:
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc",

  cookies_none: "No cookies stored for this chat.",
  cookies_header: (device: string) => `<b>Cookies for</b> <code>${device}</code>`,
  cookies_list: (domains: string) => `<b>Domains:</b>\n${domains}`,
  cookies_device_list: (lines: string) => `<b>Stored cookies:</b>\n${lines}`,

  cookies_view_in_webapp: "View in Web App",
  cookies_too_large: "The cookie data is too large to display here.\n" +
    "Open the <b>Web App</b> to view all stored cookies.",

  clearcookies_all_done: "<b>Cleared all cookies</b> for this chat.",
  clearcookies_device_done: (device: string) =>
    `<b>Cleared all cookies</b> for <code>${device}</code>.`,
  clearcookies_domain_done: (domain: string, device: string) =>
    `<b>Cleared cookies</b> for <code>${domain}</code> on <code>${device}</code>.`,
  clearcookies_none: "No cookies found to clear.",
  clearcookies_usage: "<b>Usage:</b>\n" +
    "/clearcookies — Clear all cookies\n" +
    "/clearcookies DEVICE — Clear cookies for a device\n" +
    "/clearcookies DEVICE DOMAIN — Clear cookies for a domain",

  help: "<b>Rakuyomi Cookie Sync Bot — Help</b>\n\n" +
    "This bot acts as a bridge between your Android browser " +
    "and <b>KOReader (Rakuyomi)</b> on your e-reader.\n\n" +
    "<b>Workflow</b>\n" +
    "1. Install <b>Get cookies.txt LOCALLY</b> extension in Kiwi Browser\n" +
    "2. Open the extension and tap <b>Export</b>\n" +
    "3. Open the downloaded file and copy the JSON array\n" +
    "4. Paste them into this chat\n" +
    "5. Pair your device with <code>/link CODE NAME</code>\n" +
    "6. Rakuyomi fetches cookies from the bot\n\n" +
    "<b>Cookie Syntax</b>\n" +
    '<code>[{"name":"...","value":"...","domain":".example.com"}]</code>\n' +
    "Prefix with device name: <code>kindle_bedroom [{...}]</code>\n\n" +
    "<b>Get cookies.txt LOCALLY</b>\n" +
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n\n" +
    "<b>Commands</b>\n" +
    "/link [CODE] [NAME] — Link a device\n" +
    "/unlink [NAME] — Unlink a device\n" +
    "/devices — List linked devices\n" +
    "/cookies [DEVICE] — View stored cookies\n" +
    "/app — Open mini app\n" +
    "/clearcookies [DEVICE] [DOMAIN] — Clear cookies\n" +
    "/status — Show bot status\n" +
    "/language — Change language\n" +
    "/github — Source code & issues\n" +
    "/donate — Support development",

  github: "<b>Rakuyomi is open-source!</b>\n\n" +
    "GitHub: https://github.com/tachibana-shin/rakuyomi\n" +
    "Issues & feature requests: https://github.com/tachibana-shin/rakuyomi/issues",

  donate:
    "If you find this project useful, consider supporting the developer:\n\n" +
    "<b>Ko-fi:</b> https://ko-fi.com/tachib_shin\n" +
    "<b>Momo:</b> https://me.momo.vn/tachibshin",

  language_prompt: "Choose your language:",

  language_set: (lang: string) => `<b>Language set to</b> <code>${lang}</code>.`,

  unlink_usage: "<b>Usage:</b> /unlink DEVICE_NAME",
  unlink_not_found: (device: string) =>
    `No data found for <code>${device}</code>. It may already be removed.`,
  unlink_done: (device: string) =>
    `<b>Unlinked</b> <code>${device}</code> — cookies and pairing info removed.`,
  unknown_command: "Unknown command. Type /help to see available commands.",

  cookie_needs_update: (device: string, url: string) =>
    `⚠️ <b>Cookie Update Required</b>\n\n` +
    `Device <code>${device}</code> encountered a 403 on <code>${url}</code> ` +
    `even after syncing cookies.\n\n` +
    `Please send fresh cookies for this device via Telegram.`,
}

export default en
