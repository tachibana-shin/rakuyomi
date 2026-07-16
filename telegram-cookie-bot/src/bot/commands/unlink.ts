import { Context } from "grammy"
import { t } from "../../i18n.ts"
import { clearDeviceCookies } from "../../store.ts"
import { removePairingByDevice } from "../../kv.ts"
import { removeDevice } from "../../turso.ts"
import { getChatId } from "./utils.ts"

export async function unlinkCommand(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return
  const locale = await t(chatId)

  const text = ctx.message?.text ?? ""
  const parts = text.split(/\s+/)
  const deviceName = parts[1]

  if (!deviceName) {
    await ctx.reply(locale.unlink_usage)
    return
  }

  const removedCookies = await clearDeviceCookies(chatId, deviceName)

  const removedPairing = await removePairingByDevice(chatId, deviceName)
  await removeDevice(chatId, deviceName)

  if (!removedCookies && !removedPairing) {
    await ctx.reply(locale.unlink_not_found(deviceName))
    return
  }

  await ctx.reply(locale.unlink_done(deviceName))
}
