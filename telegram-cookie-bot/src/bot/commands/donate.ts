import { Context } from "grammy"
import { t } from "../../i18n.ts"
import { getChatId } from "./utils.ts"

export async function donateCommand(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return
  await ctx.reply((await t(chatId)).donate)
}
