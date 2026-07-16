import type { Locale } from "./en.ts"

const zhCn: Locale = {
  command_start: "启动机器人",
  command_link: "链接设备",
  command_devices: "已链接设备列表",
  command_cookies: "查看已存储 Cookie",
  command_clearcookies: "清除已存储 Cookie",
  command_status: "机器人状态",
  command_help: "帮助",
  command_language: "切换语言",
  command_github: "源代码",
  command_donate: "支持开发",
  command_app: "打开mini app查看cookie",
  app_prompt: "打开mini app浏览已存储的cookie。",

  welcome: "欢迎使用 Rakuyomi Cookie Sync Bot！\n\n" +
    "此机器人可将 Android 设备（Kiwi Browser）上的 " +
    "Cloudflare Cookie 同步至运行 KOReader（Rakuyomi）的 Kindle。\n\n" +
    "开始使用:\n" +
    "1. 打开 KOReader → Rakuyomi → Cookie Sync\n" +
    "2. 点击「Pair Device」并输入机器人 URL\n" +
    "3. 使用以下命令发送配对码:\n" +
    "   /link 配对码 设备名称\n\n" +
    "发送 Cookie:\n" +
    "1. 在 Kiwi Browser 安装 Get cookies.txt LOCALLY 扩展\n" +
    "   https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n" +
    "2. 打开扩展并点击 Export\n" +
    "3. 打开下载的文件并复制 JSON 数组\n" +
    "4. 将 JSON 数组直接粘贴到此聊天\n" +
    "   可选：在前面加上设备名称以指定设备\n\n" +
    "命令:\n" +
    "/link [配对码] [名称] — 链接设备\n" +
    "/devices — 已链接设备列表\n" +
    "/cookies [名称] — 查看 Cookie\n" +
    "/clearcookies [名称] [域名] — 清除 Cookie\n" +
    "/status — 机器人状态\n" +
    "/help — 帮助\n" +
    "/language — 切换语言",

  link_usage: "用法: /link [配对码] [设备名称]\n\n" +
    "示例: /link A8F27K9X kindle_bedroom",

  link_invalid_code: "配对码无效或已过期。" +
    "请在 KOReader → Rakuyomi → Cookie Sync 中生成新的配对码。",

  link_no_chat_id: "无法确定聊天 ID。",

  link_success: (name: string) => `设备 [${name}] 已成功链接！`,

  devices_none: "尚未链接任何设备。请使用 /link 配对码 名称 来链接设备。",

  devices_list: (lines: string) => `已链接设备:\n${lines}`,

  status_online: (chatId: string, pending: number) =>
    "Rakuyomi Cookie Sync Bot\n\n" +
    `聊天 ID: ${chatId}\n` +
    `待处理配对: ${pending}\n` +
    "状态: 在线",

  cookie_received: (domains: string, device: string) =>
    `已收到以下域名的 Cookie: ${domains}\n` +
    `目标设备: ${device}`,

  cookie_syntax:
    "从 Get cookies.txt LOCALLY 扩展发送 Cookie，请将 JSON 数组直接粘贴到聊天:\n" +
    "[{...}]\n\n" +
    "为特定设备指定 Cookie，在前面加上设备名称:\n" +
    "设备名称 [{...}]",

  cookie_invalid_json: "JSON 格式无效。" +
    "请粘贴 Get cookies.txt LOCALLY 的有效 JSON 数组。",

  cookie_invalid_format: "Cookie 格式无效。" +
    "每个条目至少需要 name、value 和 domain 字段。\n\n" +
    "示例:\n" +
    '[{"name":"cf_clearance","value":"abc","domain":".example.com"}]',

  cookie_extension_url:
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc",

  cookies_none: "尚未存储任何 Cookie。",
  cookies_header: (device: string) => `设备 [${device}] 的 Cookie:`,
  cookies_list: (domains: string) => `域名:\n${domains}`,
  cookies_device_list: (lines: string) => `已存储 Cookie:\n${lines}`,
  cookies_view_in_webapp: "📋 在Web App中查看",
  cookies_too_large:
    "Cookie数据太大，无法在此显示。打开Web App查看所有Cookie。",

  clearcookies_all_done: "已清除所有 Cookie。",
  clearcookies_device_done: (device: string) =>
    `已清除设备 [${device}] 的所有 Cookie。`,
  clearcookies_domain_done: (domain: string, device: string) =>
    `已清除设备 [${device}] 上域名 ${domain} 的 Cookie。`,
  clearcookies_none: "找不到要清除的 Cookie。",
  clearcookies_usage: "用法:\n/clearcookies — 清除所有 Cookie\n" +
    "/clearcookies 设备名称 — 清除设备的 Cookie\n" +
    "/clearcookies 设备名称 域名 — 清除指定域名的 Cookie",

  help: "Rakuyomi Cookie Sync Bot — 帮助\n\n" +
    "此机器人作为 Android 浏览器与 " +
    "KOReader（Rakuyomi）之间的桥梁。\n\n" +
    "工作流程:\n" +
    "1. 在 Kiwi Browser 安装 Get cookies.txt LOCALLY 扩展\n" +
    "2. 打开扩展并点击 Export\n" +
    "3. 打开下载的文件并复制 JSON 数组\n" +
    "4. 粘贴到此聊天（普通文本消息）\n" +
    "5. 使用 /link 配对码 名称 配对设备\n" +
    "6. Rakuyomi 从机器人获取 Cookie\n\n" +
    "<b>Cookie 语法</b>\n" +
    '[{"name":"...","value":"...","domain":".example.com"}]\n' +
    "前面加上设备名称: kindle_bedroom [{...}]\n\n" +
    "Get cookies.txt LOCALLY 扩展:\n" +
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n\n" +
    "命令:\n" +
    "/link [配对码] [名称] — 链接设备\n" +
    "/devices — 已链接设备\n" +
    "/cookies [名称] — 查看 Cookie\n" +
    "/app — 打开mini app查看cookie\n" +
    "/clearcookies [名称] [域名] — 清除 Cookie\n" +
    "/status — 状态\n" +
    "/language — 切换语言\n" +
    "/github — 源代码\n" +
    "/donate — 支持开发",

  github: "Rakuyomi 是开源软件！\n\n" +
    "GitHub: https://github.com/tachibana-shin/rakuyomi\n" +
    "报告问题: https://github.com/tachibana-shin/rakuyomi/issues",

  donate: "如果您觉得此项目有用，请考虑支持开发者:\n\n" +
    "Ko-fi: https://ko-fi.com/tachib_shin\n" +
    "Momo: https://me.momo.vn/tachibshin",

  language_prompt: "请选择您的语言:",

  language_set: (lang: string) => `语言已切换为 ${lang}。`,

  unlink_usage: "<b>Usage:</b> /unlink DEVICE_NAME",
  unlink_not_found: (device: string) =>
    `No data found for <code>${device}</code>. It may already be removed.`,
  unlink_done: (device: string) =>
    `<b>Unlinked</b> <code>${device}</code> — cookies and pairing info removed.`,
  unknown_command: "未知命令。请输入 /help 查看可用命令。",

  cookie_needs_update: (device: string, url: string) =>
    `⚠️ <b>需要更新 Cookie</b>\n\n` +
    `设备 <code>${device}</code> 在 <code>${url}</code> 同步 Cookie 后仍然遇到 403 错误。\n\n` +
    `请通过 Telegram 发送该设备的新 Cookie。`,
}

export default zhCn
