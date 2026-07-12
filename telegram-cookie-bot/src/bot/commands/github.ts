import { Context } from "grammy"
import { t } from "../../i18n.ts"
import { getChatId } from "./utils.ts"

export async function githubCommand(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return
  await ctx.reply(t(chatId).github)
}
