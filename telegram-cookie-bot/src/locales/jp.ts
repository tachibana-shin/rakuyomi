import type { Locale } from "./en.ts"

const jp: Locale = {
  command_start: "ボットを起動",
  command_link: "デバイスをペアリング",
  command_devices: "リンク済みデバイス一覧",
  command_cookies: "保存されたCookieを表示",
  command_clearcookies: "保存されたCookieを削除",
  command_status: "ボットの状態",
  command_help: "ヘルプ",
  command_language: "言語設定",
  command_github: "ソースコード",
  command_donate: "開発支援",
  command_app: "ミニアプリを開く",
  app_prompt: "ミニアプリを開いて保存済みCookieを確認します。",

  welcome: "Rakuyomi Cookie Sync Bot へようこそ！\n\n" +
    "このボットは、Android端末（Kiwi Browser）の " +
    "Cloudflare cookieを、KOReader（Rakuyomi）を実行しているKindleと同期します。\n\n" +
    "開始方法:\n" +
    "1. KOReader → Rakuyomi → Cookie Sync を開く\n" +
    "2. 「Pair Device」をタップしてボットのURLを入力\n" +
    "3. 以下のコマンドでペアリングコードを送信:\n" +
    "   /link コード デバイス名\n\n" +
    "Cookieの送信:\n" +
    "1. Kiwi BrowserにGet cookies.txt LOCALLY拡張をインストール\n" +
    "   https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n" +
    "2. 拡張機能を開いてExportをタップ\n" +
    "3. ダウンロードしたファイルを開いてJSON配列をコピー\n" +
    "4. JSON配列をこのチャットに貼り付け\n" +
    "   先頭にデバイス名を追加すると割り当て可能\n\n" +
    "コマンド:\n" +
    "/link [コード] [名前] — デバイスをリンク\n" +
    "/devices — リンク済みデバイス一覧\n" +
    "/cookies [名前] — Cookieを表示\n" +
    "/clearcookies [名前] [ドメイン] — Cookieを削除\n" +
    "/status — ボットの状態\n" +
    "/help — ヘルプ\n" +
    "/language — 言語設定",

  link_usage: "使用方法: /link [コード] [デバイス名]\n\n" +
    "例: /link A8F27K9X kindle_bedroom",

  link_invalid_code: "ペアリングコードが無効または期限切れです。 " +
    "KOReader → Rakuyomi → Cookie Sync で新しいコードを生成してください。",

  link_no_chat_id: "チャットIDを特定できません。",

  link_success: (name: string) => `デバイス [${name}] のリンクに成功しました！`,

  devices_none:
    "まだデバイスがリンクされていません。/link コード 名前 でリンクしてください。",

  devices_list: (lines: string) => `リンク済みデバイス:\n${lines}`,

  status_online: (chatId: string, pending: number) =>
    "Rakuyomi Cookie Sync Bot\n\n" +
    `チャットID: ${chatId}\n` +
    `保留中のペアリング: ${pending}\n` +
    "状態: オンライン",

  cookie_received: (domains: string, device: string) =>
    `ドメインのcookieを受信しました: ${domains}\n` +
    `対象デバイス: ${device}`,

  cookie_syntax:
    "Get cookies.txt LOCALLY拡張からCookieを送信するには、JSON配列をチャットに貼り付け:\n" +
    "[{...}]\n\n" +
    "特定のデバイスに割り当てる場合は、先頭にデバイス名を追加:\n" +
    "デバイス名 [{...}]",

  cookie_invalid_json: "JSON形式が無効です。" +
    "Get cookies.txt LOCALLYから有効なJSON配列を貼り付けてください。",

  cookie_invalid_format: "Cookie形式が無効です。" +
    "各エントリには少なくともname、value、domainが必要です。\n\n" +
    "例:\n" +
    '[{"name":"cf_clearance","value":"abc","domain":".example.com"}]',

  cookie_extension_url:
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc",

  cookies_none: "保存されたCookieはありません。",
  cookies_header: (device: string) => `デバイス [${device}] のCookie:`,
  cookies_list: (domains: string) => `ドメイン:\n${domains}`,
  cookies_device_list: (lines: string) => `保存されたCookie:\n${lines}`,
  cookies_view_in_webapp: "📋 Web Appで表示",
  cookies_too_large:
    "Cookieデータが大きすぎてここに表示できません。Web Appを開いてすべてのCookieを表示してください。",

  clearcookies_all_done: "すべてのCookieを削除しました。",
  clearcookies_device_done: (device: string) =>
    `デバイス [${device}] のすべてのCookieを削除しました。`,
  clearcookies_domain_done: (domain: string, device: string) =>
    `デバイス [${device}] のドメイン ${domain} のCookieを削除しました。`,
  clearcookies_none: "削除するCookieが見つかりません。",
  clearcookies_usage: "使用方法:\n/clearcookies — すべてのCookieを削除\n" +
    "/clearcookies デバイス名 — デバイスのCookieを削除\n" +
    "/clearcookies デバイス名 ドメイン — ドメインのCookieを削除",

  help: "Rakuyomi Cookie Sync Bot — ヘルプ\n\n" +
    "このボットはAndroidブラウザと " +
    "KOReader（Rakuyomi）の間の橋渡しをします。\n\n" +
    "流れ:\n" +
    "1. Kiwi BrowserにGet cookies.txt LOCALLY拡張をインストール\n" +
    "2. 拡張機能を開いてExportをタップ\n" +
    "3. ダウンロードしたファイルを開いてJSON配列をコピー\n" +
    "4. このチャットに貼り付け（通常のテキストメッセージ）\n" +
    "5. /link コード 名前 でデバイスをペアリング\n" +
    "6. RakuyomiがボットからCookieを取得\n\n" +
    "Cookie構文:\n" +
    '[{"name":"...","value":"...","domain":".example.com"}]\n' +
    "先頭にデバイス名を追加: kindle_bedroom [{...}]\n\n" +
    "Get cookies.txt LOCALLY拡張機能:\n" +
    "https://chromewebstore.google.com/detail/get-cookiestxt-locally/cclelndahbckbenkjhflpdbgdldlbecc\n\n" +
    "コマンド:\n" +
    "/link [コード] [名前] — デバイスをリンク\n" +
    "/devices — リンク済みデバイス\n" +
    "/cookies [名前] — Cookieを表示\n" +
    "/app — ミニアプリを開く\n" +
    "/clearcookies [名前] [ドメイン] — Cookieを削除\n" +
    "/status — 状態\n" +
    "/language — 言語設定\n" +
    "/github — ソースコード\n" +
    "/donate — 開発支援",

  github: "Rakuyomiはオープンソースです！\n\n" +
    "GitHub: https://github.com/tachibana-shin/rakuyomi\n" +
    "Issues: https://github.com/tachibana-shin/rakuyomi/issues",

  donate: "このプロジェクトが役に立ったなら、開発者を支援してください:\n\n" +
    "Ko-fi: https://ko-fi.com/tachib_shin\n" +
    "Momo: https://me.momo.vn/tachibshin",

  language_prompt: "言語を選択してください:",

  language_set: (lang: string) => `言語を ${lang} に変更しました。`,

  unlink_usage: "<b>Usage:</b> /unlink DEVICE_NAME",
  unlink_not_found: (device: string) =>
    `No data found for <code>${device}</code>. It may already be removed.`,
  unlink_done: (device: string) =>
    `<b>Unlinked</b> <code>${device}</code> — cookies and pairing info removed.`,
  unknown_command:
    "不明なコマンドです。/help でコマンド一覧を確認してください。",

  cookie_needs_update: (device: string, url: string) =>
    `⚠️ <b>Cookieの更新が必要です</b>\n\n` +
    `デバイス <code>${device}</code> が <code>${url}</code> で、Cookie同期後も403エラーを受け取りました。\n\n` +
    `新しいCookieをTelegram経由で送信してください。`,
}

export default jp
