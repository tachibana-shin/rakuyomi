import { Context } from "grammy"
import { t } from "../../i18n.ts"
import { resolvePairingCode } from "../../kv.ts"
import { registerDevice } from "../../turso.ts"
import { storeChatToken } from "../../store.ts"
import { getChatId } from "./utils.ts"

export async function linkCommand(ctx: Context) {
  const chatId = getChatId(ctx)
  if (!chatId) return
  const locale = t(chatId)

  const text = ctx.message?.text ?? ""
  const parts = text.split(/\s+/)

  if (parts.length < 3) {
    await ctx.reply(locale.link_usage)
    return
  }

  const code = parts[1].toUpperCase()

  // Only accept 8-char alphanumeric pairing codes
  if (!/^[A-Z0-9]{8}$/.test(code)) {
    await ctx.reply(locale.link_usage)
    return
  }

  const deviceName = parts.slice(2).join("_")

  const apiToken = await resolvePairingCode(code, chatId, deviceName)
  if (!apiToken) {
    await ctx.reply(locale.link_invalid_code)
    return
  }

  await storeChatToken(chatId, apiToken)
  await registerDevice(chatId, deviceName)
  await ctx.reply(locale.link_success(deviceName))
}
