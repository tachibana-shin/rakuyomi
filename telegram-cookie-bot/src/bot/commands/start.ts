import { Context } from "grammy"
import { detectLanguage, getChatLang, setChatLang, t } from "../../i18n.ts"
import { getChatId } from "./utils.ts"

export async function startCommand(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return

  // Auto-detect language from Telegram user profile if not yet set
  const currentLang = await getChatLang(chatId)
  if (currentLang === "en") {
    const detected = detectLanguage(ctx.from?.language_code)
    if (detected !== "en") {
      await setChatLang(chatId, detected)
    }
  }

  await ctx.reply((await t(chatId)).welcome)
}
