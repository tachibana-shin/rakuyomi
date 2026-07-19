import { Context, InlineKeyboard } from "grammy"
import {
  getDeviceCookieCount,
  getDeviceDomains,
  getDevices,
  getDomainCookieCount,
} from "../../store.ts"
import { t } from "../../i18n.ts"
import { getConfig } from "../../config.ts"

const MAX_LENGTH = 3500

export async function cookiesCommand(ctx: Context) {
  const chatId = ctx.chat?.id
  if (!chatId) return

  const locale = await t(chatId)
  const devices = await getDevices(chatId)
  if (devices.length === 0) {
    await ctx.reply(locale.cookies_none)
    return
  }

  const args = ctx.message?.text?.split(/\s+/).slice(1) ?? []
  const deviceName = args[0] || (devices.includes("/all") ? "/all" : devices[0])

  const domains = await getDeviceDomains(chatId, deviceName)
  if (domains.length === 0) {
    await ctx.reply(
      `${locale.cookies_header(deviceName)}\n${locale.cookies_none}`,
    )
    return
  }
  const { cookies } = await getDeviceCookieCount(chatId, deviceName)
  const counts = await Promise.all(
    domains.map((d) => getDomainCookieCount(chatId, deviceName, d)),
  )
  const lines = domains.map((d, i) => {
    const n = counts[i]
    return `• ${d} — ${n} cookie${n !== 1 ? "s" : ""}`
  }).join("\n")
  const text = `${locale.cookies_header(deviceName)} (${cookies} total)\n\n${
    locale.cookies_list(lines)
  }`
  await sendWithAppButton(ctx, chatId, text, deviceName)
}

async function sendWithAppButton(
  ctx: Context,
  chatId: number,
  text: string,
  device: string,
) {
  const { PUBLIC_URL } = getConfig()

  const truncated = text.length > MAX_LENGTH
    ? text.substring(0, MAX_LENGTH) + "\n\n…"
    : text

  if (!PUBLIC_URL) {
    await ctx.reply(truncated)
    return
  }

  const locale = await t(chatId)
  const url = `${PUBLIC_URL}/webapp/cookies?chat_id=${chatId}&device=${
    encodeURIComponent(device)
  }`
  const keyboard = new InlineKeyboard().webApp(
    locale.cookies_view_in_webapp,
    url,
  )
  await ctx.reply(truncated, { reply_markup: keyboard })
}
