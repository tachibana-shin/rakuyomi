import type { Locale } from "./locales/en.ts"
import en from "./locales/en.ts"
import vi from "./locales/vi.ts"
import jp from "./locales/jp.ts"
import zhHk from "./locales/zh-hk.ts"
import zhCn from "./locales/zh-cn.ts"
import { LanguageCode } from "grammy/types"
import { getChatLang as getDbChatLang, setChatLang as setDbChatLang } from "./turso.ts"

const locales: Record<string, Locale> = {
  en,
  vi,
  jp,
  "zh-hk": zhHk,
  "zh-cn": zhCn,
}

// Registers bot command menu per language (Telegram BCP-47 codes)
export const LANG_MAP: Record<string, LanguageCode> = {
  en: "en",
  vi: "vi",
  jp: "ja",
  // "zh-hk": "zh",
  "zh-cn": "zh",
}

// In-memory cache: loaded from DB on first access per chat
const chatLang = new Map<number, string>()
const loading = new Set<number>()

async function ensureChatLangLoaded(chatId: number): Promise<void> {
  if (chatLang.has(chatId)) return
  if (loading.has(chatId)) {
    while (loading.has(chatId)) await new Promise((r) => setTimeout(r, 10))
    return
  }
  loading.add(chatId)
  try {
    const dbLang = await getDbChatLang(chatId)
    if (dbLang) chatLang.set(chatId, dbLang)
  } finally {
    loading.delete(chatId)
  }
}

export function getLocale(lang: string): Locale {
  return locales[lang] ?? en
}

export async function getChatLang(chatId: number): Promise<string> {
  await ensureChatLangLoaded(chatId)
  return chatLang.get(chatId) ?? "en"
}

export async function setChatLang(chatId: number, lang: string) {
  chatLang.set(chatId, lang)
  await setDbChatLang(chatId, lang)
}

export async function t(chatId: number): Promise<Locale> {
  const lang = await getChatLang(chatId)
  return getLocale(lang)
}

// Detect best supported language from Telegram's language_code
export function detectLanguage(tgLangCode: string | undefined): string {
  if (!tgLangCode) return "en"
  const lang = tgLangCode.split("-")[0].toLowerCase()
  if (locales[lang]) return lang
  if (lang === "zh") {
    if (tgLangCode.toLowerCase() === "zh-hk" || tgLangCode.toLowerCase() === "zh-tw") {
      return "zh-hk"
    }
    return "zh-cn"
  }
  return "en"
}

export const SUPPORTED_LANGUAGES = [
  { code: "en", label: "English" },
  { code: "vi", label: "Tiếng Việt" },
  { code: "jp", label: "日本語" },
  { code: "zh-hk", label: "繁體中文（香港）" },
  { code: "zh-cn", label: "简体中文" },
]
