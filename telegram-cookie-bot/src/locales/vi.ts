import type { Locale } from "./en.ts"

const vi: Locale = {
  command_start: "Khởi động bot",
  command_link: "Ghép đôi thiết bị với mã",
  command_devices: "Danh sách thiết bị đã ghép",
  command_cookies: "Xem cookie đã lưu",
  command_clearcookies: "Xóa cookie đã lưu",
  command_status: "Trạng thái bot",
  command_help: "Trợ giúp",
  command_language: "Đổi ngôn ngữ",
  command_github: "Mã nguồn",
  command_donate: "Ủng hộ",
  command_app: "Mở mini app xem cookie",
  app_prompt: "Mở mini app để duyệt cookie đã lưu.",

  welcome: "<b>Rakuyomi Cookie Sync Bot</b>\n\n" +
    "Bot này đồng bộ cookie Cloudflare từ thiết bị Android " +
    "(Kiwi Browser) sang Kindle chạy KOReader (Rakuyomi).\n\n" +
    "<b>Bắt đầu</b>\n" +
    "1. Mở <b>KOReader → Rakuyomi → Cookie Sync</b>\n" +
    "2. Nhấn <b>Pair Device</b> và nhập URL của bot\n" +
    "3. Gửi mã ghép đôi cho bot:\n" +
    "   <code>/link MÃ_SỐ TÊN_THIẾT_BỊ</code>\n\n" +
    "<b>Gửi cookie</b>\n" +
    "1. Cài <b>Get cookies.txt LOCALLY</b> trong Kiwi Browser\n" +
    "   https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n" +
    "2. Mở extension và nhấn <b>Export</b>\n" +
    "3. Dán JSON array vào chat này\n" +
    "   Thêm tên thiết bị: <code>TÊN_THIẾT_BỊ [{...}]</code>\n\n" +
    "<b>Lệnh</b>\n" +
    "/link [MÃ] [TÊN] — Ghép đôi thiết bị\n" +
    "/unlink [TÊN] — Gỡ thiết bị\n" +
    "/devices — Danh sách thiết bị\n" +
    "/cookies [TÊN] — Xem cookie\n" +
    "/app — Mở mini app xem cookie\n" +
    "/clearcookies [TÊN] [TÊN_MIỀN] — Xóa cookie\n" +
    "/status — Trạng thái bot\n" +
    "/help — Trợ giúp\n" +
    "/language — Đổi ngôn ngữ",

  link_usage: "<b>Cú pháp:</b> /link MÃ_SỐ TÊN_THIẾT_BỊ\n\n" +
    "Ví dụ: <code>/link A8F27K9X kindle_phong_ngu</code>",

  link_invalid_code: "Mã ghép đôi không hợp lệ hoặc đã hết hạn.\n" +
    "Hãy tạo mã mới từ <b>KOReader → Rakuyomi → Cookie Sync</b>.",

  link_no_chat_id: "Không thể xác định ID chat.",

  link_success: (name: string) =>
    `<b>Ghép đôi thành công!</b>\n\n<code>${name}</code> đã được kết nối với chat này.`,

  devices_none:
    "Chưa có thiết bị nào.\nDùng <code>/link MÃ TÊN</code> để ghép đôi.",

  devices_list: (lines: string) => `<b>Thiết bị đã ghép:</b>\n${lines}`,

  status_online: (chatId: string, pending: number) =>
    "<b>Rakuyomi Cookie Sync Bot</b>\n\n" +
    `Chat ID: <code>${chatId}</code>\n` +
    `Số ghép đôi đang chờ: <b>${pending}</b>\n` +
    "Trạng thái: <b>Đang hoạt động</b>",

  cookie_received: (domains: string, device: string) =>
    `<b>Đã nhận cookie</b>\n` +
    `Thiết bị: <code>${device}</code>\n` +
    `Tên miền: <code>${domains}</code>`,

  cookie_syntax:
    "Để gửi cookie từ Get cookies.txt LOCALLY, dán JSON array vào chat:\n" +
    "<code>[{...}]</code>\n\n" +
    "Gán cho thiết bị cụ thể, thêm tên làm prefix:\n" +
    "<code>TÊN_THIẾT_BỊ [{...}]</code>",

  cookie_invalid_json: "Định dạng JSON không hợp lệ.\n" +
    "Vui lòng dán JSON array hợp lệ từ <b>Get cookies.txt LOCALLY</b>.",

  cookie_invalid_format: "Định dạng cookie không hợp lệ.\n" +
    "Mỗi mục phải có <code>name</code>, <code>value</code> và <code>domain</code>.\n\n" +
    "Ví dụ:\n" +
    '<code>[{"name":"cf_clearance","value":"abc","domain":".example.com"}]</code>',

  cookie_extension_url:
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc",

  cookies_none: "Chưa có cookie nào được lưu.",
  cookies_header: (device: string) =>
    `<b>Cookie cho</b> <code>${device}</code>`,
  cookies_list: (domains: string) => `<b>Tên miền:</b>\n${domains}`,
  cookies_device_list: (lines: string) => `<b>Cookie đã lưu:</b>\n${lines}`,
  cookies_view_in_webapp: "Xem trong Web App",
  cookies_too_large:
    "Dữ liệu cookie quá lớn để hiển thị ở đây.\nMở <b>Web App</b> để xem tất cả cookie.",

  clearcookies_all_done: "<b>Đã xóa toàn bộ cookie</b>.",
  clearcookies_device_done: (device: string) =>
    `<b>Đã xóa toàn bộ cookie</b> của <code>${device}</code>.`,
  clearcookies_domain_done: (domain: string, device: string) =>
    `<b>Đã xóa cookie</b> của <code>${domain}</code> trên <code>${device}</code>.`,
  clearcookies_none: "Không tìm thấy cookie để xóa.",
  clearcookies_usage: "<b>Cú pháp:</b>\n" +
    "/clearcookies — Xóa toàn bộ cookie\n" +
    "/clearcookies THIẾT_BỊ — Xóa cookie của thiết bị\n" +
    "/clearcookies THIẾT_BỊ TÊN_MIỀN — Xóa cookie của tên miền",

  help: "<b>Rakuyomi Cookie Sync Bot — Trợ giúp</b>\n\n" +
    "Bot này hoạt động như cầu nối giữa trình duyệt Android " +
    "và <b>KOReader (Rakuyomi)</b> trên máy đọc sách.\n\n" +
    "<b>Quy trình</b>\n" +
    "1. Cài <b>Get cookies.txt LOCALLY</b> trong Kiwi Browser\n" +
    "2. Mở extension và nhấn <b>Export</b>\n" +
    "3. Mở file đã tải về và copy JSON array\n" +
    "4. Dán vào chat\n" +
    "5. Ghép đôi với <code>/link MÃ TÊN</code>\n" +
    "6. Rakuyomi lấy cookie từ bot\n\n" +
    "<b>Cú pháp cookie</b>\n" +
    '<code>[{"name":"...","value":"...","domain":".example.com"}]</code>\n' +
    "Thêm prefix tên thiết bị: <code>kindle_bedroom [{...}]</code>\n\n" +
    "<b>Get cookies.txt LOCALLY</b>\n" +
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n\n" +
    "<b>Lệnh</b>\n" +
    "/link [MÃ] [TÊN] — Ghép đôi thiết bị\n" +
    "/unlink [TÊN] — Gỡ thiết bị\n" +
    "/devices — Danh sách thiết bị\n" +
    "/cookies [TÊN] — Xem cookie\n" +
    "/app — Mở mini app xem cookie\n" +
    "/clearcookies [TÊN] [TÊN_MIỀN] — Xóa cookie\n" +
    "/status — Trạng thái\n" +
    "/language — Đổi ngôn ngữ\n" +
    "/github — Mã nguồn\n" +
    "/donate — Ủng hộ",

  github: "<b>Rakuyomi là mã nguồn mở!</b>\n\n" +
    "GitHub: https://github.com/tachibana-shin/rakuyomi\n" +
    "Báo lỗi & góp ý: https://github.com/tachibana-shin/rakuyomi/issues",

  donate: "Nếu bạn thấy dự án hữu ích, hãy cân nhắc ủng hộ:\n\n" +
    "<b>Ko-fi:</b> https://ko-fi.com/tachib_shin\n" +
    "<b>Momo:</b> https://me.momo.vn/tachibshin",

  language_prompt: "Chọn ngôn ngữ của bạn:",

  language_set: (lang: string) => `<b>Đã chuyển sang</b> <code>${lang}</code>.`,

  unlink_usage: "<b>Cú pháp:</b> /unlink TÊN_THIẾT_BỊ",
  unlink_not_found: (device: string) =>
    `Không tìm thấy dữ liệu của <code>${device}</code>. Có thể đã được gỡ trước đó.`,
  unlink_done: (device: string) =>
    `<b>Đã gỡ</b> <code>${device}</code> — cookie và thông tin ghép đôi đã xóa.`,
  unknown_command: "Lệnh không hợp lệ. Gõ /help để xem danh sách lệnh.",

  cookie_needs_update: (device: string, url: string) =>
    `⚠️ <b>Cần cập nhật Cookie</b>\n\n` +
    `Thiết bị <code>${device}</code> bị 403 tại <code>${url}</code> ` +
    `ngay cả sau khi đồng bộ cookie.\n\n` +
    `Vui lòng gửi cookie mới cho thiết bị này qua Telegram.`,
}

export default vi
