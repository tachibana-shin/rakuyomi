import type { Locale } from "./locales/en.ts"
import en from "./locales/en.ts"
import vi from "./locales/vi.ts"
import jp from "./locales/jp.ts"
import zhHk from "./locales/zh-hk.ts"
import zhCn from "./locales/zh-cn.ts"
import { LanguageCode } from "grammy/types"

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

// Per-chat language preference (in-memory, survives bot lifetime)
const chatLang = new Map<number, string>()

export function getLocale(lang: string): Locale {
  return locales[lang] ?? en
}

export function getChatLang(chatId: number): string {
  return chatLang.get(chatId) ?? "en"
}

export function setChatLang(chatId: number, lang: string) {
  chatLang.set(chatId, lang)
}

export function t(chatId: number): Locale {
  const lang = getChatLang(chatId)
  return getLocale(lang)
}

export const SUPPORTED_LANGUAGES = [
  { code: "en", label: "English" },
  { code: "vi", label: "Tiếng Việt" },
  { code: "jp", label: "日本語" },
  { code: "zh-hk", label: "繁體中文（香港）" },
  { code: "zh-cn", label: "简体中文" },
]
