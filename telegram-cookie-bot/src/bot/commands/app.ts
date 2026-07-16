import { Context, InlineKeyboard } from "grammy"
import { t } from "../../i18n.ts"
import { getConfig } from "../../../config.ts"

export async function appCommand(ctx: Context) {
  const chatId = ctx.chat?.id
  if (!chatId) return

  const { PUBLIC_URL } = getConfig()
  if (!PUBLIC_URL) {
    await ctx.reply(
      "Mini App is not configured. Set PUBLIC_URL environment variable.",
    )
    return
  }

  const locale = await t(chatId)
  const url = `${PUBLIC_URL}/webapp/cookies`
  const keyboard = new InlineKeyboard().webApp(
    locale.cookies_view_in_webapp,
    url,
  )
  await ctx.reply(locale.app_prompt, { reply_markup: keyboard })
}
