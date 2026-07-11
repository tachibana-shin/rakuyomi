import { Context } from "grammy"
import { t } from "../../i18n.ts"
import { getPairingPendingCount } from "../../kv.ts"
import { getChatId } from "./utils.ts"

export async function statusCommand(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return
  const locale = t(chatId)

  const pending = await getPairingPendingCount()

  await ctx.reply(
    locale.status_online(String(chatId), pending),
  )
}
