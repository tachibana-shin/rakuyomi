import { Context, InlineKeyboard } from "grammy"
import { setChatLang, SUPPORTED_LANGUAGES, t } from "../../i18n.ts"
import { getChatId } from "./utils.ts"

export async function languageCommand(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return

  const keyboard = InlineKeyboard.from(
    SUPPORTED_LANGUAGES.map((lang) => [InlineKeyboard.text(
      lang.label,
      `lang:${lang.code}`,
    )]),
  )

  await ctx.reply(t(chatId).language_prompt, {
    reply_markup: keyboard,
  })
}

export async function handleLanguageCallback(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId || !ctx.callbackQuery?.data) return

  const langCode = ctx.callbackQuery.data.replace("lang:", "")
  const lang = SUPPORTED_LANGUAGES.find((l) => l.code === langCode)
  if (!lang) return

  setChatLang(chatId, langCode)
  await ctx.answerCallbackQuery()
  await ctx.reply(t(chatId).language_set(lang.label))
}
